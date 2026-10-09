use crate::{Plotter, Points, SeriesId};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender, TryRecvError},
};
use std::time::Duration;
use web_time::Instant;

const UPDATE_QUEUE_CAPACITY: usize = 64;

/// The result returned by a streaming computation.
pub type StreamResult<T = ()> = Result<T, StreamError>;

/// An error returned by a streaming computation.
pub type StreamError = Box<dyn Error + Send + Sync + 'static>;

/// A formatted plot and the background computation that will update it.
pub struct StreamPlan {
    plotter: Plotter,
    job: Box<dyn FnOnce(PlotSink, CancelToken) -> StreamResult + Send + 'static>,
}

impl StreamPlan {
    /// Creates a stream whose plot stays on the UI thread while `job` runs in
    /// the background.
    pub fn new(
        plotter: Plotter,
        job: impl FnOnce(PlotSink, CancelToken) -> StreamResult + Send + 'static,
    ) -> Self {
        Self {
            plotter,
            job: Box::new(job),
        }
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        Plotter,
        Box<dyn FnOnce(PlotSink, CancelToken) -> StreamResult + Send + 'static>,
    ) {
        (self.plotter, self.job)
    }
}

/// Cooperative cancellation for a streaming computation.
#[derive(Clone, Debug)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    /// Returns `true` after the app has asked this computation to stop.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Returned when an update can no longer reach its plot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamClosed;

impl Display for StreamClosed {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("the plot stream is closed")
    }
}

impl Error for StreamClosed {}

/// Sends plot updates from background computation to the UI thread.
///
/// Send points in batches rather than one update per point. The queue is
/// bounded, so producers wait if they outpace the UI instead of growing memory
/// without limit.
//
// Workers deliberately never retain an egui Context. On threaded Wasm, a
// worker-side request_repaint can contend with the UI's parking_lot locks and
// panic because parking is unsupported. App polls this queue and schedules the
// next repaint from the UI thread instead.
#[derive(Clone)]
pub struct PlotSink {
    updates: SyncSender<StreamUpdate>,
}

impl PlotSink {
    /// Appends a batch of points to a series.
    pub fn append(
        &self,
        series: SeriesId,
        points: impl IntoIterator<Item = [f64; 2]>,
    ) -> Result<(), StreamClosed> {
        self.send(StreamUpdate::Append(series, points.into_iter().collect()))
    }

    /// Replaces all points in a series.
    pub fn replace(&self, series: SeriesId, points: Points) -> Result<(), StreamClosed> {
        self.send(StreamUpdate::Replace(series, points))
    }

    /// Removes all points from a series.
    pub fn clear(&self, series: SeriesId) -> Result<(), StreamClosed> {
        self.send(StreamUpdate::Clear(series))
    }

    /// Reports completion in the range `0.0..=1.0`.
    pub fn set_progress(&self, progress: f32) -> Result<(), StreamClosed> {
        self.send(StreamUpdate::Progress(progress.clamp(0.0, 1.0)))
    }

    fn send(&self, update: StreamUpdate) -> Result<(), StreamClosed> {
        self.updates.send(update).map_err(|_| StreamClosed)
    }
}

enum StreamUpdate {
    Append(SeriesId, Points),
    Replace(SeriesId, Points),
    Clear(SeriesId),
    Progress(f32),
    Finished(Result<(), String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StreamStatus {
    Running,
    RestartPending,
    Stopping,
    Stopped,
    Finished,
    Failed,
}

impl StreamStatus {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::RestartPending => "Restart pending",
            Self::Stopping => "Stopping",
            Self::Stopped => "Stopped",
            Self::Finished => "Finished",
            Self::Failed => "Failed",
        }
    }
}

pub(crate) struct StreamState {
    pub(crate) plotter: Plotter,
    updates: Option<Receiver<StreamUpdate>>,
    cancel: CancelToken,
    status: StreamStatus,
    progress: Option<f32>,
    started: Instant,
    elapsed: Duration,
    error: Option<String>,
}

impl StreamState {
    pub(crate) fn start(plan: StreamPlan) -> Self {
        let (plotter, job) = plan.into_parts();
        let (sender, receiver) = mpsc::sync_channel(UPDATE_QUEUE_CAPACITY);
        let cancel = CancelToken::new();
        let worker_cancel = cancel.clone();
        let worker_sender = sender.clone();
        let sink = PlotSink { updates: sender };
        let mut state = Self {
            plotter,
            updates: Some(receiver),
            cancel,
            status: StreamStatus::Running,
            progress: None,
            started: Instant::now(),
            elapsed: Duration::ZERO,
            error: None,
        };

        let result = spawn_compute(move || {
            let result = job(sink, worker_cancel).map_err(|error| error.to_string());
            let _ = worker_sender.send(StreamUpdate::Finished(result));
        });
        if let Err(error) = result {
            state.updates = None;
            state.status = StreamStatus::Failed;
            state.error = Some(error);
        }
        state
    }

    pub(crate) fn drain(&mut self) {
        let Some(receiver) = &self.updates else {
            return;
        };
        let mut updates = Vec::new();
        let disconnected = loop {
            match receiver.try_recv() {
                Ok(update) => updates.push(update),
                Err(TryRecvError::Empty) => break false,
                Err(TryRecvError::Disconnected) => break true,
            }
        };

        let mut finished = false;
        for update in updates {
            self.elapsed = self.started.elapsed();
            match update {
                StreamUpdate::Append(series, points) => {
                    self.plotter.append_points(series, points);
                }
                StreamUpdate::Replace(series, points) => {
                    self.plotter.replace_points(series, points);
                }
                StreamUpdate::Clear(series) => {
                    self.plotter.clear_points(series);
                }
                StreamUpdate::Progress(progress) => self.progress = Some(progress),
                StreamUpdate::Finished(result) => {
                    finished = true;
                    match result {
                        Ok(()) if self.cancel.is_cancelled() => {
                            self.status = StreamStatus::Stopped;
                        }
                        Ok(()) => {
                            self.status = StreamStatus::Finished;
                            self.progress = Some(1.0);
                        }
                        Err(error) if self.cancel.is_cancelled() => {
                            self.status = StreamStatus::Stopped;
                            self.error = Some(error);
                        }
                        Err(error) => {
                            self.status = StreamStatus::Failed;
                            self.error = Some(error);
                        }
                    }
                }
            }
        }

        if finished {
            self.updates = None;
        } else if disconnected {
            self.updates = None;
            self.status = StreamStatus::Failed;
            self.error = Some("the computation ended without a final status".to_owned());
            self.elapsed = self.started.elapsed();
        }
    }

    pub(crate) fn stop(&mut self) {
        self.cancel.cancel();
        if self.updates.is_some() {
            self.status = StreamStatus::Stopping;
        } else {
            self.status = StreamStatus::Stopped;
        }
    }

    pub(crate) fn mark_restart_pending(&mut self) {
        self.cancel.cancel();
        self.status = StreamStatus::RestartPending;
    }

    pub(crate) fn status(&self) -> StreamStatus {
        self.status
    }

    pub(crate) fn progress(&self) -> Option<f32> {
        self.progress
    }

    pub(crate) fn elapsed(&self) -> Duration {
        self.elapsed
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn can_stop(&self) -> bool {
        matches!(
            self.status,
            StreamStatus::Running | StreamStatus::RestartPending
        )
    }

    pub(crate) fn needs_repaint_poll(&self) -> bool {
        self.updates.is_some()
    }
}

impl Drop for StreamState {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_compute(job: impl FnOnce() + Send + 'static) -> Result<(), String> {
    std::thread::Builder::new()
        .name("myplotlib-stream".to_owned())
        .spawn(job)
        .map(|_| ())
        .map_err(|error| format!("could not start computation: {error}"))
}

#[cfg(target_arch = "wasm32")]
fn spawn_compute(job: impl FnOnce() + Send + 'static) -> Result<(), String> {
    rayon::spawn(job);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plot_with_series() -> (Plotter, SeriesId) {
        let mut plot = Plotter::new();
        let series = plot.add_points(Vec::new()).id();
        (plot, series)
    }

    #[test]
    fn stream_applies_all_update_kinds_and_finishes() {
        let (plot, series) = plot_with_series();
        let plan = StreamPlan::new(plot, move |sink, _| {
            sink.append(series, [[0.0, 1.0], [1.0, 2.0]])?;
            sink.replace(series, vec![[2.0, 3.0]])?;
            sink.clear(series)?;
            sink.append(series, [[3.0, 4.0]])?;
            sink.set_progress(0.75)?;
            Ok(())
        });
        let mut state = StreamState::start(plan);

        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            state.drain();
            if state.status() == StreamStatus::Finished {
                break;
            }
            std::thread::yield_now();
        }

        assert_eq!(state.status(), StreamStatus::Finished);
        assert_eq!(state.progress(), Some(1.0));
    }

    #[test]
    fn cancelled_job_stops_cooperatively() {
        let (plot, _) = plot_with_series();
        let plan = StreamPlan::new(plot, move |_, cancel| {
            while !cancel.is_cancelled() {
                std::thread::yield_now();
            }
            Ok(())
        });
        let mut state = StreamState::start(plan);
        state.stop();

        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            state.drain();
            if state.status() == StreamStatus::Stopped {
                break;
            }
            std::thread::yield_now();
        }

        assert_eq!(state.status(), StreamStatus::Stopped);
    }

    #[test]
    fn job_errors_leave_the_partial_plot_available() {
        let (plot, series) = plot_with_series();
        let plan = StreamPlan::new(plot, move |sink, _| {
            sink.append(series, [[0.0, 1.0]])?;
            Err(std::io::Error::other("calculation failed").into())
        });
        let mut state = StreamState::start(plan);

        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            state.drain();
            if state.status() == StreamStatus::Failed {
                break;
            }
            std::thread::yield_now();
        }

        assert_eq!(state.status(), StreamStatus::Failed);
        assert_eq!(state.error(), Some("calculation failed"));
    }
}
