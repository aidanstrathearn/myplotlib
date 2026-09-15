use myplotlib::{Figure, Plotter};

#[test]
fn supports_the_public_plotting_api() {
    let x = [0.0, 1.0, 2.0];
    let increasing = [0.0, 1.0, 4.0];
    let decreasing = [4.0, 1.0, 0.0];

    let mut plot = Plotter::new();
    plot.plot(&x, &increasing).label("Increasing");
    plot.plot(&x, &decreasing).label("Decreasing");
    plot.plot(&x, &increasing);
    plot.axvline(1.0)
        .label(format!("Reference (x = {:.3e})", 1.0));
    plot.xlim(0.0, 2.0);
    plot.ylim(0.0, 4.0);
    plot.xlabel("x");
    plot.ylabel("y");
    plot.title("Plotting API test");

    // Type-check the consuming call without opening a blocking native window.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let show: fn(Plotter) -> myplotlib::Result = Plotter::show;
        let _ = (plot, show);
    }
}

#[test]
fn supports_the_public_subplot_api() {
    let x = [0.0, 1.0, 2.0];
    let increasing = [0.0, 1.0, 4.0];
    let decreasing = [4.0, 1.0, 0.0];

    let mut figure = Figure::subplots(1, 2);
    figure.suptitle("Increasing and decreasing series");

    let increasing_axes = figure.axes_mut(0, 0);
    increasing_axes.plot(&x, &increasing).label("Increasing");
    increasing_axes.title("Increasing series");

    let decreasing_axes = figure.axes_mut(0, 1);
    decreasing_axes.plot(&x, &decreasing).label("Decreasing");
    decreasing_axes.title("Decreasing series");

    // Type-check the consuming call without opening a blocking native window.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let show: fn(Figure) -> myplotlib::Result = Figure::show;
        let _ = (figure, show);
    }
}
