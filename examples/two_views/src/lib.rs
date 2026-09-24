//! Application shared by the native and WebAssembly two-views examples.
//!
//! Switch views with the buttons, keys 1/2/3, or left/right arrows; reset with R.
//! Each view keeps its own slider values when switching between them.

use myplotlib::{
    AppDefinition, AppResult, AxisScale, Plotter, Slider, SliderGrid, SliderGroup, ViewOption,
};
use rayon::prelude::*;
use std::f64::consts::{PI, TAU};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
pub use wasm_bindgen_rayon::init_thread_pool;

const SQRT_2: f64 = std::f64::consts::SQRT_2;
const SQRT_2_PI: f64 = 2.506_628_274_631_000_2;
const GAUSSIAN_HALF_RANGE: f64 = 8.0;
const VOIGT_X_LIMIT: f64 = 8.0;
const VOIGT_PLOT_POINTS: usize = 501;

struct Params {
    amplitude: f64,
    frequency: f64,
    phase: f64,
    curvature: f64,
    center: f64,
    offset: f64,
    gaussian_sigma: f64,
    lorentzian_gamma: f64,
    averaging_samples: usize,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            amplitude: 1.0,
            frequency: 1.0,
            phase: 0.0,
            curvature: 1.0,
            center: 0.0,
            offset: 0.0,
            gaussian_sigma: 0.8,
            lorentzian_gamma: 0.35,
            averaging_samples: 4097,
        }
    }
}

fn wave_controls(params: &mut Params) -> SliderGrid<'_> {
    SliderGrid::new(
        3,
        [SliderGroup::new(
            "Wave",
            [
                Slider::new("Amplitude", &mut params.amplitude, 0.0..=3.0),
                Slider::new("Frequency", &mut params.frequency, 0.1..=5.0).logarithmic(true),
                Slider::new("Phase (rad)", &mut params.phase, -PI..=PI),
            ],
        )],
    )
}

fn wave_plot(params: &mut Params) -> AppResult {
    let x: Vec<f64> = (0..=400).map(|i| TAU * f64::from(i) / 400.0).collect();
    let sine: Vec<f64> = x
        .iter()
        .map(|x| params.amplitude * (params.frequency * x + params.phase).sin())
        .collect();
    let cosine: Vec<f64> = x
        .iter()
        .map(|x| params.amplitude * (params.frequency * x + params.phase).cos())
        .collect();

    let mut plot = Plotter::new();
    plot.plot(&x, &sine).label("Sine");
    plot.plot(&x, &cosine).label("Cosine");
    plot.axhline(0.0).label("Zero");
    plot.xlabel("x (rad)");
    plot.ylabel("Amplitude");
    plot.xlim(0.0, TAU);
    plot.add_summary("Period", format!("{:.4}", TAU / params.frequency));
    plot.add_summary("Peak amplitude", params.amplitude);
    Ok(plot)
}

fn parabola_controls(params: &mut Params) -> SliderGrid<'_> {
    SliderGrid::new(
        2,
        [
            SliderGroup::new(
                "Shape",
                [Slider::new("Curvature", &mut params.curvature, -2.0..=2.0).step_by(0.1)],
            ),
            SliderGroup::new(
                "Position",
                [
                    Slider::new("Center", &mut params.center, -3.0..=3.0),
                    Slider::new("Offset", &mut params.offset, -5.0..=5.0),
                ],
            ),
        ],
    )
}

fn parabola_plot(params: &mut Params) -> AppResult {
    let points = (0..=200)
        .map(|i| {
            let x = -5.0 + f64::from(i) / 20.0;
            let y = params.curvature * (x - params.center).powi(2) + params.offset;
            [x, y]
        })
        .collect();

    let mut plot = Plotter::new();
    plot.add_points(points).label("Parabola");
    plot.axvline(params.center).label("Center");
    plot.xlabel("x");
    plot.ylabel("y");
    plot.yscale(AxisScale::Log10);
    plot.xlim(-5.0, 5.0);
    plot.add_summary("Vertex x", params.center);
    plot.add_summary("Vertex y", params.offset);
    Ok(plot)
}

fn voigt_controls(params: &mut Params) -> SliderGrid<'_> {
    SliderGrid::new(
        3,
        [
            SliderGroup::new(
                "Profile widths",
                [
                    Slider::new("Gaussian σ", &mut params.gaussian_sigma, 0.1..=2.0)
                        .logarithmic(true),
                    Slider::new("Lorentzian γ", &mut params.lorentzian_gamma, 0.02..=1.5)
                        .logarithmic(true),
                ],
            ),
            SliderGroup::new(
                "Numerical average",
                [Slider::new(
                    "Gaussian samples",
                    &mut params.averaging_samples,
                    257..=16_385 * 4,
                )
                .logarithmic(true)],
            ),
        ],
    )
}

fn lorentzian(x: f64, gamma: f64) -> f64 {
    gamma / (PI * (x * x + gamma * gamma))
}

fn gaussian_kernel(sigma: f64, sample_count: usize) -> Vec<(f64, f64)> {
    let sample_count = sample_count.max(1);
    let spacing = 2.0 * GAUSSIAN_HALF_RANGE * sigma / sample_count as f64;
    let mut kernel: Vec<_> = (0..sample_count)
        .map(|index| {
            let standard_deviations = -GAUSSIAN_HALF_RANGE
                + (index as f64 + 0.5) * 2.0 * GAUSSIAN_HALF_RANGE / sample_count as f64;
            let shift = standard_deviations * sigma;
            let weight = (-0.5 * standard_deviations * standard_deviations).exp() * spacing;
            (shift, weight)
        })
        .collect();
    let total_weight: f64 = kernel.iter().map(|(_, weight)| weight).sum();
    kernel
        .iter_mut()
        .for_each(|(_, weight)| *weight /= total_weight);
    kernel
}

fn parallel_gaussian_average(
    x_values: &[f64],
    sigma: f64,
    gamma: f64,
    sample_count: usize,
) -> Vec<f64> {
    let kernel = gaussian_kernel(sigma, sample_count);
    x_values
        .par_iter()
        .map(|&x| {
            kernel
                .iter()
                .map(|&(shift, weight)| weight * lorentzian(x - shift, gamma))
                .sum()
        })
        .collect()
}

fn analytic_voigt(x: f64, sigma: f64, gamma: f64) -> f64 {
    let z_scale = sigma * SQRT_2;
    let (real, _) = puruspe::faddeeva(x / z_scale, gamma / z_scale);
    real / (sigma * SQRT_2_PI)
}

fn voigt_plot(params: &mut Params) -> AppResult {
    let x: Vec<_> = (0..VOIGT_PLOT_POINTS)
        .map(|index| {
            -VOIGT_X_LIMIT + 2.0 * VOIGT_X_LIMIT * index as f64 / (VOIGT_PLOT_POINTS - 1) as f64
        })
        .collect();
    let numerical = parallel_gaussian_average(
        &x,
        params.gaussian_sigma,
        params.lorentzian_gamma,
        params.averaging_samples,
    );
    let analytic: Vec<_> = x
        .iter()
        .map(|&x| analytic_voigt(x, params.gaussian_sigma, params.lorentzian_gamma))
        .collect();
    let max_error = numerical
        .iter()
        .zip(&analytic)
        .map(|(numerical, analytic)| (numerical - analytic).abs())
        .fold(0.0_f64, f64::max);

    let mut plot = Plotter::new();
    plot.plot(&x, &numerical).label("Parallel Gaussian average");
    plot.plot(&x, &analytic).label("Analytic Voigt");
    plot.xlabel("x");
    plot.ylabel("Normalized profile");
    plot.xlim(-VOIGT_X_LIMIT, VOIGT_X_LIMIT);
    plot.add_summary("Max |numerical - analytic|", format!("{max_error:.3e}"));
    plot.add_summary("Gaussian samples", params.averaging_samples);
    plot.add_summary("Rayon workers", rayon::current_num_threads());
    Ok(plot)
}

const VIEWS: &[ViewOption<Params>] = &[
    ViewOption::new("Wave", wave_plot, wave_controls),
    ViewOption::new("Parabola", parabola_plot, parabola_controls),
    ViewOption::new("Voigt average", voigt_plot, voigt_controls),
];

fn definition() -> AppDefinition<Params> {
    AppDefinition::new("Two plots", VIEWS)
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
    fn parallel_gaussian_average_matches_analytic_voigt() {
        let x = [-4.0, -1.0, 0.0, 0.75, 3.0];
        let sigma = 0.8;
        let gamma = 0.35;
        let numerical = parallel_gaussian_average(&x, sigma, gamma, 4097);

        for (&x, numerical) in x.iter().zip(numerical) {
            let analytic = analytic_voigt(x, sigma, gamma);
            assert!(
                (numerical - analytic).abs() < 1e-10,
                "at x={x}: numerical={numerical}, analytic={analytic}"
            );
        }
    }
}
