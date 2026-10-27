//! Run with `cargo run --example heatmap_app`; change width with the slider.
use myplotlib::{AppDefinition, AppResult, Plotter, Slider, SliderGrid, SliderGroup, ViewOption};

struct Params {
    width: f64,
}
impl Default for Params {
    fn default() -> Self {
        Self { width: 1.0 }
    }
}
fn controls(params: &mut Params) -> SliderGrid<'_> {
    SliderGrid::new(
        1,
        [SliderGroup::new(
            "Gaussian",
            [Slider::new("Width", &mut params.width, 0.2..=2.0)],
        )],
    )
}
fn plot(params: &mut Params) -> AppResult {
    let width = params.width;
    let values = (0..80)
        .flat_map(|row| {
            (0..80).map(move |column| {
                let x = -4.0 + (column as f64 + 0.5) * 0.1;
                let y = -4.0 + (row as f64 + 0.5) * 0.1;
                (-(x * x + y * y) / (2.0 * width * width)).exp()
            })
        })
        .collect();
    let mut plot = Plotter::new();
    plot.heatmap(values, 80, 80, [-4.0, 4.0], [-4.0, 4.0])
        .color_limits(0.0, 1.0)
        .label("Intensity");
    plot.xlabel("x");
    plot.ylabel("y");
    Ok(plot)
}
const DEFINITION: AppDefinition<Params> =
    AppDefinition::new("Heatmap", &[ViewOption::new("Gaussian", plot, controls)]);
#[cfg(not(target_arch = "wasm32"))]
fn main() -> myplotlib::NativeResult {
    myplotlib::run_native(DEFINITION)
}
#[cfg(target_arch = "wasm32")]
fn main() {
    let _ = DEFINITION;
}
