#[cfg(not(target_arch = "wasm32"))]
fn main() -> myplotlib::Result {
    use myplotlib::{Plotter, YAxisSide};

    let time = [0.0, 1.0, 2.0, 3.0, 4.0];
    let temperature = [18.0, 20.0, 24.0, 23.0, 21.0];
    let pressure = [101.2, 101.0, 100.6, 100.8, 101.1];

    let mut plot = Plotter::new();
    plot.title("Weather readings");
    plot.xlabel("Time (h)");
    plot.ylabel("Temperature (°C)");
    plot.ylim(15.0, 27.0);
    plot.plot(&time, &temperature).label("Temperature");

    let pressure_axis = plot.add_y_axis(YAxisSide::Right);
    plot.y_axis_mut(pressure_axis)
        .label("Pressure (kPa)")
        .limits(100.0, 102.0);
    plot.plot(&time, &pressure)
        .label("Pressure")
        .y_axis(pressure_axis);
    plot.ylim(17.0, 25.0);

    plot.show()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
