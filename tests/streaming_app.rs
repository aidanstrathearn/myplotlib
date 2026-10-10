#![cfg(feature = "streaming")]

use eframe::egui;
use myplotlib::{
    App, AppDefinition, Plotter, SliderGrid, StreamPlan, StreamPlanResult, ViewOption,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use web_time::Instant;

static RUNS: AtomicUsize = AtomicUsize::new(0);
static CANCELLED: AtomicUsize = AtomicUsize::new(0);
static POLL_CANCELLED: AtomicUsize = AtomicUsize::new(0);

#[derive(Default)]
struct Params;

fn controls(_: &mut Params) -> SliderGrid<'_> {
    SliderGrid::empty()
}

fn stream(_: &mut Params) -> StreamPlanResult {
    let mut plot = Plotter::new();
    let series = plot.add_points(Vec::new()).label("Results").id();
    plot.title("Streaming signal");

    Ok(StreamPlan::new(plot, move |stream| {
        RUNS.fetch_add(1, Ordering::Relaxed);
        stream.append(series, [[0.0, 1.0], [1.0, 2.0]])?;
        stream.set_progress(0.5)?;
        stream.append(series, [[2.0, 3.0]])?;
        Ok(())
    }))
}

fn input(time: f64) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        time: Some(time),
        ..Default::default()
    }
}

fn key_input(time: f64, key: egui::Key) -> egui::RawInput {
    let mut input = input(time);
    input.events = [true, false]
        .into_iter()
        .map(|pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        })
        .collect();
    input
}

fn pointer_input(time: f64, position: egui::Pos2, pressed: bool) -> egui::RawInput {
    let mut input = input(time);
    input.events = vec![
        egui::Event::PointerMoved(position),
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ];
    input
}

fn text_position(output: &egui::FullOutput, expected: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == expected => {
                Some(text.pos + egui::vec2(3.0, 3.0))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{expected:?} should be visible"))
}

fn contains_text_starting_with(output: &egui::FullOutput, expected: &str) -> bool {
    output.shapes.iter().any(|shape| match &shape.shape {
        egui::Shape::Text(text) => text.galley.job.text.starts_with(expected),
        _ => false,
    })
}

fn repaint_delay(output: &egui::FullOutput) -> Duration {
    output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
}

#[test]
fn streaming_view_runs_in_the_standard_app_and_reaches_finished_state() {
    use eframe::App as _;

    RUNS.store(0, Ordering::Relaxed);
    const VIEWS: &[ViewOption<Params>] = &[ViewOption::streaming("Stream", stream, controls)];
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = App::new(&creation, AppDefinition::new("Test", VIEWS));
    let mut frame = eframe::Frame::_new_kittest();
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut frame_time = 0.0;
    let mut finished = false;

    let ready = context.run(input(frame_time), |context| {
        app.update(context, &mut frame);
    });
    assert!(contains_text_starting_with(&ready, "Ready"));
    assert_eq!(RUNS.load(Ordering::Relaxed), 0);
    frame_time += 0.01;
    let ready_with_timing = context.run(input(frame_time), |context| {
        app.update(context, &mut frame);
    });
    assert!(contains_text_starting_with(&ready_with_timing, "Compute: "));
    assert_eq!(RUNS.load(Ordering::Relaxed), 0);
    let start = text_position(&ready_with_timing, "Start");
    frame_time += 0.01;
    let _ = context.run(pointer_input(frame_time, start, true), |context| {
        app.update(context, &mut frame);
    });
    frame_time += 0.01;
    let _ = context.run(pointer_input(frame_time, start, false), |context| {
        app.update(context, &mut frame);
    });

    while Instant::now() < deadline {
        frame_time += 0.01;
        let output = context.run(input(frame_time), |context| {
            app.update(context, &mut frame);
        });
        finished = contains_text_starting_with(&output, "Finished");
        if finished {
            assert!(contains_text_starting_with(&output, "Streaming signal"));
            assert!(contains_text_starting_with(&output, "Compute: "));
            break;
        }
        std::thread::yield_now();
    }

    assert!(finished);
    assert_eq!(RUNS.load(Ordering::Relaxed), 1);

    let settled = context.run(input(frame_time + 0.01), |context| {
        app.update(context, &mut frame);
    });
    assert_eq!(repaint_delay(&settled), Duration::MAX);
}

fn waiting_stream(_: &mut Params) -> StreamPlanResult {
    let mut plot = Plotter::new();
    plot.add_points(Vec::new());
    Ok(StreamPlan::new(plot, |stream| {
        loop {
            if stream.checkpoint().is_err() {
                CANCELLED.fetch_add(1, Ordering::Relaxed);
                return Ok(());
            }
            std::thread::yield_now();
        }
    }))
}

fn immediate(_: &mut Params) -> myplotlib::AppResult {
    let mut plot = Plotter::new();
    plot.add_points(vec![[0.0, 0.0]]);
    plot.title("Immediate signal");
    Ok(plot)
}

fn polling_stream(_: &mut Params) -> StreamPlanResult {
    let mut plot = Plotter::new();
    plot.add_points(Vec::new());
    Ok(StreamPlan::new(plot, |stream| {
        loop {
            if stream.checkpoint().is_err() {
                POLL_CANCELLED.fetch_add(1, Ordering::Relaxed);
                return Ok(());
            }
            std::thread::yield_now();
        }
    }))
}

#[test]
fn switching_views_cancels_the_previous_stream() {
    use eframe::App as _;

    CANCELLED.store(0, Ordering::Relaxed);
    const VIEWS: &[ViewOption<Params>] = &[
        ViewOption::streaming("Stream", waiting_stream, controls),
        ViewOption::new("Immediate", immediate, controls),
    ];
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = App::new(&creation, AppDefinition::new("Test", VIEWS));
    let mut frame = eframe::Frame::_new_kittest();

    let ready = context.run(input(0.0), |context| app.update(context, &mut frame));
    let start = text_position(&ready, "Start");
    let _ = context.run(pointer_input(0.01, start, true), |context| {
        app.update(context, &mut frame)
    });
    let _ = context.run(pointer_input(0.02, start, false), |context| {
        app.update(context, &mut frame)
    });
    let output = context.run(key_input(0.1, egui::Key::Num2), |context| {
        app.update(context, &mut frame);
    });
    assert!(contains_text_starting_with(&output, "Immediate signal"));

    let deadline = Instant::now() + Duration::from_secs(1);
    while CANCELLED.load(Ordering::Relaxed) == 0 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert_eq!(CANCELLED.load(Ordering::Relaxed), 1);
}

#[test]
fn active_stream_schedules_ui_thread_repaints() {
    use eframe::App as _;

    POLL_CANCELLED.store(0, Ordering::Relaxed);
    const VIEWS: &[ViewOption<Params>] =
        &[ViewOption::streaming("Stream", polling_stream, controls)];
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = App::new(&creation, AppDefinition::new("Test", VIEWS));
    let mut frame = eframe::Frame::_new_kittest();

    let ready = context.run(input(0.0), |context| app.update(context, &mut frame));
    assert_eq!(repaint_delay(&ready), Duration::ZERO);
    let mut settled_ready = ready;
    let mut frame_time = 0.0;
    for frame_index in 1..=10 {
        frame_time = f64::from(frame_index) * 0.1;
        settled_ready = context.run(input(frame_time), |context| app.update(context, &mut frame));
        if repaint_delay(&settled_ready) == Duration::MAX {
            break;
        }
    }
    assert_eq!(repaint_delay(&settled_ready), Duration::MAX);
    let start = text_position(&settled_ready, "Start");
    frame_time += 0.01;
    let _ = context.run(pointer_input(frame_time, start, true), |context| {
        app.update(context, &mut frame)
    });
    frame_time += 0.01;
    let _ = context.run(pointer_input(frame_time, start, false), |context| {
        app.update(context, &mut frame)
    });
    frame_time += 0.01;
    let mut output = context.run(input(frame_time), |context| app.update(context, &mut frame));
    for _ in 0..10 {
        if repaint_delay(&output) > Duration::ZERO {
            break;
        }
        frame_time += 0.1;
        output = context.run(input(frame_time), |context| app.update(context, &mut frame));
    }
    assert!(repaint_delay(&output) > Duration::ZERO);
    assert!(repaint_delay(&output) <= Duration::from_millis(33));
    assert!(contains_text_starting_with(&output, "Running"));
    assert!(contains_text_starting_with(&output, "Compute: "));

    drop(app);
    let deadline = Instant::now() + Duration::from_secs(1);
    while POLL_CANCELLED.load(Ordering::Relaxed) == 0 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert_eq!(POLL_CANCELLED.load(Ordering::Relaxed), 1);
}
