use myplotlib::{Figure, Plotter, Result};

#[test]
fn supports_the_public_api_used_by_lasers() {
    let x = [0.0, 1.0, 2.0];
    let forward = [0.0, 1.0, 4.0];
    let backward = [4.0, 1.0, 0.0];

    let mut plot = Plotter::new();
    plot.plot(&x, &forward).label("Forward");
    plot.plot(&x, &backward).label("Backward");
    plot.plot(&x, &forward);
    plot.axvline(1.0)
        .label(format!("Small-signal threshold ({:.3e} W)", 1.0));
    plot.xlabel("Position (m)");
    plot.ylabel("Power (W)");
    plot.title("Lasers compatibility test");

    // Type-check the consuming call without opening a blocking native window.
    let show: fn(Plotter) -> Result = Plotter::show;
    let _ = (plot, show);
}

#[test]
fn supports_the_public_subplot_api() {
    let x = [0.0, 1.0, 2.0];
    let forward = [0.0, 1.0, 4.0];
    let backward = [4.0, 1.0, 0.0];

    let mut figure = Figure::subplots(1, 2);
    figure.suptitle("Forward and backward power");

    let forward_axes = figure.axes_mut(0, 0);
    forward_axes.plot(&x, &forward).label("Forward");
    forward_axes.title("Forward power");

    let backward_axes = figure.axes_mut(0, 1);
    backward_axes.plot(&x, &backward).label("Backward");
    backward_axes.title("Backward power");

    // Type-check the consuming call without opening a blocking native window.
    let show: fn(Figure) -> Result = Figure::show;
    let _ = (figure, show);
}
