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

type ParamUpdate<P> = Box<dyn FnOnce(&mut P) + Send + 'static>;
type StreamJob<P> = Box<dyn FnOnce(StreamContext<P>) -> StreamResult + Send + 'static>;

/// The result returned by a streaming computation.
pub type StreamResult<T = ()> = Result<T, StreamError>;

/// An error returned by a streaming computation.
pub type StreamError = Box<dyn Error + Send + Sync + 'static>;

/// A formatted plot and the background computation that will update it.
pub struct StreamPlan<P> {
    plotter: Plotter,
    job: StreamJob<P>,
}

impl<P: 'static> StreamPlan<P> {
    /// Creates a stream whose plot stays on the UI thread while `job` runs in
    /// the background.
    pub fn new(
        plotter: Plotter,
        job: impl FnOnce(StreamContext<P>) -> StreamResult + Send + 'static,
    ) -> Self {
        Self {
            plotter,
            job: Box::new(job),
        }
    }

    pub(crate) fn into_parts(self) -> (Plotter, StreamJob<P>) {
        (self.plotter, self.job)
    }
}

#[derive(Clone, Debug)]
struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Returned internally when the app has stopped a streaming computation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StreamStopped;

impl Display for StreamStopped {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("the plot stream was stopped")
    }
}

impl Error for StreamStopped {}

/// Updates a plot from its background computation.
///
/// Send points in batches rather than one update per point. The queue is
/// bounded, so producers wait if they outpace the UI instead of growing memory
/// without limit. Every update also checks whether the app has stopped the
/// computation, so a regularly publishing job needs no separate cancellation
/// handling. Use [`Self::checkpoint`] during long stretches that do not publish.
//
// Workers deliberately never retain an egui Context. On threaded Wasm, a
// worker-side request_repaint can contend with the UI's parking_lot locks and
// panic because parking is unsupported. App polls this queue and schedules the
// next repaint from the UI thread instead.
pub struct StreamContext<P> {
    updates: SyncSender<StreamUpdate<P>>,
    cancel: CancelFlag,
}

impl<P> Clone for StreamContext<P> {
    fn clone(&self) -> Self {
        Self {
            updates: self.updates.clone(),
            cancel: self.cancel.clone(),
        }
    }
}

impl<P: 'static> StreamContext<P> {
    /// Appends a batch of points to a series.
    pub fn append(
        &self,
        series: SeriesId,
        points: impl IntoIterator<Item = [f64; 2]>,
    ) -> StreamResult {
        self.checkpoint()?;
        self.send(StreamUpdate::Append(series, points.into_iter().collect()))
    }

    /// Replaces all points in a series.
    pub fn replace(&self, series: SeriesId, points: Points) -> StreamResult {
        self.send(StreamUpdate::Replace(series, points))
    }

    /// Removes all points from a series.
    pub fn clear(&self, series: SeriesId) -> StreamResult {
        self.send(StreamUpdate::Clear(series))
    }

    /// Reports completion in the range `0.0..=1.0`.
    pub fn set_progress(&self, progress: f32) -> StreamResult {
        self.send(StreamUpdate::Progress(progress.clamp(0.0, 1.0)))
    }

    /// Applies a mutation to the application parameters on the UI thread.
    ///
    /// Group related values into one update and publish at display cadence.
    /// Programmatic changes are visible to controls on the next frame without
    /// being treated as user input.
    ///
    /// The closure parameter can name the application's parameter type when it
    /// is needed for type inference:
    ///
    /// ```ignore
    /// stream.update_params(|params: &mut Params| {
    ///     params.latest_value = value;
    /// })?;
    /// ```
    pub fn update_params(&self, update: impl FnOnce(&mut P) + Send + 'static) -> StreamResult {
        self.send(StreamUpdate::UpdateParams(Box::new(update)))
    }

    /// Stops the job promptly if the app has cancelled it.
    ///
    /// Update methods perform this check automatically. Call `checkpoint`
    /// between bounded pieces of work only when the job can run for a long time
    /// without publishing an update.
    pub fn checkpoint(&self) -> StreamResult {
        if self.cancel.is_cancelled() {
            Err(StreamStopped.into())
        } else {
            Ok(())
        }
    }

    fn send(&self, update: StreamUpdate<P>) -> StreamResult {
        self.checkpoint()?;
        self.updates
            .send(update)
            .map_err(|_| StreamError::from(StreamStopped))
    }
}

enum StreamUpdate<P> {
    Append(SeriesId, Points),
    Replace(SeriesId, Points),
    Clear(SeriesId),
    Progress(f32),
    UpdateParams(ParamUpdate<P>),
    Finished(JobOutcome, Duration),
}

enum JobOutcome {
    Completed,
    Stopped,
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StreamStatus {
    Ready,
    Running,
    Stopping,
    Stopped,
    Finished,
    Failed,
}

impl StreamStatus {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Running => "Running",
            Self::Stopping => "Stopping",
            Self::Stopped => "Stopped",
            Self::Finished => "Finished",
            Self::Failed => "Failed",
        }
    }
}

pub(crate) struct StreamState<P> {
    pub(crate) plotter: Plotter,
    job: Option<StreamJob<P>>,
    updates: Option<Receiver<StreamUpdate<P>>>,
    cancel: Option<CancelFlag>,
    status: StreamStatus,
    progress: Option<f32>,
    started: Option<Instant>,
    elapsed: Duration,
    error: Option<String>,
}

impl<P: 'static> StreamState<P> {
    pub(crate) fn prepare(plan: StreamPlan<P>) -> Self {
        let (plotter, job) = plan.into_parts();
        Self {
            plotter,
            job: Some(job),
            updates: None,
            cancel: None,
            status: StreamStatus::Ready,
            progress: None,
            started: None,
            elapsed: Duration::ZERO,
            error: None,
        }
    }

    pub(crate) fn start(&mut self) {
        let Some(job) = self.job.take() else {
            return;
        };
        let (sender, receiver) = mpsc::sync_channel(UPDATE_QUEUE_CAPACITY);
        let cancel = CancelFlag::new();
        let worker_cancel = cancel.clone();
        let worker_sender = sender.clone();
        let context = StreamContext {
            updates: sender,
            cancel: worker_cancel,
        };
        self.updates = Some(receiver);
        self.cancel = Some(cancel);
        self.status = StreamStatus::Running;
        self.progress = None;
        self.started = Some(Instant::now());
        self.elapsed = Duration::ZERO;
        self.error = None;

        let result = spawn_compute(move || {
            let started = Instant::now();
            let outcome = match job(context) {
                Ok(()) => JobOutcome::Completed,
                Err(error) if error.is::<StreamStopped>() => JobOutcome::Stopped,
                Err(error) => JobOutcome::Failed(error.to_string()),
            };
            let _ = worker_sender.send(StreamUpdate::Finished(outcome, started.elapsed()));
        });
        if let Err(error) = result {
            self.updates = None;
            self.cancel = None;
            self.started = None;
            self.status = StreamStatus::Failed;
            self.error = Some(error);
        }
    }

    pub(crate) fn drain(&mut self, params: &mut P) {
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
                StreamUpdate::UpdateParams(update) => update(params),
                StreamUpdate::Finished(outcome, elapsed) => {
                    finished = true;
                    self.elapsed = elapsed;
                    match outcome {
                        JobOutcome::Completed
                            if self.cancel.as_ref().is_some_and(CancelFlag::is_cancelled) =>
                        {
                            self.status = StreamStatus::Stopped;
                        }
                        JobOutcome::Completed => {
                            self.status = StreamStatus::Finished;
                            self.progress = Some(1.0);
                        }
                        JobOutcome::Stopped => self.status = StreamStatus::Stopped,
                        JobOutcome::Failed(error) => {
                            self.status = StreamStatus::Failed;
                            self.error = Some(error);
                        }
                    }
                }
            }
        }

        if finished {
            self.updates = None;
            self.cancel = None;
            self.started = None;
        } else if disconnected {
            self.updates = None;
            self.cancel = None;
            self.status = StreamStatus::Failed;
            self.error = Some("the computation ended without a final status".to_owned());
            self.elapsed = self
                .started
                .take()
                .map_or(Duration::ZERO, |started| started.elapsed());
        }
    }

    pub(crate) fn stop(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
            self.status = StreamStatus::Stopping;
        }
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

    pub(crate) fn needs_repaint_poll(&self) -> bool {
        self.updates.is_some()
    }
}

impl<P> Drop for StreamState<P> {
    fn drop(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
        }
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

    fn start(plan: StreamPlan<()>) -> StreamState<()> {
        let mut state = StreamState::prepare(plan);
        state.start();
        state
    }

    #[test]
    fn prepared_stream_does_not_run_until_started() {
        let (plot, _) = plot_with_series();
        let (started_sender, started_receiver) = mpsc::channel();
        let plan = StreamPlan::<()>::new(plot, move |_| {
            started_sender.send(()).unwrap();
            Ok(())
        });
        let mut state = StreamState::prepare(plan);

        assert_eq!(state.status(), StreamStatus::Ready);
        assert!(!state.needs_repaint_poll());
        assert_eq!(started_receiver.try_recv(), Err(mpsc::TryRecvError::Empty));

        state.start();
        started_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
    }

    #[test]
    fn parameter_updates_are_applied_only_when_the_ui_drains_them() {
        #[derive(Default, Debug, PartialEq, Eq)]
        struct Params {
            _ui_only: std::rc::Rc<()>,
            value: usize,
            updates: usize,
        }

        let (plot, _) = plot_with_series();
        let (published_sender, published_receiver) = mpsc::channel();
        let plan = StreamPlan::<Params>::new(plot, move |stream| {
            stream.update_params(|params| {
                params.value = 42;
                params.updates += 1;
            })?;
            published_sender.send(()).unwrap();
            Ok(())
        });
        let mut state = StreamState::prepare(plan);
        let mut params = Params::default();
        state.start();

        published_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        assert_eq!(params, Params::default());

        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            state.drain(&mut params);
            if state.status() == StreamStatus::Finished {
                break;
            }
            std::thread::yield_now();
        }

        assert_eq!(
            params,
            Params {
                value: 42,
                updates: 1,
                ..Params::default()
            }
        );
        assert_eq!(state.status(), StreamStatus::Finished);
    }

    #[test]
    fn stream_applies_all_update_kinds_and_finishes() {
        let (plot, series) = plot_with_series();
        let plan = StreamPlan::new(plot, move |stream| {
            stream.append(series, [[0.0, 1.0], [1.0, 2.0]])?;
            stream.replace(series, vec![[2.0, 3.0]])?;
            stream.clear(series)?;
            stream.append(series, [[3.0, 4.0]])?;
            stream.set_progress(0.75)?;
            Ok(())
        });
        let mut state = start(plan);

        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            state.drain(&mut ());
            if state.status() == StreamStatus::Finished {
                break;
            }
            std::thread::yield_now();
        }

        assert_eq!(state.status(), StreamStatus::Finished);
        assert_eq!(state.progress(), Some(1.0));
    }

    #[test]
    fn cancelled_job_stops_at_checkpoint() {
        let (plot, _) = plot_with_series();
        let plan = StreamPlan::new(plot, move |stream| {
            loop {
                stream.checkpoint()?;
                std::thread::yield_now();
            }
        });
        let mut state = start(plan);
        state.stop();

        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            state.drain(&mut ());
            if state.status() == StreamStatus::Stopped {
                break;
            }
            std::thread::yield_now();
        }

        assert_eq!(state.status(), StreamStatus::Stopped);
        assert_eq!(state.error(), None);
    }

    #[test]
    fn publishing_observes_cancellation_automatically() {
        let (plot, series) = plot_with_series();
        let (release_sender, release_receiver) = mpsc::channel();
        let plan = StreamPlan::new(plot, move |stream| {
            release_receiver.recv().unwrap();
            stream.append(
                series,
                std::iter::once_with(|| panic!("cancelled append consumed its points")),
            )
        });
        let mut state = start(plan);
        state.stop();
        release_sender.send(()).unwrap();

        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            state.drain(&mut ());
            if state.status() == StreamStatus::Stopped {
                break;
            }
            std::thread::yield_now();
        }

        assert_eq!(state.status(), StreamStatus::Stopped);
        assert_eq!(state.error(), None);
    }

    #[test]
    fn cancellation_does_not_hide_a_job_failure() {
        let (plot, _) = plot_with_series();
        let (release_sender, release_receiver) = mpsc::channel();
        let plan = StreamPlan::new(plot, move |_| {
            release_receiver.recv().unwrap();
            Err(std::io::Error::other("calculation failed").into())
        });
        let mut state = start(plan);
        state.stop();
        release_sender.send(()).unwrap();

        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            state.drain(&mut ());
            if state.status() == StreamStatus::Failed {
                break;
            }
            std::thread::yield_now();
        }

        assert_eq!(state.status(), StreamStatus::Failed);
        assert_eq!(state.error(), Some("calculation failed"));
    }

    #[test]
    fn job_errors_leave_the_partial_plot_available() {
        let (plot, series) = plot_with_series();
        let plan = StreamPlan::new(plot, move |stream| {
            stream.append(series, [[0.0, 1.0]])?;
            Err(std::io::Error::other("calculation failed").into())
        });
        let mut state = start(plan);

        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            state.drain(&mut ());
            if state.status() == StreamStatus::Failed {
                break;
            }
            std::thread::yield_now();
        }

        assert_eq!(state.status(), StreamStatus::Failed);
        assert_eq!(state.error(), Some("calculation failed"));
    }
}
