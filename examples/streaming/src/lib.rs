//! A streaming Monte Carlo plot using Myplotlib's reusable app API.

use myplotlib::{
    AppDefinition, AppResult, AxisScale, Plotter, Slider, SliderGrid, SliderGroup, StreamPlan,
    StreamPlanResult, StreamResult, ViewOption,
};
use std::time::Duration;
use web_time::Instant;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
pub use myplotlib::init_thread_pool;

const DEFAULT_SAMPLES: u64 = 100_000_000;
const MIN_SAMPLES: u64 = 100_000;
const MAX_SAMPLES: u64 = 1_000_000_000;
const TARGET_TRACE_POINTS: u64 = 5_000;
const REFERENCE_TRACE_POINTS: usize = 200;
const REFERENCE_MIN_SAMPLES: f64 = 100.0;
const MAX_BATCH_POINTS: usize = 128;
const MAX_BATCH_AGE: Duration = Duration::from_millis(33);

struct Params {
    target_samples: u64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            target_samples: DEFAULT_SAMPLES,
        }
    }
}

fn controls(params: &mut Params) -> SliderGrid<'_> {
    SliderGrid::new(
        1,
        [SliderGroup::new(
            "Calculation",
            [Slider::new(
                "Target samples",
                &mut params.target_samples,
                MIN_SAMPLES..=MAX_SAMPLES,
            )
            .logarithmic(true)],
        )],
    )
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

fn calculate_monte_carlo(
    target_samples: u64,
    mut publish: impl FnMut(Vec<[f64; 2]>, f32) -> StreamResult,
) -> StreamResult {
    let trace_interval = (target_samples / TARGET_TRACE_POINTS).max(1);
    let mut last_publish = Instant::now();
    let mut generator = XorShift64::new(0x4d59_5df4_d0f3_3173 ^ target_samples);
    let mut inside_circle = 0_u64;
    let mut points = Vec::with_capacity(MAX_BATCH_POINTS);

    for completed in 1..=target_samples {
        let x = 2.0 * generator.next_unit() - 1.0;
        let y = 2.0 * generator.next_unit() - 1.0;
        inside_circle += u64::from(x * x + y * y <= 1.0);

        if completed % trace_interval == 0 || completed == target_samples {
            let estimate = 4.0 * inside_circle as f64 / completed as f64;
            points.push([completed as f64, estimate]);

            if points.len() >= MAX_BATCH_POINTS || last_publish.elapsed() >= MAX_BATCH_AGE {
                let batch = std::mem::replace(&mut points, Vec::with_capacity(MAX_BATCH_POINTS));
                publish(batch, completed as f32 / target_samples as f32)?;
                last_publish = Instant::now();
            }
        }
    }

    if !points.is_empty() {
        publish(points, 1.0)?;
    }
    Ok(())
}

fn monte_carlo(params: &mut Params) -> StreamPlanResult {
    let target_samples = params.target_samples;
    let mut plot = Plotter::new();
    let estimate = plot
        .add_points(Vec::new())
        .label("Monte Carlo estimate")
        .id();
    plot.plot(&[0.0, target_samples as f64], &[0.0, 6.0]);
    plot.axhline(std::f64::consts::PI).label("π reference");
    plot.xlabel("Samples");
    plot.ylabel("Estimate of π");
    plot.title("Running Monte Carlo estimate");
    plot.xlim(0.0, target_samples as f64);
    plot.ylim(2.8, 3.5);

    Ok(StreamPlan::new(plot, move |stream| {
        calculate_monte_carlo(target_samples, |points, progress| {
            stream.append(estimate, points)?;
            stream.set_progress(progress)?;
            Ok(())
        })
    }))
}

fn expected_error(params: &mut Params) -> AppResult {
    let target_samples = params.target_samples as f64;
    let log_min = REFERENCE_MIN_SAMPLES.ln();
    let log_max = target_samples.ln();
    let hit_probability = std::f64::consts::PI / 4.0;
    let estimate_standard_deviation = 4.0 * (hit_probability * (1.0 - hit_probability)).sqrt();

    let mut samples = Vec::with_capacity(REFERENCE_TRACE_POINTS + 1);
    let mut upper = Vec::with_capacity(REFERENCE_TRACE_POINTS + 1);
    let mut lower = Vec::with_capacity(REFERENCE_TRACE_POINTS + 1);

    for index in 0..=REFERENCE_TRACE_POINTS {
        let fraction = index as f64 / REFERENCE_TRACE_POINTS as f64;
        let sample_count = (log_min + fraction * (log_max - log_min)).exp();
        let confidence_radius = 1.96 * estimate_standard_deviation / sample_count.sqrt();
        samples.push(sample_count);
        upper.push(std::f64::consts::PI + confidence_radius);
        lower.push(std::f64::consts::PI - confidence_radius);
    }

    let final_radius = 1.96 * estimate_standard_deviation / target_samples.sqrt();
    let mut plot = Plotter::new();
    plot.plot(&samples, &upper).label("Upper 95% limit");
    plot.plot(&samples, &lower).label("Lower 95% limit");
    plot.axhline(std::f64::consts::PI).label("π reference");
    plot.xlabel("Samples");
    plot.ylabel("Estimate of π");
    plot.title("Expected Monte Carlo uncertainty");
    plot.xscale(AxisScale::Log10);
    plot.xlim(REFERENCE_MIN_SAMPLES, target_samples);
    plot.ylim(2.8, 3.5);
    plot.add_summary("Target samples", params.target_samples);
    plot.add_summary("Final 95% radius", format!("{final_radius:.3e}"));
    Ok(plot)
}

const VIEWS: &[ViewOption<Params>] = &[
    ViewOption::streaming("Monte Carlo", monte_carlo, controls),
    ViewOption::new("Expected error", expected_error, controls),
];

fn definition() -> AppDefinition<Params> {
    AppDefinition::new("Streaming plot", VIEWS)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_native() -> myplotlib::NativeResult {
    myplotlib::run_native(definition())
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(js_name = mountApp)]
pub async fn mount_app(
    canvas: web_sys::HtmlCanvasElement,
) -> Result<myplotlib::WebHandle, JsValue> {
    myplotlib::mount_web(canvas, definition()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculation_publishes_batches_and_converges() {
        let mut points = Vec::new();
        calculate_monte_carlo(100_000, |batch, _| {
            points.extend(batch);
            Ok(())
        })
        .unwrap();

        assert!(points.len() > 1);
        let estimate = points.last().unwrap()[1];
        assert!((estimate - std::f64::consts::PI).abs() < 0.03);
    }

    #[test]
    fn calculation_stops_when_publication_stops() {
        let mut publications = 0;
        let result = calculate_monte_carlo(100_000, |_, _| {
            publications += 1;
            Err(std::io::Error::other("stop publishing").into())
        });

        assert_eq!(publications, 1);
        assert_eq!(result.unwrap_err().to_string(), "stop publishing");
    }
}
