//! Run with `cargo run --example log_scale`.
//!
//! This is a native example; the WASM entry point is a compilation-only stub.

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> myplotlib::Result {
    use myplotlib::{AxisScale, Plotter};

    let x: Vec<f64> = (-20..=30)
        .map(|exponent| 10.0_f64.powf(f64::from(exponent) / 10.0))
        .collect();
    let y: Vec<f64> = x.iter().map(|value| value.powi(2)).collect();

    let mut plot = Plotter::new();
    plot.plot(&x, &y).label("y = x²");
    plot.xscale(AxisScale::Log10);
    plot.yscale(AxisScale::Log10);
    plot.xlabel("x");
    plot.ylabel("y");
    plot.title("Log scale axes");

    plot.show()
}
