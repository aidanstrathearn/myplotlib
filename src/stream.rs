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
pub(crate) const DEFAULT_STREAM_LABEL: &str = "Streaming";

type ParamUpdate<P> = Box<dyn FnOnce(&mut P) + Send + 'static>;
type PointBinding<P> = (SeriesId, Box<dyn Fn(&P) -> Points + 'static>);
type StreamJob<P> = Box<dyn FnOnce(StreamContext<P>) -> StreamResult + Send + 'static>;

/// The result returned by a streaming computation.
pub type StreamResult<T = ()> = Result<T, StreamError>;

/// An error returned by a streaming computation.
pub type StreamError = Box<dyn Error + Send + Sync + 'static>;

/// A formatted plot and the background computation that will update it.
pub struct StreamPlan<P> {
    plotter: Plotter,
    job: StreamJob<P>,
    point_bindings: Vec<PointBinding<P>>,
    label: String,
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
            point_bindings: Vec::new(),
            label: DEFAULT_STREAM_LABEL.to_owned(),
        }
    }

    /// Sets the heading shown in the stream control strip.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Recomputes a series on the UI thread after streamed parameter updates.
    ///
    /// The plot should contain the series' initial points before it is passed
    /// to [`Self::new`]. When the worker calls
    /// [`StreamContext::update_params`], bindings are refreshed once after the
    /// UI drains that batch of updates. The stream itself is not restarted.
    pub fn bind_points(
        mut self,
        series: SeriesId,
        points: impl Fn(&P) -> Points + 'static,
    ) -> Self {
        self.point_bindings.push((series, Box::new(points)));
        self
    }

    fn into_parts(self) -> StreamPlanParts<P> {
        StreamPlanParts {
            plotter: self.plotter,
            job: self.job,
            point_bindings: self.point_bindings,
            label: self.label,
        }
    }
}

struct StreamPlanParts<P> {
    plotter: Plotter,
    job: StreamJob<P>,
    point_bindings: Vec<PointBinding<P>>,
    label: String,
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

    /// Queues replacement of consecutive complete rows in the existing heatmap.
    ///
    /// Values are row-major, with row zero at the bottom. Send only newly
    /// calculated rows in batches; bounds, colour limits, and labels stay fixed.
    /// Initialize uncomputed cells with NaN to leave them transparent.
    ///
    /// This returns after queueing, before validation on the UI thread. A missing
    /// heatmap, incomplete rows, or an out-of-bounds range fails the stream and
    /// leaves the previously applied data available for inspection.
    pub fn set_heatmap_rows(&self, first_row: usize, values: Vec<f64>) -> StreamResult {
        self.send(StreamUpdate::HeatmapRows(first_row, values))
    }

    /// Updates a labeled plot summary on the UI thread, or adds it if absent.
    /// Uses the same bounded, cancellation-aware queue as point updates.
    pub fn set_summary(&self, label: impl Into<String>, value: impl Display) -> StreamResult {
        self.send(StreamUpdate::Summary(label.into(), value.to_string()))
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
    HeatmapRows(usize, Vec<f64>),
    Progress(f32),
    Summary(String, String),
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
    point_bindings: Vec<PointBinding<P>>,
    label: String,
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
        let StreamPlanParts {
            plotter,
            job,
            point_bindings,
            label,
        } = plan.into_parts();
        Self {
            plotter,
            job: Some(job),
            point_bindings,
            label,
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
        let mut params_updated = false;
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
                StreamUpdate::HeatmapRows(first_row, values) => {
                    if let Err(error) = self.plotter.set_heatmap_rows(first_row, &values) {
                        if let Some(cancel) = &self.cancel {
                            cancel.cancel();
                        }
                        self.elapsed = self
                            .started
                            .as_ref()
                            .map_or(Duration::ZERO, |time| time.elapsed());
                        self.status = StreamStatus::Failed;
                        self.error = Some(error.to_string());
                        finished = true;
                        break;
                    }
                }
                StreamUpdate::Progress(progress) => self.progress = Some(progress),
                StreamUpdate::Summary(label, value) => {
                    self.plotter.set_summary(label, value);
                }
                StreamUpdate::UpdateParams(update) => {
                    update(params);
                    params_updated = true;
                }
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

        if params_updated {
            for (series, points) in &self.point_bindings {
                self.plotter.replace_points(*series, points(params));
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

    pub(crate) fn label(&self) -> &str {
        &self.label
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

        assert_eq!(state.label(), DEFAULT_STREAM_LABEL);
        assert_eq!(state.status(), StreamStatus::Ready);
        assert!(!state.needs_repaint_poll());
        assert_eq!(started_receiver.try_recv(), Err(mpsc::TryRecvError::Empty));

        state.start();
        started_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
    }

    #[test]
    fn configured_stream_uses_its_label_and_waits_until_started() {
        let (plot, _) = plot_with_series();
        let (started_sender, started_receiver) = mpsc::channel();
        let plan = StreamPlan::<()>::new(plot, move |_| {
            started_sender.send(()).unwrap();
            Ok(())
        })
        .label("Custom calculation");
        let mut state = StreamState::prepare(plan);

        assert_eq!(state.label(), "Custom calculation");
        assert_eq!(state.status(), StreamStatus::Ready);
        assert_eq!(started_receiver.try_recv(), Err(mpsc::TryRecvError::Empty));

        state.start();
        assert_eq!(state.status(), StreamStatus::Running);
        assert!(state.needs_repaint_poll());
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
    fn parameter_updates_refresh_bound_points_once_per_drain() {
        use std::cell::Cell;
        use std::rc::Rc;

        #[derive(Default)]
        struct Params {
            value: usize,
        }

        let (plot, series) = plot_with_series();
        let (published_sender, published_receiver) = mpsc::channel();
        let (release_sender, release_receiver) = mpsc::channel();
        let refreshes = Rc::new(Cell::new(0));
        let observed_value = Rc::new(Cell::new(0));
        let binding_refreshes = refreshes.clone();
        let binding_value = observed_value.clone();
        let plan = StreamPlan::<Params>::new(plot, move |stream| {
            stream.update_params(|params| params.value = 1)?;
            stream.update_params(|params| params.value = 2)?;
            published_sender.send(()).unwrap();
            release_receiver.recv().unwrap();
            Ok(())
        })
        .bind_points(series, move |params| {
            binding_refreshes.set(binding_refreshes.get() + 1);
            binding_value.set(params.value);
            vec![[0.0, params.value as f64]]
        });
        let mut state = StreamState::prepare(plan);
        let mut params = Params::default();
        state.start();

        published_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        state.drain(&mut params);

        assert_eq!(params.value, 2);
        assert_eq!(refreshes.get(), 1);
        assert_eq!(observed_value.get(), 2);
        assert_eq!(state.status(), StreamStatus::Running);

        release_sender.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            state.drain(&mut params);
            if state.status() == StreamStatus::Finished {
                break;
            }
            std::thread::yield_now();
        }
        assert_eq!(state.status(), StreamStatus::Finished);
        assert_eq!(refreshes.get(), 1);
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
    fn heatmap_batches_are_applied_on_the_ui_thread_and_rendered_together() {
        use eframe::egui;

        let mut plot = Plotter::new();
        plot.heatmap(vec![f64::NAN; 6], 3, 2, [0.0, 2.0], [0.0, 3.0])
            .color_limits(0.0, 1.0);
        let ctx = egui::Context::default();
        let draw = |plot: &Plotter| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| plot.show_ui(ui, "heatmap"));
                },
            )
        };
        let initial = draw(&plot);
        let texture = initial
            .textures_delta
            .set
            .iter()
            .find(|(_, delta)| delta.image.size() == [2, 3])
            .unwrap()
            .0;

        let (published_sender, published_receiver) = mpsc::channel();
        let (release_sender, release_receiver) = mpsc::channel();
        let plan = StreamPlan::new(plot, move |stream| {
            stream.set_heatmap_rows(1, vec![0.0, 0.25, 0.75, 1.0])?;
            stream.set_heatmap_rows(2, vec![f64::NAN, 0.5])?;
            stream.set_progress(0.75)?;
            published_sender.send(()).unwrap();
            release_receiver.recv().unwrap();
            Ok(())
        });
        let mut state = start(plan);
        published_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();

        // Queueing on the worker does not mutate or upload the UI's heatmap.
        assert!(
            !draw(&state.plotter)
                .textures_delta
                .set
                .iter()
                .any(|(id, _)| *id == texture)
        );
        state.drain(&mut ());
        assert_eq!(state.progress(), Some(0.75));
        let updated = draw(&state.plotter);
        let deltas: Vec<_> = updated
            .textures_delta
            .set
            .iter()
            .filter(|(id, _)| *id == texture)
            .collect();
        assert_eq!(deltas.len(), 1);
        let egui::ImageData::Color(actual) = &deltas[0].1.image;

        let mut expected = crate::Heatmap::new(
            vec![f64::NAN, f64::NAN, 0.0, 0.25, f64::NAN, 0.5],
            3,
            2,
            [0.0, 2.0],
            [0.0, 3.0],
        );
        expected.color_limits(0.0, 1.0);
        let expected_ctx = egui::Context::default();
        let expected_texture = expected.texture(&expected_ctx);
        let expected_delta = expected_ctx.tex_manager().write().take_delta();
        let (_, delta) = expected_delta
            .set
            .iter()
            .find(|(id, _)| *id == expected_texture)
            .unwrap();
        let egui::ImageData::Color(expected_image) = &delta.image;
        assert_eq!(actual.pixels, expected_image.pixels);

        release_sender.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            state.drain(&mut ());
            if state.status() == StreamStatus::Finished {
                break;
            }
            std::thread::yield_now();
        }
        assert_eq!(state.status(), StreamStatus::Finished);
    }

    #[test]
    fn invalid_heatmap_updates_fail_the_stream_without_later_updates_overwriting_failure() {
        use crate::HeatmapUpdateError;

        for (has_heatmap, first_row, values, expected_error) in [
            (false, 0, vec![1.0, 1.0], HeatmapUpdateError::MissingHeatmap),
            (true, 0, vec![1.0], HeatmapUpdateError::IncompleteRows),
            (
                true,
                2,
                vec![1.0, 1.0],
                HeatmapUpdateError::RowRangeOutOfBounds,
            ),
        ] {
            let mut plot = Plotter::new();
            if has_heatmap {
                plot.heatmap(vec![f64::NAN; 4], 2, 2, [0.0, 2.0], [0.0, 2.0]);
            }
            let (published_sender, published_receiver) = mpsc::channel();
            let plan = StreamPlan::new(plot, move |stream| {
                stream.set_progress(0.25)?;
                stream.set_heatmap_rows(first_row, values)?;
                stream.set_progress(0.75)?;
                published_sender.send(()).unwrap();
                Ok(())
            });
            let mut state = start(plan);
            published_receiver
                .recv_timeout(Duration::from_secs(1))
                .unwrap();
            state.drain(&mut ());
            state.drain(&mut ());

            assert_eq!(state.status(), StreamStatus::Failed);
            assert_eq!(state.error(), Some(expected_error.to_string().as_str()));
            assert_eq!(state.progress(), Some(0.25));
            assert!(!state.needs_repaint_poll());
        }
    }

    #[test]
    fn heatmap_publishing_observes_cancellation() {
        let mut plot = Plotter::new();
        plot.heatmap(vec![f64::NAN; 4], 2, 2, [0.0, 2.0], [0.0, 2.0]);
        let (release_sender, release_receiver) = mpsc::channel();
        let plan = StreamPlan::new(plot, move |stream| {
            release_receiver.recv().unwrap();
            stream.set_heatmap_rows(0, vec![0.0, 1.0])
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
