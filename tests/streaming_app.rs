#![cfg(feature = "streaming")]

use eframe::egui;
use myplotlib::{
    App, AppDefinition, Plotter, Slider, SliderGrid, SliderGroup, StreamPlan, StreamPlanResult,
    ViewOption,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use web_time::Instant;

static RUNS: AtomicUsize = AtomicUsize::new(0);
static AUTOPLAY_PREPARATIONS: AtomicUsize = AtomicUsize::new(0);
static AUTOPLAY_RUNS: AtomicUsize = AtomicUsize::new(0);
static AUTOPLAY_INPUT_PREPARATIONS: AtomicUsize = AtomicUsize::new(0);
static AUTOPLAY_INPUT_RUNS: AtomicUsize = AtomicUsize::new(0);
static AUTOPLAY_WAIT_PREPARATIONS: AtomicUsize = AtomicUsize::new(0);
static AUTOPLAY_WAIT_RUNS: AtomicUsize = AtomicUsize::new(0);
static AUTOPLAY_WAIT_CANCELLED: AtomicUsize = AtomicUsize::new(0);
static CANCELLED: AtomicUsize = AtomicUsize::new(0);
static POLL_CANCELLED: AtomicUsize = AtomicUsize::new(0);
static UPDATE_PREPARATIONS: AtomicUsize = AtomicUsize::new(0);
static UPDATE_RUNS: AtomicUsize = AtomicUsize::new(0);
static UPDATE_BINDINGS: AtomicUsize = AtomicUsize::new(0);

#[derive(Default)]
struct Params;

type PlanResult = StreamPlanResult<Params>;

fn controls(_: &mut Params) -> SliderGrid<'_> {
    SliderGrid::empty()
}

fn stream(_: &mut Params) -> PlanResult {
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

fn autoplay_stream(_: &mut Params) -> PlanResult {
    AUTOPLAY_PREPARATIONS.fetch_add(1, Ordering::Relaxed);
    let mut plot = Plotter::new();
    plot.add_points(Vec::new());
    Ok(StreamPlan::new(plot, |_| {
        AUTOPLAY_RUNS.fetch_add(1, Ordering::Relaxed);
        Ok(())
    })
    .label("Automatic calculation"))
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

struct TestApp<P: 'static> {
    context: egui::Context,
    app: App<P>,
    frame: eframe::Frame,
    time: f64,
}

impl<P: Default + 'static> TestApp<P> {
    fn new(views: &'static [ViewOption<P>]) -> Self {
        let context = egui::Context::default();
        let creation = eframe::CreationContext::_new_kittest(context.clone());
        let app = App::new(&creation, AppDefinition::new("Test", views));
        Self {
            context,
            app,
            frame: eframe::Frame::_new_kittest(),
            time: 0.0,
        }
    }

    fn update(&mut self, mut input: egui::RawInput) -> egui::FullOutput {
        use eframe::App as _;

        self.time += 0.1;
        input.time = Some(self.time);
        self.context
            .run(input, |context| self.app.update(context, &mut self.frame))
    }

    fn idle(&mut self) -> egui::FullOutput {
        self.update(input(0.0))
    }

    fn key(&mut self, key: egui::Key) -> egui::FullOutput {
        self.update(key_input(0.0, key))
    }

    fn click(&mut self, text: &str) -> egui::FullOutput {
        let output = self.idle();
        self.click_at(text_position(&output, text))
    }

    fn click_at(&mut self, position: egui::Pos2) -> egui::FullOutput {
        let _ = self.update(pointer_input(0.0, position, true));
        let _ = self.update(pointer_input(0.0, position, false));
        self.idle()
    }

    fn wait_for_status(&mut self, status: &str) -> egui::FullOutput {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let output = self.idle();
            if contains_text_starting_with(&output, status) {
                return output;
            }
            assert!(Instant::now() < deadline, "stream should reach {status}");
            std::thread::yield_now();
        }
    }
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
    assert!(contains_text_starting_with(&ready, "Streaming"));
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

#[test]
fn user_autoplay_is_per_view_and_survives_view_switches_until_reset() {
    AUTOPLAY_PREPARATIONS.store(0, Ordering::Relaxed);
    AUTOPLAY_RUNS.store(0, Ordering::Relaxed);
    const VIEWS: &[ViewOption<Params>] = &[
        ViewOption::streaming("Stream", autoplay_stream, controls),
        ViewOption::streaming("Manual stream", autoplay_stream, controls),
        ViewOption::new("Immediate", immediate, controls),
    ];
    let mut app = TestApp::new(VIEWS);

    let first = app.idle();
    assert!(contains_text_starting_with(&first, "Automatic calculation"));
    assert!(contains_text_starting_with(&first, "Ready"));
    assert!(contains_text_starting_with(&first, "Autoplay: Off"));
    assert_eq!(AUTOPLAY_PREPARATIONS.load(Ordering::Relaxed), 1);
    assert_eq!(AUTOPLAY_RUNS.load(Ordering::Relaxed), 0);

    let _ = app.click("Autoplay: Off");
    let finished = app.wait_for_status("Finished");
    assert!(contains_text_starting_with(&finished, "Autoplay: On"));
    assert_eq!(AUTOPLAY_PREPARATIONS.load(Ordering::Relaxed), 1);
    assert_eq!(AUTOPLAY_RUNS.load(Ordering::Relaxed), 1);

    // Completion leaves autoplay enabled without repeatedly running the job.
    for _ in 0..3 {
        let settled = app.idle();
        assert!(contains_text_starting_with(&settled, "Finished"));
    }
    assert_eq!(AUTOPLAY_RUNS.load(Ordering::Relaxed), 1);

    let manual = app.key(egui::Key::Num2);
    assert!(contains_text_starting_with(&manual, "Ready"));
    assert!(contains_text_starting_with(&manual, "Autoplay: Off"));
    assert_eq!(AUTOPLAY_RUNS.load(Ordering::Relaxed), 1);

    let immediate_output = app.key(egui::Key::Num3);
    assert!(contains_text_starting_with(
        &immediate_output,
        "Immediate signal"
    ));
    assert!(!contains_text_starting_with(&immediate_output, "Autoplay:"));

    let restarted = app.key(egui::Key::Num1);
    assert!(contains_text_starting_with(
        &restarted,
        "Automatic calculation"
    ));
    assert!(contains_text_starting_with(&restarted, "Running"));
    assert!(contains_text_starting_with(&restarted, "Autoplay: On"));
    let _ = app.wait_for_status("Finished");
    assert_eq!(AUTOPLAY_PREPARATIONS.load(Ordering::Relaxed), 3);
    assert_eq!(AUTOPLAY_RUNS.load(Ordering::Relaxed), 2);

    let disabled = app.click("Autoplay: On");
    assert!(contains_text_starting_with(&disabled, "Autoplay: Off"));
    assert!(contains_text_starting_with(&disabled, "Finished"));
    let _ = app.key(egui::Key::Num3);
    let manual = app.key(egui::Key::Num1);
    assert!(contains_text_starting_with(&manual, "Ready"));
    assert!(contains_text_starting_with(&manual, "Autoplay: Off"));
    assert_eq!(AUTOPLAY_RUNS.load(Ordering::Relaxed), 2);

    let _ = app.click("Start");
    let finished = app.wait_for_status("Finished");
    assert!(contains_text_starting_with(&finished, "Autoplay: Off"));
    assert_eq!(AUTOPLAY_RUNS.load(Ordering::Relaxed), 3);

    // Enabling autoplay on a finished plot starts a fresh calculation.
    let _ = app.click("Autoplay: Off");
    let _ = app.wait_for_status("Finished");
    assert_eq!(AUTOPLAY_RUNS.load(Ordering::Relaxed), 4);
    assert_eq!(AUTOPLAY_PREPARATIONS.load(Ordering::Relaxed), 5);

    let reset = app.key(egui::Key::R);
    assert!(contains_text_starting_with(&reset, "Ready"));
    assert!(contains_text_starting_with(&reset, "Autoplay: Off"));
    assert_eq!(AUTOPLAY_RUNS.load(Ordering::Relaxed), 4);
}

#[derive(Default)]
struct AutoplayInputParams {
    value: u64,
}

fn autoplay_input_controls(params: &mut AutoplayInputParams) -> SliderGrid<'_> {
    SliderGrid::new(
        1,
        [SliderGroup::new(
            "Inputs",
            [Slider::new("Value", &mut params.value, 0..=100)],
        )],
    )
}

fn autoplay_input_stream(
    params: &mut AutoplayInputParams,
) -> StreamPlanResult<AutoplayInputParams> {
    AUTOPLAY_INPUT_PREPARATIONS.fetch_add(1, Ordering::Relaxed);
    let mut plot = Plotter::new();
    plot.add_points(Vec::new());
    plot.title(format!("Input: {}", params.value));
    Ok(StreamPlan::new(plot, |_| {
        AUTOPLAY_INPUT_RUNS.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }))
}

#[test]
fn input_changes_run_only_while_user_autoplay_is_enabled() {
    AUTOPLAY_INPUT_PREPARATIONS.store(0, Ordering::Relaxed);
    AUTOPLAY_INPUT_RUNS.store(0, Ordering::Relaxed);
    const VIEWS: &[ViewOption<AutoplayInputParams>] = &[ViewOption::streaming(
        "Stream",
        autoplay_input_stream,
        autoplay_input_controls,
    )];
    let mut app = TestApp::new(VIEWS);
    let _ = app.idle();
    let ready = app.idle();
    let slider_rect = ready
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if rect.rect.height() <= 8.0 && (50.0..=150.0).contains(&rect.rect.width()) =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
        .expect("the input slider should be visible");

    let manual = app.click_at(slider_rect.lerp_inside(egui::vec2(0.75, 0.5)));
    assert!(contains_text_starting_with(&manual, "Ready"));
    assert!(!contains_text_starting_with(&manual, "Input: 0"));
    assert_eq!(AUTOPLAY_INPUT_PREPARATIONS.load(Ordering::Relaxed), 2);
    assert_eq!(AUTOPLAY_INPUT_RUNS.load(Ordering::Relaxed), 0);

    let _ = app.click("Autoplay: Off");
    let _ = app.wait_for_status("Finished");
    assert_eq!(AUTOPLAY_INPUT_PREPARATIONS.load(Ordering::Relaxed), 2);
    assert_eq!(AUTOPLAY_INPUT_RUNS.load(Ordering::Relaxed), 1);

    let _ = app.click_at(slider_rect.lerp_inside(egui::vec2(0.25, 0.5)));
    let finished = app.wait_for_status("Finished");
    assert!(contains_text_starting_with(&finished, "Autoplay: On"));
    assert_eq!(AUTOPLAY_INPUT_PREPARATIONS.load(Ordering::Relaxed), 3);
    assert_eq!(AUTOPLAY_INPUT_RUNS.load(Ordering::Relaxed), 2);

    let _ = app.click("Autoplay: On");
    let manual = app.click_at(slider_rect.center());
    assert!(contains_text_starting_with(&manual, "Ready"));
    assert!(contains_text_starting_with(&manual, "Autoplay: Off"));
    assert_eq!(AUTOPLAY_INPUT_PREPARATIONS.load(Ordering::Relaxed), 4);
    assert_eq!(AUTOPLAY_INPUT_RUNS.load(Ordering::Relaxed), 2);
}

fn autoplay_waiting_stream(_: &mut Params) -> PlanResult {
    AUTOPLAY_WAIT_PREPARATIONS.fetch_add(1, Ordering::Relaxed);
    let mut plot = Plotter::new();
    plot.add_points(Vec::new());
    Ok(StreamPlan::new(plot, |stream| {
        AUTOPLAY_WAIT_RUNS.fetch_add(1, Ordering::Relaxed);
        loop {
            if stream.checkpoint().is_err() {
                AUTOPLAY_WAIT_CANCELLED.fetch_add(1, Ordering::Relaxed);
                return Ok(());
            }
            std::thread::yield_now();
        }
    }))
}

#[test]
fn toggling_autoplay_preserves_active_work_and_stop_does_not_restart_it() {
    AUTOPLAY_WAIT_PREPARATIONS.store(0, Ordering::Relaxed);
    AUTOPLAY_WAIT_RUNS.store(0, Ordering::Relaxed);
    AUTOPLAY_WAIT_CANCELLED.store(0, Ordering::Relaxed);
    const VIEWS: &[ViewOption<Params>] = &[ViewOption::streaming(
        "Stream",
        autoplay_waiting_stream,
        controls,
    )];
    let mut app = TestApp::new(VIEWS);

    let _ = app.click("Autoplay: Off");
    let running = app.wait_for_status("Running");
    assert!(contains_text_starting_with(&running, "Autoplay: On"));

    let disabled = app.click("Autoplay: On");
    assert!(contains_text_starting_with(&disabled, "Running"));
    assert!(contains_text_starting_with(&disabled, "Autoplay: Off"));
    assert_eq!(AUTOPLAY_WAIT_CANCELLED.load(Ordering::Relaxed), 0);

    let enabled = app.click("Autoplay: Off");
    assert!(contains_text_starting_with(&enabled, "Running"));
    assert!(contains_text_starting_with(&enabled, "Autoplay: On"));
    assert_eq!(AUTOPLAY_WAIT_PREPARATIONS.load(Ordering::Relaxed), 1);
    assert_eq!(AUTOPLAY_WAIT_CANCELLED.load(Ordering::Relaxed), 0);

    let _ = app.click("Stop");
    let stopped = app.wait_for_status("Stopped");
    assert!(contains_text_starting_with(&stopped, "Autoplay: On"));
    for _ in 0..3 {
        let stopped = app.idle();
        assert!(contains_text_starting_with(&stopped, "Stopped"));
    }
    assert_eq!(AUTOPLAY_WAIT_RUNS.load(Ordering::Relaxed), 1);
    assert_eq!(AUTOPLAY_WAIT_CANCELLED.load(Ordering::Relaxed), 1);

    let _ = app.click("Start");
    let _ = app.wait_for_status("Running");
    let _ = app.click("Stop");
    let _ = app.wait_for_status("Stopped");
    assert_eq!(AUTOPLAY_WAIT_RUNS.load(Ordering::Relaxed), 2);
    assert_eq!(AUTOPLAY_WAIT_CANCELLED.load(Ordering::Relaxed), 2);

    let disabled = app.click("Autoplay: On");
    assert!(contains_text_starting_with(&disabled, "Stopped"));
    let _ = app.click("Autoplay: Off");
    let _ = app.wait_for_status("Running");
    let _ = app.click("Stop");
    let _ = app.wait_for_status("Stopped");
    assert_eq!(AUTOPLAY_WAIT_PREPARATIONS.load(Ordering::Relaxed), 3);
    assert_eq!(AUTOPLAY_WAIT_RUNS.load(Ordering::Relaxed), 3);
    assert_eq!(AUTOPLAY_WAIT_CANCELLED.load(Ordering::Relaxed), 3);
}

fn waiting_stream(_: &mut Params) -> PlanResult {
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

fn polling_stream(_: &mut Params) -> PlanResult {
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
    assert!(!contains_text_starting_with(&output, "Streaming"));

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

#[derive(Default)]
struct UpdatingParams {
    worker_value: u64,
}

fn updating_controls(params: &mut UpdatingParams) -> SliderGrid<'_> {
    SliderGrid::new(
        1,
        [SliderGroup::new(
            "Worker state",
            [Slider::new(
                "Worker value",
                &mut params.worker_value,
                0..=100,
            )],
        )],
    )
}

fn updating_stream(_: &mut UpdatingParams) -> StreamPlanResult<UpdatingParams> {
    UPDATE_PREPARATIONS.fetch_add(1, Ordering::Relaxed);
    let mut plot = Plotter::new();
    let series = plot.add_points(Vec::new()).id();
    Ok(StreamPlan::new(plot, |stream| {
        UPDATE_RUNS.fetch_add(1, Ordering::Relaxed);
        stream.update_params(|params: &mut UpdatingParams| params.worker_value = 42)?;
        Ok(())
    })
    .bind_points(series, |params| {
        UPDATE_BINDINGS.fetch_add(1, Ordering::Relaxed);
        vec![[0.0, params.worker_value as f64]]
    }))
}

#[test]
fn autoplay_parameter_updates_refresh_controls_and_bound_points_without_preparing_again() {
    use eframe::App as _;

    UPDATE_PREPARATIONS.store(0, Ordering::Relaxed);
    UPDATE_RUNS.store(0, Ordering::Relaxed);
    UPDATE_BINDINGS.store(0, Ordering::Relaxed);
    const VIEWS: &[ViewOption<UpdatingParams>] = &[ViewOption::streaming(
        "Stream",
        updating_stream,
        updating_controls,
    )];
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = App::new(&creation, AppDefinition::new("Test", VIEWS));
    let mut frame = eframe::Frame::_new_kittest();
    let mut frame_time = 0.0;

    let _ = context.run(input(frame_time), |context| app.update(context, &mut frame));
    frame_time += 0.01;
    let ready = context.run(input(frame_time), |context| app.update(context, &mut frame));
    assert!(contains_text_starting_with(&ready, "Worker value"));
    assert!(contains_text_starting_with(&ready, "0"));
    let stream_heading = text_position(&ready, "Streaming");
    let worker_heading = text_position(&ready, "Worker state");
    let start = text_position(&ready, "Start");
    let autoplay = text_position(&ready, "Autoplay: Off");
    assert!(stream_heading.y > worker_heading.y);
    assert!(start.x > stream_heading.x);
    assert!((start.y - stream_heading.y).abs() < 8.0);
    assert!(autoplay.x > start.x);
    assert!((autoplay.y - start.y).abs() < 8.0);
    assert_eq!(UPDATE_PREPARATIONS.load(Ordering::Relaxed), 1);

    frame_time += 0.01;
    let _ = context.run(pointer_input(frame_time, autoplay, true), |context| {
        app.update(context, &mut frame)
    });
    frame_time += 0.01;
    let _ = context.run(pointer_input(frame_time, autoplay, false), |context| {
        app.update(context, &mut frame)
    });

    let deadline = Instant::now() + Duration::from_secs(1);
    let mut updated = false;
    while Instant::now() < deadline {
        frame_time += 0.01;
        let output = context.run(input(frame_time), |context| app.update(context, &mut frame));
        updated = contains_text_starting_with(&output, "42")
            && contains_text_starting_with(&output, "Finished")
            && contains_text_starting_with(&output, "Autoplay: On");
        if updated {
            break;
        }
        std::thread::yield_now();
    }

    assert!(updated);
    assert_eq!(UPDATE_RUNS.load(Ordering::Relaxed), 1);
    assert_eq!(UPDATE_BINDINGS.load(Ordering::Relaxed), 1);
    assert_eq!(UPDATE_PREPARATIONS.load(Ordering::Relaxed), 1);
}
