//! Run with `cargo run --example two_views`.
//!
//! Switch views with the buttons or keys 1/2; reset all parameters with R.
//! Each view keeps its own slider values when switching between them.
//! This is a native example; the WASM entry point is a compilation-only stub.

#[cfg(not(target_arch = "wasm32"))]
use myplotlib::{AppDefinition, AppResult, Plotter, Slider, SliderGrid, SliderGroup, ViewOption};
#[cfg(not(target_arch = "wasm32"))]
use std::f64::consts::{PI, TAU};
use myplotlib::AxisScale;

#[cfg(not(target_arch = "wasm32"))]
struct Params {
    amplitude: f64,
    frequency: f64,
    phase: f64,
    curvature: f64,
    center: f64,
    offset: f64,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for Params {
    fn default() -> Self {
        Self {
            amplitude: 1.0,
            frequency: 1.0,
            phase: 0.0,
            curvature: 1.0,
            center: 0.0,
            offset: 0.0,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
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
    Ok(plot)
}

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
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
    //plot.axhline(params.offset).label("Offset");
    plot.xlabel("x");
    plot.ylabel("y");
    plot.yscale(AxisScale::Log10);
    plot.xlim(-5.0, 5.0);
    Ok(plot)
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> myplotlib::NativeResult {
    const VIEWS: &[ViewOption<Params>] = &[
        ViewOption::new("Wave", wave_plot, wave_controls),
        ViewOption::new("Parabola", parabola_plot, parabola_controls),
    ];

    myplotlib::run_native(AppDefinition::new("Two plots", "plot-canvas", VIEWS))
}
