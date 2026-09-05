use std::f64::consts::TAU;

use myplotlib::{Figure, Result};

fn main() -> Result {
    let samples = 400;
    let x: Vec<f64> = (0..samples)
        .map(|index| 2.0 * TAU * index as f64 / (samples - 1) as f64)
        .collect();
    let sine: Vec<f64> = x.iter().map(|value| value.sin()).collect();
    let cosine: Vec<f64> = x.iter().map(|value| value.cos()).collect();
    let damped_sine: Vec<f64> = x
        .iter()
        .map(|value| (-value / 5.0).exp() * (2.0 * value).sin())
        .collect();

    let mut figure = Figure::subplots(2, 2);
    figure.suptitle("Subplot demonstration");

    let waves = figure.axes_mut(0, 0);
    waves.plot(&x, &sine).label("sin(x)");
    waves.plot(&x, &cosine).label("cos(x)");
    waves.axhline(0.0).label("zero");
    waves.xlabel("x");
    waves.ylabel("amplitude");
    waves.title("Trigonometric waves");

    let damped = figure.axes_mut(0, 1);
    damped.plot(&x, &damped_sine).label("damped sine");
    damped.axvline(5.0).label("x = 5");
    damped.xlabel("time");
    damped.ylabel("amplitude");
    damped.title("Damped oscillation");

    let parabola_x: Vec<f64> = (-200..=200).map(|value| value as f64 / 50.0).collect();
    let parabola_y: Vec<f64> = parabola_x.iter().map(|value| value * value).collect();
    let parabola = figure.axes_mut(1, 0);
    parabola.plot(&parabola_x, &parabola_y).label("y = x²");
    parabola.xlabel("x");
    parabola.ylabel("y");
    parabola.title("Parabola");

    let angle: Vec<f64> = (0..samples)
        .map(|index| TAU * index as f64 / (samples - 1) as f64)
        .collect();
    let circle_x: Vec<f64> = angle.iter().map(|value| value.cos()).collect();
    let circle_y: Vec<f64> = angle.iter().map(|value| value.sin()).collect();
    let circle = figure.axes_mut(1, 1);
    circle.plot(&circle_x, &circle_y).label("unit circle");
    circle.axhline(0.0);
    circle.axvline(0.0);
    circle.xlabel("x");
    circle.ylabel("y");
    circle.title("Parametric circle");

    figure.show()
}
