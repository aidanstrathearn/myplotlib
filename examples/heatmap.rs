//! Run with `cargo run --example heatmap`.
#[cfg(not(target_arch = "wasm32"))]
fn main() -> myplotlib::Result {
    use myplotlib::Figure;
    let values: Vec<f64> = (0..60)
        .flat_map(|row| {
            (0..80).map(move |column| {
                let x = -4.0 + (column as f64 + 0.5) * 0.1;
                let y = -3.0 + (row as f64 + 0.5) * 0.1;
                (-0.5 * (x * x + y * y)).exp()
            })
        })
        .collect();
    let mut figure = Figure::subplots(1, 2);
    figure.suptitle("Heatmap and line overlay");
    let axes = figure.axes_mut(0, 0);
    axes.heatmap(values, 60, 80, [-4.0, 4.0], [-3.0, 3.0])
        .color_limits(0.0, 1.0)
        .label("Intensity");
    axes.axhline(0.0).label("y = 0");
    axes.xlabel("x");
    axes.ylabel("y");
    let x: Vec<f64> = (0..80).map(|i| -4.0 + (i as f64 + 0.5) * 0.1).collect();
    let y: Vec<f64> = x.iter().map(|x| (-0.5 * x * x).exp()).collect();
    figure.axes_mut(0, 1).plot(&x, &y).label("Section at y = 0");
    figure.show()
}
#[cfg(target_arch = "wasm32")]
fn main() {}
