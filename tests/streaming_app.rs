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

    Ok(StreamPlan::new(plot, move |sink, _| {
        RUNS.fetch_add(1, Ordering::Relaxed);
        sink.append(series, [[0.0, 1.0], [1.0, 2.0]])?;
        sink.set_progress(0.5)?;
        sink.append(series, [[2.0, 3.0]])?;
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

    while Instant::now() < deadline {
        let output = context.run(input(frame_time), |context| {
            app.update(context, &mut frame);
        });
        finished = contains_text_starting_with(&output, "Finished:");
        if finished {
            assert!(contains_text_starting_with(&output, "Streaming signal"));
            break;
        }
        frame_time += 0.01;
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
    Ok(StreamPlan::new(plot, |_, cancel| {
        while !cancel.is_cancelled() {
            std::thread::yield_now();
        }
        CANCELLED.fetch_add(1, Ordering::Relaxed);
        Ok(())
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
    Ok(StreamPlan::new(plot, |_, cancel| {
        while !cancel.is_cancelled() {
            std::thread::yield_now();
        }
        POLL_CANCELLED.fetch_add(1, Ordering::Relaxed);
        Ok(())
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

    let _ = context.run(input(0.0), |context| app.update(context, &mut frame));
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

    for frame_index in 0..3 {
        let _ = context.run(input(f64::from(frame_index) * 0.01), |context| {
            app.update(context, &mut frame);
        });
    }
    let output = context.run(input(0.04), |context| app.update(context, &mut frame));
    assert!(repaint_delay(&output) > Duration::ZERO);
    assert!(repaint_delay(&output) <= Duration::from_millis(33));

    drop(app);
    let deadline = Instant::now() + Duration::from_secs(1);
    while POLL_CANCELLED.load(Ordering::Relaxed) == 0 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert_eq!(POLL_CANCELLED.load(Ordering::Relaxed), 1);
}
