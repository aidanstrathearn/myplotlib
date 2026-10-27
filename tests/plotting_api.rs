use myplotlib::{AxisScale, Figure, Plotter, YAxisSide};

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
    plot.xscale(AxisScale::Linear);
    plot.yscale(AxisScale::Linear);
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
fn supports_twin_y_axes_with_separate_configuration() {
    let x = [0.0, 1.0, 2.0];
    let temperature = [10.0, 20.0, 30.0];
    let pressure = [98.0, 100.0, 102.0];

    let mut plot = Plotter::new();
    plot.ylabel("Temperature (°C)");
    plot.ylim(0.0, 40.0);

    let pressure_axis = plot.add_y_axis(YAxisSide::Right);
    plot.y_axis_mut(pressure_axis)
        .label("Pressure (kPa)")
        .limits(95.0, 105.0)
        .scale(AxisScale::Linear);

    plot.plot(&x, &temperature).label("Temperature");
    plot.plot(&x, &pressure)
        .label("Pressure")
        .y_axis(pressure_axis);
    plot.axhline(100.0)
        .label("Reference pressure")
        .y_axis(pressure_axis);
}

#[test]
fn supports_mutating_a_series_through_its_id() {
    let mut plot = Plotter::new();
    let series = plot
        .add_points(vec![[0.0, 1.0]])
        .label("Incremental result")
        .id();

    plot.append_points(series, [[1.0, 2.0], [2.0, 3.0]]);
    plot.replace_points(series, vec![[3.0, 4.0]]);
    plot.clear_points(series);
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

#[test]
fn heatmap_renders_with_overlays_and_caches_its_texture() {
    use eframe::egui;
    let mut plot = Plotter::new();
    plot.heatmap(vec![0.0, 1.0, 2.0, 3.0], 2, 2, [10.0, 20.0], [30.0, 50.0])
        .color_limits(0.0, 3.0)
        .label("Value");
    plot.plot(&[10.0, 20.0], &[30.0, 50.0]).label("Overlay");
    let context = egui::Context::default();
    let draw = || {
        context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    plot.show_ui(ui, "heatmap");
                });
            },
        )
    };
    let first = draw();
    assert!(!first.shapes.is_empty());
    assert!(
        first
            .textures_delta
            .set
            .iter()
            .any(|(_, delta)| delta.image.size() == [2, 2])
    );
    let second = draw();
    assert!(
        !second
            .textures_delta
            .set
            .iter()
            .any(|(_, delta)| delta.image.size() == [2, 2])
    );
}

#[test]
#[should_panic(expected = "heatmaps require linear")]
fn heatmap_rejects_logarithmic_spatial_axes() {
    let mut plot = Plotter::new();
    plot.heatmap(vec![1.0], 1, 1, [1.0, 2.0], [1.0, 2.0]);
    plot.xscale(AxisScale::Log10);
    let context = eframe::egui::Context::default();
    let _ = context.run(Default::default(), |ctx| {
        eframe::egui::CentralPanel::default().show(ctx, |ui| {
            plot.show_ui(ui, "heatmap");
        });
    });
}
