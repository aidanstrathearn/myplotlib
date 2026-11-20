//! Run with `cargo run --features streaming --example streaming_heatmap`.
//! Press Start to fill the heatmap. NaN cells remain transparent until computed.

use myplotlib::{
    AppDefinition, Plotter, Slider, SliderGrid, SliderGroup, StreamPlan, StreamPlanResult,
    ViewOption,
};

const ROWS: usize = 200;
const COLUMNS: usize = 160;
const BATCH_ROWS: usize = 4;

struct Params {
    coupling: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self { coupling: 1.0 }
    }
}

fn controls(params: &mut Params) -> SliderGrid<'_> {
    SliderGrid::new(
        1,
        [SliderGroup::new(
            "Signal",
            [Slider::new("Coupling", &mut params.coupling, 0.2..=2.0)],
        )],
    )
}

fn plot(params: &mut Params) -> StreamPlanResult<Params> {
    let coupling = params.coupling;
    let mut plot = Plotter::new();
    plot.heatmap(
        vec![f64::NAN; ROWS * COLUMNS],
        ROWS,
        COLUMNS,
        [-4.0, 4.0],
        [0.0, 10.0],
    )
    .color_limits(0.0, 1.0)
    .label("Excited-state probability");
    plot.xlabel("Detuning");
    plot.ylabel("Elapsed time");

    Ok(StreamPlan::new(plot, move |stream| {
        for first_row in (0..ROWS).step_by(BATCH_ROWS) {
            stream.checkpoint()?;
            let end_row = (first_row + BATCH_ROWS).min(ROWS);
            let values = (first_row..end_row)
                .flat_map(|row| {
                    (0..COLUMNS).map(move |column| {
                        // Sample at cell centres, since heatmap bounds are cell edges.
                        let detuning = -4.0 + (column as f64 + 0.5) * 8.0 / COLUMNS as f64;
                        let time = (row as f64 + 0.5) * 10.0 / ROWS as f64;
                        let frequency = coupling.hypot(detuning);
                        (coupling / frequency).powi(2) * (frequency * time / 2.0).sin().powi(2)
                    })
                })
                .collect();
            stream.set_heatmap_rows(first_row, values)?;
            stream.set_progress(end_row as f32 / ROWS as f32)?;
            // Slow this lightweight example down so progressive filling is visible.
            #[cfg(not(target_arch = "wasm32"))]
            std::thread::sleep(std::time::Duration::from_millis(33));
        }
        Ok(())
    })
    .label("Population evolution"))
}

const DEFINITION: AppDefinition<Params> = AppDefinition::new(
    "Streaming heatmap",
    &[ViewOption::streaming("Evolution", plot, controls)],
);

#[cfg(not(target_arch = "wasm32"))]
fn main() -> myplotlib::NativeResult {
    myplotlib::run_native(DEFINITION)
}

#[cfg(target_arch = "wasm32")]
fn main() {
    let _ = DEFINITION;
}
