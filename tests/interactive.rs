use eframe::egui;
use egui_plot::PlotMemory;
use myplotlib::{
    App, AppDefinition, AppResult, Plotter, Slider, SliderGrid, SliderGroup, ViewOption,
};
use std::cell::Cell;

fn input(time: f64, events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        time: Some(time),
        events,
        ..Default::default()
    }
}

fn pointer_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn plot_frame(ctx: &egui::Context, plot: &Plotter, input: egui::RawInput) -> PlotMemory {
    let mut id = egui::Id::NULL;
    let _ = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            id = ui.make_persistent_id(egui::Id::new("test-plot"));
            plot.show_ui(ui, "test-plot");
        });
    });
    PlotMemory::load(ctx, id).unwrap()
}

fn sample_plot() -> Plotter {
    let mut plot = Plotter::new();
    plot.add_points(vec![[0.0, 0.0], [5.0, 1.0], [10.0, 0.0]])
        .label("Signal");
    plot.xlim(0.0, 10.0);
    plot
}

#[test]
fn x_limits_allow_navigation_and_restore_on_reset_and_recomputation() {
    let ctx = egui::Context::default();
    let plot = sample_plot();
    let first = plot_frame(&ctx, &plot, input(0.0, vec![]));
    assert_eq!(first.bounds().range_x(), 0.0..=10.0);
    let pos = first.transform().frame().center();
    plot_frame(
        &ctx,
        &plot,
        input(
            0.1,
            vec![egui::Event::PointerMoved(pos), pointer_button(pos, true)],
        ),
    );
    let moved = pos + egui::vec2(80.0, 0.0);
    plot_frame(
        &ctx,
        &plot,
        input(0.2, vec![egui::Event::PointerMoved(moved)]),
    );
    let panned = plot_frame(&ctx, &plot, input(0.3, vec![pointer_button(moved, false)]));
    assert_ne!(panned.bounds().range_x(), 0.0..=10.0);
    let idle = plot_frame(&ctx, &plot, input(0.4, vec![]));
    assert_eq!(idle.bounds().range_x(), panned.bounds().range_x());

    // A double-click restores the requested limits, rather than fitting the data.
    for (time, pressed) in [(1.0, true), (1.05, false), (1.1, true), (1.15, false)] {
        plot_frame(
            &ctx,
            &plot,
            input(time, vec![pointer_button(moved, pressed)]),
        );
    }
    let reset = plot_frame(&ctx, &plot, input(1.2, vec![]));
    assert_eq!(reset.bounds().range_x(), 0.0..=10.0);

    // Zoom must also survive the next redraw.
    let zoomed = plot_frame(&ctx, &plot, input(1.3, vec![egui::Event::Zoom(2.0)]));
    assert!(zoomed.bounds().width() < 10.0);
    let idle = plot_frame(&ctx, &plot, input(1.4, vec![]));
    assert_eq!(idle.bounds().range_x(), zoomed.bounds().range_x());

    // A slider callback returns a fresh Plotter, even when its xlim is unchanged.
    let recomputed = plot_frame(&ctx, &sample_plot(), input(1.5, vec![]));
    assert_eq!(recomputed.bounds().range_x(), 0.0..=10.0);
}

thread_local! {
    static COMPUTATIONS: Cell<usize> = const { Cell::new(0) };
}

#[derive(Default)]
struct Params {
    amplitude: f64,
}

fn controls(params: &mut Params) -> SliderGrid<'_> {
    SliderGrid::new(
        1,
        [SliderGroup::new(
            "Controls",
            [Slider::new("Amplitude", &mut params.amplitude, 0.0..=10.0)],
        )],
    )
}

fn success(params: &mut Params) -> AppResult {
    COMPUTATIONS.set(COMPUTATIONS.get() + 1);
    let mut plot = sample_plot();
    plot.title(format!("Amplitude: {}", params.amplitude));
    Ok(plot)
}

fn failure(_: &mut Params) -> AppResult {
    COMPUTATIONS.set(COMPUTATIONS.get() + 1);
    Err("example computation failed".into())
}

fn key(key: egui::Key) -> Vec<egui::Event> {
    [true, false]
        .into_iter()
        .map(|pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        })
        .collect()
}

fn contains_text(output: &egui::FullOutput, expected: &str) -> bool {
    output.shapes.iter().any(|shape| match &shape.shape {
        egui::Shape::Text(text) => text.galley.job.text == expected,
        _ => false,
    })
}

#[test]
fn slider_app_renders_caches_switches_views_and_resets() {
    use eframe::App as _;

    COMPUTATIONS.set(0);
    const VIEWS: &[ViewOption<Params>] = &[
        ViewOption::new("Signal", success, controls),
        ViewOption::new("Error", failure, controls),
    ];
    let ctx = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = App::new(&creation, AppDefinition::new("Test", "canvas", VIEWS));
    let mut frame = eframe::Frame::_new_kittest();
    let mut draw = |time, events| ctx.run(input(time, events), |ctx| app.update(ctx, &mut frame));

    let output = draw(0.0, vec![]);
    assert!(contains_text(&output, "Amplitude: 0"));
    assert_eq!(COMPUTATIONS.get(), 1);
    let output = draw(0.1, vec![]);
    assert_eq!(COMPUTATIONS.get(), 1);

    let slider_pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if rect.rect.height() <= 8.0 && (50.0..=150.0).contains(&rect.rect.width()) =>
            {
                Some(rect.rect.center())
            }
            _ => None,
        })
        .expect("the amplitude slider should be visible");
    let output = draw(
        0.2,
        vec![
            egui::Event::PointerMoved(slider_pos),
            pointer_button(slider_pos, true),
        ],
    );
    assert!(!contains_text(&output, "Amplitude: 0"));
    assert_eq!(COMPUTATIONS.get(), 2);
    draw(0.25, vec![pointer_button(slider_pos, false)]);
    draw(0.3, vec![]);
    assert_eq!(COMPUTATIONS.get(), 2);

    let output = draw(0.4, key(egui::Key::Num2));
    assert!(contains_text(&output, "example computation failed"));
    assert_eq!(COMPUTATIONS.get(), 3);
    draw(0.5, vec![]);
    assert_eq!(COMPUTATIONS.get(), 3);

    let output = draw(0.6, key(egui::Key::R));
    assert!(contains_text(&output, "Amplitude: 0"));
    assert_eq!(COMPUTATIONS.get(), 4);
}
