//! Prototype for incrementally updating a Myplotlib plot from background work.
//!
//! The worker/session implementation is intentionally private to this example.
//! It exercises the design before streaming becomes part of Myplotlib's public
//! application API.

use eframe::egui;
use myplotlib::{Plotter, SeriesId};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};
use std::time::Duration;
use web_time::Instant;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
pub use wasm_bindgen_rayon::init_thread_pool;

const DEFAULT_SAMPLES: u64 = 100_000_000;
const MIN_SAMPLES: u64 = 100_000;
const MAX_SAMPLES: u64 = 1_000_000_000;
const TARGET_TRACE_POINTS: u64 = 5_000;
const CANCELLATION_INTERVAL: u64 = 4_096;
const MAX_BATCH_POINTS: usize = 128;
const MAX_BATCH_AGE: Duration = Duration::from_millis(33);
const RESTART_DELAY: Duration = Duration::from_millis(150);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunStatus {
    Ready,
    RestartPending,
    Running,
    Stopped,
    Cancelled,
    Finished,
    Failed,
}

impl RunStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::RestartPending => "Restart pending",
            Self::Running => "Running",
            Self::Stopped => "Stopped",
            Self::Cancelled => "Cancelled",
            Self::Finished => "Finished",
            Self::Failed => "Failed",
        }
    }
}

enum WorkerUpdate {
    Batch {
        generation: u64,
        points: Vec<[f64; 2]>,
        completed: u64,
        estimate: f64,
        elapsed: Duration,
    },
    Cancelled {
        generation: u64,
        completed: u64,
        estimate: Option<f64>,
        elapsed: Duration,
    },
    Finished {
        generation: u64,
        completed: u64,
        estimate: f64,
        elapsed: Duration,
    },
}

impl WorkerUpdate {
    fn generation(&self) -> u64 {
        match self {
            Self::Batch { generation, .. }
            | Self::Cancelled { generation, .. }
            | Self::Finished { generation, .. } => *generation,
        }
    }
}

struct ActiveRun {
    generation: u64,
    cancel: Arc<AtomicBool>,
    updates: Receiver<WorkerUpdate>,
}

struct XorShift64(u64);

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next_unit(&mut self) -> f64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;

        const SCALE: f64 = 1.0 / ((1_u64 << 53) as f64);
        ((value >> 11) as f64) * SCALE
    }
}

fn publish(
    sender: &mpsc::Sender<WorkerUpdate>,
    context: &egui::Context,
    update: WorkerUpdate,
) -> bool {
    if sender.send(update).is_err() {
        return false;
    }
    context.request_repaint();
    true
}

fn run_monte_carlo(
    generation: u64,
    target_samples: u64,
    cancel: Arc<AtomicBool>,
    sender: mpsc::Sender<WorkerUpdate>,
    context: egui::Context,
) {
    let started = Instant::now();
    let trace_interval = (target_samples / TARGET_TRACE_POINTS).max(1);
    let mut last_publish = Instant::now();
    let mut generator = XorShift64::new(0x4d59_5df4_d0f3_3173 ^ generation);
    let mut inside_circle = 0_u64;
    let mut points = Vec::with_capacity(MAX_BATCH_POINTS);
    let mut last_estimate = None;

    for completed in 1..=target_samples {
        if (completed == 1 || completed % CANCELLATION_INTERVAL == 0)
            && cancel.load(Ordering::Relaxed)
        {
            let _ = publish(
                &sender,
                &context,
                WorkerUpdate::Cancelled {
                    generation,
                    completed: completed - 1,
                    estimate: last_estimate,
                    elapsed: started.elapsed(),
                },
            );
            return;
        }

        let x = 2.0 * generator.next_unit() - 1.0;
        let y = 2.0 * generator.next_unit() - 1.0;
        inside_circle += u64::from(x * x + y * y <= 1.0);

        if completed % trace_interval == 0 || completed == target_samples {
            let estimate = 4.0 * inside_circle as f64 / completed as f64;
            last_estimate = Some(estimate);
            points.push([completed as f64, estimate]);

            if points.len() >= MAX_BATCH_POINTS || last_publish.elapsed() >= MAX_BATCH_AGE {
                let batch = std::mem::replace(&mut points, Vec::with_capacity(MAX_BATCH_POINTS));
                if !publish(
                    &sender,
                    &context,
                    WorkerUpdate::Batch {
                        generation,
                        points: batch,
                        completed,
                        estimate,
                        elapsed: started.elapsed(),
                    },
                ) {
                    return;
                }
                last_publish = Instant::now();
            }
        }
    }

    let estimate = last_estimate.unwrap_or(0.0);
    if !points.is_empty()
        && !publish(
            &sender,
            &context,
            WorkerUpdate::Batch {
                generation,
                points,
                completed: target_samples,
                estimate,
                elapsed: started.elapsed(),
            },
        )
    {
        return;
    }

    let _ = publish(
        &sender,
        &context,
        WorkerUpdate::Finished {
            generation,
            completed: target_samples,
            estimate,
            elapsed: started.elapsed(),
        },
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_compute(job: impl FnOnce() + Send + 'static) -> Result<(), String> {
    std::thread::Builder::new()
        .name("myplotlib-streaming".to_owned())
        .spawn(job)
        .map(|_| ())
        .map_err(|error| format!("could not start computation: {error}"))
}

#[cfg(target_arch = "wasm32")]
fn spawn_compute(job: impl FnOnce() + Send + 'static) -> Result<(), String> {
    rayon::spawn(job);
    Ok(())
}

struct StreamingApp {
    plotter: Plotter,
    estimate_series: SeriesId,
    target_samples: u64,
    generation: u64,
    active_run: Option<ActiveRun>,
    restart_at: Option<Instant>,
    initial_start: bool,
    status: RunStatus,
    completed: u64,
    run_target: u64,
    estimate: Option<f64>,
    elapsed: Duration,
    plotted_points: usize,
    error: Option<String>,
}

impl StreamingApp {
    fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        creation_context
            .egui_ctx
            .set_visuals(egui::Visuals::light());

        let mut plotter = Plotter::new();
        let estimate_series = plotter
            .add_points(Vec::new())
            .label("Monte Carlo estimate")
            .id();
        plotter.axhline(std::f64::consts::PI).label("π reference");
        plotter.xlabel("Samples");
        plotter.ylabel("Estimate of π");
        plotter.title("Running Monte Carlo estimate");
        plotter.xlim(0.0, DEFAULT_SAMPLES as f64);
        plotter.ylim(2.8, 3.5);

        Self {
            plotter,
            estimate_series,
            target_samples: DEFAULT_SAMPLES,
            generation: 0,
            active_run: None,
            restart_at: None,
            initial_start: true,
            status: RunStatus::Ready,
            completed: 0,
            run_target: DEFAULT_SAMPLES,
            estimate: None,
            elapsed: Duration::ZERO,
            plotted_points: 0,
            error: None,
        }
    }

    fn cancel_active(&mut self) {
        if let Some(run) = self.active_run.take() {
            run.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn stop(&mut self) {
        self.cancel_active();
        self.restart_at = None;
        self.status = RunStatus::Stopped;
    }

    fn schedule_restart(&mut self, context: &egui::Context) {
        self.cancel_active();
        self.restart_at = Some(Instant::now() + RESTART_DELAY);
        self.status = RunStatus::RestartPending;
        context.request_repaint_after(RESTART_DELAY);
    }

    fn start(&mut self, context: &egui::Context) {
        self.cancel_active();
        self.restart_at = None;
        self.generation = self.generation.wrapping_add(1);
        self.run_target = self.target_samples;
        self.completed = 0;
        self.estimate = None;
        self.elapsed = Duration::ZERO;
        self.plotted_points = 0;
        self.error = None;
        self.status = RunStatus::Running;
        self.plotter.clear_points(self.estimate_series);
        self.plotter.xlim(0.0, self.run_target as f64);

        let generation = self.generation;
        let target_samples = self.run_target;
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let worker_context = context.clone();
        let (sender, updates) = mpsc::channel();

        match spawn_compute(move || {
            run_monte_carlo(
                generation,
                target_samples,
                worker_cancel,
                sender,
                worker_context,
            );
        }) {
            Ok(()) => {
                self.active_run = Some(ActiveRun {
                    generation,
                    cancel,
                    updates,
                });
            }
            Err(error) => {
                self.status = RunStatus::Failed;
                self.error = Some(error);
            }
        }
    }

    fn apply_update(&mut self, update: WorkerUpdate) -> bool {
        if update.generation() != self.generation {
            return false;
        }

        match update {
            WorkerUpdate::Batch {
                points,
                completed,
                estimate,
                elapsed,
                ..
            } => {
                self.plotted_points += points.len();
                self.plotter.append_points(self.estimate_series, points);
                self.completed = completed;
                self.estimate = Some(estimate);
                self.elapsed = elapsed;
                false
            }
            WorkerUpdate::Cancelled {
                completed,
                estimate,
                elapsed,
                ..
            } => {
                self.completed = completed;
                self.estimate = estimate;
                self.elapsed = elapsed;
                self.status = RunStatus::Cancelled;
                true
            }
            WorkerUpdate::Finished {
                completed,
                estimate,
                elapsed,
                ..
            } => {
                self.completed = completed;
                self.estimate = Some(estimate);
                self.elapsed = elapsed;
                self.status = RunStatus::Finished;
                true
            }
        }
    }

    fn drain_updates(&mut self) {
        let Some(active_run) = self.active_run.as_ref() else {
            return;
        };

        let generation = active_run.generation;
        let mut updates = Vec::new();
        let disconnected = loop {
            match active_run.updates.try_recv() {
                Ok(update) => updates.push(update),
                Err(TryRecvError::Empty) => break false,
                Err(TryRecvError::Disconnected) => break true,
            }
        };

        let mut terminal = false;
        for update in updates {
            terminal |= self.apply_update(update);
        }

        if terminal {
            if self
                .active_run
                .as_ref()
                .is_some_and(|run| run.generation == generation)
            {
                self.active_run = None;
            }
        } else if disconnected
            && self
                .active_run
                .as_ref()
                .is_some_and(|run| run.generation == generation)
        {
            self.active_run = None;
            self.status = RunStatus::Failed;
            self.error = Some("the computation ended without a final status".to_owned());
        }
    }

    fn start_when_due(&mut self, context: &egui::Context) {
        if self.initial_start {
            self.initial_start = false;
            self.start(context);
            return;
        }

        let Some(restart_at) = self.restart_at else {
            return;
        };

        let now = Instant::now();
        if now >= restart_at {
            self.start(context);
        } else {
            context.request_repaint_after(restart_at - now);
        }
    }

    fn status_text(&self) -> String {
        let mut text = self.status.label().to_owned();
        if let Some(estimate) = self.estimate {
            text.push_str(&format!("  •  π ≈ {estimate:.9}"));
        }
        text.push_str(&format!(
            "  •  {} / {} samples  •  {:.3} s  •  {} plotted points",
            self.completed,
            self.run_target,
            self.elapsed.as_secs_f64(),
            self.plotted_points
        ));
        text
    }
}

impl Drop for StreamingApp {
    fn drop(&mut self) {
        self.cancel_active();
    }
}

impl eframe::App for StreamingApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_updates();
        self.start_when_due(context);

        egui::CentralPanel::default().show(context, |ui| {
            ui.heading("Streaming plot prototype");
            ui.label(
                "The Monte Carlo calculation runs in the background and publishes batched plot updates.",
            );
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                let slider = ui.add(
                    egui::Slider::new(
                        &mut self.target_samples,
                        MIN_SAMPLES..=MAX_SAMPLES,
                    )
                    .logarithmic(true)
                    .text("Target samples"),
                );
                if slider.changed() {
                    self.schedule_restart(context);
                }

                if self.active_run.is_some() {
                    if ui.button("Stop").clicked() {
                        self.stop();
                    }
                } else if ui.button("Start").clicked() {
                    self.start(context);
                }

                if ui.button("Restart").clicked() {
                    self.start(context);
                }
                if ui.button("Reset").clicked() {
                    self.target_samples = DEFAULT_SAMPLES;
                    self.start(context);
                }
            });

            let progress = if self.run_target == 0 {
                0.0
            } else {
                self.completed as f32 / self.run_target as f32
            };
            ui.add(
                egui::ProgressBar::new(progress.clamp(0.0, 1.0))
                    .show_percentage()
                    .animate(self.status == RunStatus::Running),
            );
            ui.label(self.status_text());
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }

            ui.separator();
            self.plotter.show_ui(ui, "streaming-monte-carlo");
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_native() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 650.0])
            .with_min_inner_size([500.0, 400.0])
            .with_resizable(true),
        ..Default::default()
    };

    eframe::run_native(
        "Myplotlib streaming prototype",
        options,
        Box::new(|creation_context| Ok(Box::new(StreamingApp::new(creation_context)))),
    )
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct StreamingWebHandle {
    runner: eframe::WebRunner,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl StreamingWebHandle {
    pub fn destroy(&self) {
        self.runner.destroy();
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(js_name = mountApp)]
pub async fn mount_app(canvas: web_sys::HtmlCanvasElement) -> Result<StreamingWebHandle, JsValue> {
    let runner = eframe::WebRunner::new();
    runner
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(|creation_context| Ok(Box::new(StreamingApp::new(creation_context)))),
        )
        .await?;

    Ok(StreamingWebHandle { runner })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_streams_batched_points_and_finishes() {
        let (sender, updates) = mpsc::channel();
        run_monte_carlo(
            7,
            100_000,
            Arc::new(AtomicBool::new(false)),
            sender,
            egui::Context::default(),
        );

        let updates: Vec<_> = updates.into_iter().collect();
        let plotted_points: usize = updates
            .iter()
            .map(|update| match update {
                WorkerUpdate::Batch { points, .. } => points.len(),
                _ => 0,
            })
            .sum();

        assert!(plotted_points > 1);
        assert!(matches!(
            updates.last(),
            Some(WorkerUpdate::Finished {
                generation: 7,
                completed: 100_000,
                estimate,
                ..
            }) if (*estimate - std::f64::consts::PI).abs() < 0.03
        ));
    }

    #[test]
    fn worker_honours_cancellation_before_starting() {
        let (sender, updates) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(true));
        run_monte_carlo(11, 100_000, cancel, sender, egui::Context::default());

        let updates: Vec<_> = updates.into_iter().collect();
        assert!(matches!(
            updates.as_slice(),
            [WorkerUpdate::Cancelled {
                generation: 11,
                completed: 0,
                estimate: None,
                ..
            }]
        ));
    }
}
