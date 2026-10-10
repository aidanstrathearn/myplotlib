//! Append, replace, and immediate plots using Myplotlib's reusable app API.

use myplotlib::{
    AppDefinition, AppResult, AxisScale, Plotter, SeriesId, Slider, SliderGrid, SliderGroup,
    StreamContext, StreamPlan, StreamPlanResult, StreamResult, ViewOption,
};
use std::f64::consts::PI;
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
const DEFAULT_VOIGT_STEPS: usize = 32_768;
const MIN_VOIGT_STEPS: usize = 512;
const MAX_VOIGT_STEPS: usize = 1_000_000_000;
const VOIGT_PLOT_POINTS: usize = 501;
const VOIGT_X_LIMIT: f64 = 8.0;
const SQRT_2_PI: f64 = 2.506_628_274_631_000_2;
const MIN_GAUSSIAN_SIGMA: f64 = 0.1;
const MAX_GAUSSIAN_SIGMA: f64 = 2.0;
const MIN_LORENTZIAN_GAMMA: f64 = 0.02;
const MAX_LORENTZIAN_GAMMA: f64 = 1.5;
const MIN_FIT_GAMMA: f64 = 0.0;
const FIT_SAMPLE_STRIDE: usize = 4;
const FIT_ITERATIONS: usize = 16;

struct Params {
    target_samples: u64,
    gaussian_sigma: f64,
    lorentzian_gamma: f64,
    fit_sigma: f64,
    fit_gamma: f64,
    voigt_steps: usize,
}

type PlanResult = StreamPlanResult<Params>;
type Context = StreamContext<Params>;

impl Default for Params {
    fn default() -> Self {
        Self {
            target_samples: DEFAULT_SAMPLES,
            gaussian_sigma: 0.8,
            lorentzian_gamma: 0.35,
            fit_sigma: 1.2,
            fit_gamma: 0.7,
            voigt_steps: DEFAULT_VOIGT_STEPS,
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

fn next_estimate(generator: &mut XorShift64, inside_circle: &mut u64, completed: u64) -> f64 {
    let x = 2.0 * generator.next_unit() - 1.0;
    let y = 2.0 * generator.next_unit() - 1.0;
    *inside_circle += u64::from(x * x + y * y <= 1.0);
    4.0 * *inside_circle as f64 / completed as f64
}

fn calculate_monte_carlo(target_samples: u64, series: SeriesId, stream: Context) -> StreamResult {
    let trace_interval = (target_samples / TARGET_TRACE_POINTS).max(1);
    let mut last_publish = Instant::now();
    let mut generator = XorShift64::new(0x4d59_5df4_d0f3_3173 ^ target_samples);
    let mut inside_circle = 0_u64;
    let mut points = Vec::with_capacity(MAX_BATCH_POINTS);

    for completed in 1..=target_samples {
        let estimate = next_estimate(&mut generator, &mut inside_circle, completed);

        if completed % trace_interval == 0 || completed == target_samples {
            points.push([completed as f64, estimate]);

            if points.len() >= MAX_BATCH_POINTS || last_publish.elapsed() >= MAX_BATCH_AGE {
                let batch = std::mem::replace(&mut points, Vec::with_capacity(MAX_BATCH_POINTS));
                stream.append(series, batch)?;
                stream.set_progress(completed as f32 / target_samples as f32)?;
                last_publish = Instant::now();
            }
        }
    }

    if !points.is_empty() {
        stream.append(series, points)?;
        stream.set_progress(1.0)?;
    }
    Ok(())
}

fn monte_carlo(params: &mut Params) -> PlanResult {
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
        calculate_monte_carlo(target_samples, estimate, stream)
    }))
}

fn voigt_controls(params: &mut Params) -> SliderGrid<'_> {
    SliderGrid::new(
        3,
        [
            SliderGroup::new(
                "Profile widths",
                [
                    Slider::new(
                        "Gaussian σ",
                        &mut params.gaussian_sigma,
                        MIN_GAUSSIAN_SIGMA..=MAX_GAUSSIAN_SIGMA,
                    )
                    .logarithmic(true),
                    Slider::new(
                        "Lorentzian γ",
                        &mut params.lorentzian_gamma,
                        MIN_LORENTZIAN_GAMMA..=MAX_LORENTZIAN_GAMMA,
                    )
                    .logarithmic(true),
                ],
            ),
            SliderGroup::new(
                "Fitted Voigt",
                [
                    Slider::new(
                        "Fitted Gaussian σ",
                        &mut params.fit_sigma,
                        MIN_GAUSSIAN_SIGMA..=MAX_GAUSSIAN_SIGMA,
                    ),
                    Slider::new(
                        "Fitted Lorentzian γ",
                        &mut params.fit_gamma,
                        MIN_FIT_GAMMA..=MAX_LORENTZIAN_GAMMA,
                    ),
                ],
            ),
            SliderGroup::new(
                "Numerical convolution",
                [Slider::new(
                    "Integration steps",
                    &mut params.voigt_steps,
                    MIN_VOIGT_STEPS..=MAX_VOIGT_STEPS,
                )
                .logarithmic(true)],
            ),
        ],
    )
}

fn gaussian(x: f64, sigma: f64) -> f64 {
    (-0.5 * (x / sigma).powi(2)).exp() / (sigma * SQRT_2_PI)
}

fn analytic_voigt(x: f64, sigma: f64, gamma: f64) -> f64 {
    let z_scale = sigma * std::f64::consts::SQRT_2;
    let (real, _) = puruspe::faddeeva(x / z_scale, gamma / z_scale);
    real / (sigma * SQRT_2_PI)
}

fn add_shift_pair(accumulated: &mut [f64], x_values: &[f64], sigma: f64, shift: f64) {
    for (sum, &x) in accumulated.iter_mut().zip(x_values) {
        *sum += gaussian(x - shift, sigma) + gaussian(x + shift, sigma);
    }
}

fn profile_points(x_values: &[f64], accumulated: &[f64], samples: usize) -> Vec<[f64; 2]> {
    let normalisation = 1.0 / samples as f64;
    x_values
        .iter()
        .zip(accumulated)
        .map(|(&x, &sum)| [x, sum * normalisation])
        .collect()
}

fn voigt_points(x_values: &[f64], sigma: f64, gamma: f64) -> Vec<[f64; 2]> {
    x_values
        .iter()
        .map(|&x| [x, analytic_voigt(x, sigma, gamma)])
        .collect()
}

fn voigt_fit_error(
    x_values: &[f64],
    accumulated: &[f64],
    samples: usize,
    sigma: f64,
    gamma: f64,
) -> f64 {
    let normalisation = 1.0 / samples as f64;
    x_values
        .iter()
        .zip(accumulated)
        .step_by(FIT_SAMPLE_STRIDE)
        .map(|(&x, &sum)| {
            let residual = analytic_voigt(x, sigma, gamma) - sum * normalisation;
            residual * residual
        })
        .sum()
}

fn fit_voigt(
    x_values: &[f64],
    accumulated: &[f64],
    samples: usize,
    initial_sigma: f64,
    initial_gamma: f64,
) -> (f64, f64) {
    let mut sigma = initial_sigma.clamp(MIN_GAUSSIAN_SIGMA, MAX_GAUSSIAN_SIGMA);
    let mut gamma = initial_gamma.clamp(MIN_FIT_GAMMA, MAX_LORENTZIAN_GAMMA);
    let mut sigma_step = (MAX_GAUSSIAN_SIGMA - MIN_GAUSSIAN_SIGMA) / 8.0;
    let mut gamma_step = (MAX_LORENTZIAN_GAMMA - MIN_FIT_GAMMA) / 8.0;
    let mut best_error = voigt_fit_error(x_values, accumulated, samples, sigma, gamma);

    for _ in 0..FIT_ITERATIONS {
        let mut next = (sigma, gamma, best_error);
        for sigma_direction in [-1.0, 0.0, 1.0] {
            for gamma_direction in [-1.0, 0.0, 1.0] {
                if sigma_direction == 0.0 && gamma_direction == 0.0 {
                    continue;
                }
                let candidate_sigma = (sigma + sigma_direction * sigma_step)
                    .clamp(MIN_GAUSSIAN_SIGMA, MAX_GAUSSIAN_SIGMA);
                let candidate_gamma = (gamma + gamma_direction * gamma_step)
                    .clamp(MIN_FIT_GAMMA, MAX_LORENTZIAN_GAMMA);
                let error = voigt_fit_error(
                    x_values,
                    accumulated,
                    samples,
                    candidate_sigma,
                    candidate_gamma,
                );
                if error < next.2 {
                    next = (candidate_sigma, candidate_gamma, error);
                }
            }
        }

        if next.2 < best_error {
            (sigma, gamma, best_error) = next;
        } else {
            sigma_step *= 0.5;
            gamma_step *= 0.5;
        }
    }

    (sigma, gamma)
}

struct VoigtCalculation {
    x_values: Vec<f64>,
    sigma: f64,
    gamma: f64,
    steps: usize,
    partial_series: SeriesId,
    fitted_series: SeriesId,
    fit_sigma: f64,
    fit_gamma: f64,
}

fn calculate_voigt(calculation: VoigtCalculation, stream: Context) -> StreamResult {
    let VoigtCalculation {
        x_values,
        sigma,
        gamma,
        steps,
        partial_series,
        fitted_series,
        mut fit_sigma,
        mut fit_gamma,
    } = calculation;
    let sample_count = 2 * steps + 1;
    let mut accumulated: Vec<_> = x_values.iter().map(|&x| gaussian(x, sigma)).collect();
    let mut last_publish = Instant::now();

    for step in 1..=steps {
        // Integrate symmetric Lorentzian quantiles from the centre outwards.
        // The normalized partial sum therefore starts Gaussian and broadens
        // smoothly into the complete Voigt convolution.
        let quantile = 0.5 + step as f64 / sample_count as f64;
        let shift = gamma * (PI * (quantile - 0.5)).tan();
        add_shift_pair(&mut accumulated, &x_values, sigma, shift);

        if last_publish.elapsed() >= MAX_BATCH_AGE || step == steps {
            let partial_samples = 2 * step + 1;
            (fit_sigma, fit_gamma) = fit_voigt(
                &x_values,
                &accumulated,
                partial_samples,
                fit_sigma,
                fit_gamma,
            );
            stream.replace(
                partial_series,
                profile_points(&x_values, &accumulated, partial_samples),
            )?;
            stream.replace(fitted_series, voigt_points(&x_values, fit_sigma, fit_gamma))?;
            stream.update_params(move |params: &mut Params| {
                params.fit_sigma = fit_sigma;
                params.fit_gamma = fit_gamma;
            })?;
            stream.set_progress(step as f32 / steps as f32)?;
            last_publish = Instant::now();
        }
    }

    Ok(())
}

fn voigt(params: &mut Params) -> PlanResult {
    let sigma = params.gaussian_sigma;
    let gamma = params.lorentzian_gamma;
    let fit_sigma = params.fit_sigma;
    let fit_gamma = params.fit_gamma;
    let steps = params.voigt_steps;
    let x_values: Vec<_> = (0..VOIGT_PLOT_POINTS)
        .map(|index| {
            -VOIGT_X_LIMIT + 2.0 * VOIGT_X_LIMIT * index as f64 / (VOIGT_PLOT_POINTS - 1) as f64
        })
        .collect();
    let initial: Vec<_> = x_values.iter().map(|&x| [x, gaussian(x, sigma)]).collect();
    let fitted = voigt_points(&x_values, fit_sigma, fit_gamma);
    let target = voigt_points(&x_values, sigma, gamma);

    let mut plot = Plotter::new();
    let partial = plot.add_points(initial).label("Partial convolution").id();
    let fitted = plot.add_points(fitted).label("Fitted Voigt").id();
    plot.add_points(target).label("Analytic Voigt target");
    plot.xlabel("x");
    plot.ylabel("Normalized profile");
    plot.title("Gaussian evolving into a Voigt profile");
    plot.xlim(-VOIGT_X_LIMIT, VOIGT_X_LIMIT);
    let initial_peak = gaussian(0.0, sigma).max(analytic_voigt(0.0, fit_sigma, fit_gamma));
    plot.ylim(0.0, 1.1 * initial_peak);
    plot.add_summary("Integration samples", 2 * steps + 1);

    Ok(StreamPlan::new(plot, move |stream| {
        calculate_voigt(
            VoigtCalculation {
                x_values,
                sigma,
                gamma,
                steps,
                partial_series: partial,
                fitted_series: fitted,
                fit_sigma,
                fit_gamma,
            },
            stream,
        )
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
    ViewOption::streaming("Voigt replace", voigt, voigt_controls),
    ViewOption::new("Expected error", expected_error, controls),
];

fn definition() -> AppDefinition<Params> {
    AppDefinition::new("Streaming plots", VIEWS)
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
    fn calculation_converges() {
        let target_samples = 100_000;
        let mut generator = XorShift64::new(0x4d59_5df4_d0f3_3173 ^ target_samples);
        let mut inside_circle = 0;
        let mut estimate = 0.0;
        for completed in 1..=target_samples {
            estimate = next_estimate(&mut generator, &mut inside_circle, completed);
        }

        assert!((estimate - std::f64::consts::PI).abs() < 0.03);
    }

    #[test]
    fn lorentzian_average_converges_to_voigt_profile() {
        let x_values = [-4.0, -1.0, 0.0, 0.75, 3.0];
        let sigma = 0.8;
        let gamma = 0.35;
        let steps = 4096;
        let sample_count = 2 * steps + 1;
        let mut accumulated: Vec<_> = x_values.iter().map(|&x| gaussian(x, sigma)).collect();

        for step in 1..=steps {
            let quantile = 0.5 + step as f64 / sample_count as f64;
            let shift = gamma * (PI * (quantile - 0.5)).tan();
            add_shift_pair(&mut accumulated, &x_values, sigma, shift);
        }

        for (&x, sum) in x_values.iter().zip(accumulated) {
            let numerical = sum / sample_count as f64;
            let analytic = analytic_voigt(x, sigma, gamma);
            assert!(
                (numerical - analytic).abs() < 1e-8,
                "at x={x}: numerical={numerical}, analytic={analytic}"
            );
        }
    }

    #[test]
    fn fitted_voigt_recovers_profile_widths() {
        let sigma = 0.8;
        let gamma = 0.35;
        let x_values: Vec<_> = (0..VOIGT_PLOT_POINTS)
            .map(|index| {
                -VOIGT_X_LIMIT + 2.0 * VOIGT_X_LIMIT * index as f64 / (VOIGT_PLOT_POINTS - 1) as f64
            })
            .collect();
        let observed: Vec<_> = x_values
            .iter()
            .map(|&x| analytic_voigt(x, sigma, gamma))
            .collect();

        let (fitted_sigma, fitted_gamma) = fit_voigt(&x_values, &observed, 1, 1.2, 0.7);

        assert!((fitted_sigma - sigma).abs() < 0.002, "{fitted_sigma}");
        assert!((fitted_gamma - gamma).abs() < 0.002, "{fitted_gamma}");
    }

    #[test]
    fn fitted_widths_converge_as_the_integral_grows() {
        let sigma = 0.8;
        let gamma = 0.35;
        let steps = 1024;
        let sample_count = 2 * steps + 1;
        let x_values: Vec<_> = (0..VOIGT_PLOT_POINTS)
            .map(|index| {
                -VOIGT_X_LIMIT + 2.0 * VOIGT_X_LIMIT * index as f64 / (VOIGT_PLOT_POINTS - 1) as f64
            })
            .collect();
        let mut accumulated: Vec<_> = x_values.iter().map(|&x| gaussian(x, sigma)).collect();
        let mut fitted = (1.2, 0.7);
        let mut early = None;

        for step in 1..=steps {
            let quantile = 0.5 + step as f64 / sample_count as f64;
            let shift = gamma * (PI * (quantile - 0.5)).tan();
            add_shift_pair(&mut accumulated, &x_values, sigma, shift);

            if step == steps / 8 || step == steps {
                fitted = fit_voigt(&x_values, &accumulated, 2 * step + 1, fitted.0, fitted.1);
                early.get_or_insert(fitted);
            }
        }

        let distance = |fit: (f64, f64)| (fit.0 - sigma).abs() + (fit.1 - gamma).abs();
        assert!(distance(fitted) < distance(early.unwrap()));
        assert!((fitted.0 - sigma).abs() < 0.01, "{}", fitted.0);
        assert!((fitted.1 - gamma).abs() < 0.01, "{}", fitted.1);
    }
}
