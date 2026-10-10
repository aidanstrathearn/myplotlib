use crate::data_import::{ImportedSeries, import_dropped_file};
#[cfg(feature = "streaming")]
use crate::stream::{StreamState, StreamStatus};
#[cfg(target_arch = "wasm32")]
use crate::web::{self, WebHandle, WebResult};
use crate::{Plotter, SliderGrid, save};
use eframe::egui;
use eframe::egui::Ui;
use std::time::Duration;
use web_time::Instant;

pub type AppError = Box<dyn std::error::Error + 'static>;
pub type AppResult = std::result::Result<Plotter, AppError>;

pub type PlotAction<P> = fn(&mut P) -> AppResult;
#[cfg(feature = "streaming")]
pub type StreamPlanResult = std::result::Result<crate::StreamPlan, AppError>;
#[cfg(feature = "streaming")]
pub type StreamAction<P> = fn(&mut P) -> StreamPlanResult;
pub type ControlsAction<P> = for<'a> fn(&'a mut P) -> SliderGrid<'a>;

#[cfg(feature = "streaming")]
const STREAM_REPAINT_INTERVAL: Duration = Duration::from_millis(33);

#[cfg(not(target_arch = "wasm32"))]
pub type NativeResult<T = ()> = eframe::Result<T>;

fn timed<T>(compute: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let result = compute();
    (result, start.elapsed())
}

enum ViewAction<P> {
    Immediate(PlotAction<P>),
    #[cfg(feature = "streaming")]
    Streaming(StreamAction<P>),
}

impl<P> Clone for ViewAction<P> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<P> Copy for ViewAction<P> {}

pub struct ViewOption<P> {
    title: &'static str,
    action: ViewAction<P>,
    controls: ControlsAction<P>,
}

impl<P> ViewOption<P> {
    pub const fn new(
        title: &'static str,
        plot: PlotAction<P>,
        controls: ControlsAction<P>,
    ) -> Self {
        Self {
            title,
            action: ViewAction::Immediate(plot),
            controls,
        }
    }

    /// Creates a view whose plot is updated by background computation.
    ///
    /// The initial plot is shown in a ready state. Computation begins only
    /// after the user presses Start.
    #[cfg(feature = "streaming")]
    pub const fn streaming(
        title: &'static str,
        stream: StreamAction<P>,
        controls: ControlsAction<P>,
    ) -> Self {
        Self {
            title,
            action: ViewAction::Streaming(stream),
            controls,
        }
    }
}

pub struct AppDefinition<P: 'static> {
    title: &'static str,
    views: &'static [ViewOption<P>],
}

impl<P> AppDefinition<P> {
    pub const fn new(title: &'static str, views: &'static [ViewOption<P>]) -> Self {
        assert!(!views.is_empty(), "an app must have at least one view");
        Self { title, views }
    }

    pub const fn title(&self) -> &'static str {
        self.title
    }
}

const VIEW_KEYS: [egui::Key; 9] = [
    egui::Key::Num1,
    egui::Key::Num2,
    egui::Key::Num3,
    egui::Key::Num4,
    egui::Key::Num5,
    egui::Key::Num6,
    egui::Key::Num7,
    egui::Key::Num8,
    egui::Key::Num9,
];

fn view_selector<P>(selected: &mut usize, options: &[ViewOption<P>], ui: &mut Ui) -> bool {
    let mut changed = false;

    if !ui.ctx().wants_keyboard_input() {
        let arrows = ui.input_mut(|input| {
            let mut arrows = Vec::new();
            input.events.retain(|event| {
                if let egui::Event::Key {
                    //'@' means check event.key == (ArrowLeft | ArrowRight), then bind to key
                    key: key @ (egui::Key::ArrowLeft | egui::Key::ArrowRight),
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                    ..
                } = event
                {
                    arrows.push(*key);
                    false
                } else {
                    true
                }
            });
            arrows
        });
        if !arrows.is_empty() {
            // egui schedules directional focus movement before these events are consumed.
            ui.ctx()
                .memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
        }
        for arrow in arrows {
            let next = match arrow {
                egui::Key::ArrowLeft => selected.saturating_sub(1),
                egui::Key::ArrowRight => (*selected + 1).min(options.len() - 1),
                _ => unreachable!(),
            };
            changed |= *selected != next;
            *selected = next;
        }

        for (index, key) in (0..options.len()).zip(VIEW_KEYS) {
            let shortcut = egui::KeyboardShortcut::new(egui::Modifiers::NONE, key);

            if ui.input_mut(|input| input.consume_shortcut(&shortcut)) {
                changed |= *selected != index;
                *selected = index;
                break;
            }
        }
    }

    for (index, option) in options.iter().enumerate() {
        let label = if index < VIEW_KEYS.len() {
            format!("[{}] {}", index + 1, option.title)
        } else {
            option.title.to_owned()
        };
        changed |= ui.selectable_value(selected, index, label).changed();
    }

    changed
}

pub struct App<P: 'static> {
    definition: AppDefinition<P>,
    selected_view: usize,
    params: P,
    plot_state: Option<PlotState>,
    compute_time: Option<Duration>,
    view_ui: Vec<ViewUiState>,
    pending_drop_target: Option<(usize, usize)>,
}

enum PlotState {
    Immediate(AppResult),
    #[cfg(feature = "streaming")]
    Streaming(StreamState),
}

impl PlotState {
    fn plotter(&self) -> Option<&Plotter> {
        match self {
            Self::Immediate(Ok(plotter)) => Some(plotter),
            Self::Immediate(Err(_)) => None,
            #[cfg(feature = "streaming")]
            Self::Streaming(state) => Some(&state.plotter),
        }
    }
}

#[derive(Clone, Copy)]
#[cfg_attr(not(feature = "streaming"), allow(dead_code))]
enum StreamCommand {
    Start,
    Stop,
}

#[derive(Default)]
struct AxesUiState {
    imported: Vec<ImportedSeries>,
    import_errors: Vec<String>,
    next_import_id: u64,
}

struct FigureUiState {
    axes: Vec<AxesUiState>,
}

impl FigureUiState {
    fn single_axes() -> Self {
        Self {
            axes: vec![AxesUiState::default()],
        }
    }
}

struct ViewUiState {
    figure: FigureUiState,
}

impl ViewUiState {
    fn single_axes() -> Self {
        Self {
            figure: FigureUiState::single_axes(),
        }
    }
}

fn show_axes_ui(
    ui: &mut Ui,
    plotter: &Plotter,
    state: &mut AxesUiState,
    id: impl std::hash::Hash + Copy,
) -> egui::Rect {
    show_import_status(ui, state, id);
    plotter.show_ui_with_overlays(ui, id, &state.imported)
}

fn show_import_status(ui: &mut Ui, state: &mut AxesUiState, id: impl std::hash::Hash) {
    if state.imported.is_empty() && state.import_errors.is_empty() {
        return;
    }

    let mut remove_import = None;
    let mut remove_error = None;
    ui.scope(|ui| {
        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        egui::ScrollArea::horizontal()
            .id_salt(("plot-import-status", id))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if !state.imported.is_empty() {
                        ui.label(egui::RichText::new("Imported:").strong());
                        for (index, series) in state.imported.iter().enumerate() {
                            if ui
                                .small_button(format!("{}  ×", series.source_name))
                                .on_hover_text(format!("Remove imported series {}", series.label))
                                .clicked()
                            {
                                remove_import = Some(index);
                            }
                        }
                    }

                    for (index, error) in state.import_errors.iter().enumerate() {
                        if !state.imported.is_empty() || index > 0 {
                            ui.separator();
                        }
                        ui.colored_label(ui.visuals().error_fg_color, error);
                        if ui
                            .small_button("×")
                            .on_hover_text("Dismiss error")
                            .clicked()
                        {
                            remove_error = Some(index);
                        }
                    }
                });
            });
    });

    if let Some(index) = remove_import {
        state.imported.remove(index);
    }
    if let Some(index) = remove_error {
        state.import_errors.remove(index);
    }
}

fn handle_file_drop(
    ui: &Ui,
    selected_view: usize,
    pending_target: &mut Option<(usize, usize)>,
    view_ui: &mut [ViewUiState],
    plot_rects: &[egui::Rect],
) {
    let files_hovering = ui.ctx().input(|input| !input.raw.hovered_files.is_empty());
    let only_visible_plot = (plot_rects.len() == 1).then_some((selected_view, 0));
    if files_hovering {
        *pending_target = ui
            .ctx()
            .pointer_latest_pos()
            .and_then(|pointer| {
                plot_rects
                    .iter()
                    .position(|plot_rect| plot_rect.contains(pointer))
            })
            .map(|axes| (selected_view, axes))
            .or(only_visible_plot);

        if let Some((_, axes)) = *pending_target {
            paint_drop_target(ui, plot_rects[axes]);
        }
    }

    let dropped_files = ui
        .ctx()
        .input_mut(|input| std::mem::take(&mut input.raw.dropped_files));
    if dropped_files.is_empty() {
        return;
    }

    let Some((view, axes)) = pending_target.or(only_visible_plot) else {
        return;
    };
    let Some(axes) = view_ui
        .get_mut(view)
        .and_then(|state| state.figure.axes.get_mut(axes))
    else {
        return;
    };

    for file in dropped_files {
        match import_dropped_file(&file) {
            Ok(series) => {
                for mut series in series {
                    series.id = axes.next_import_id;
                    axes.next_import_id += 1;
                    axes.imported.push(series);
                }
            }
            Err(error) => axes.import_errors.push(error),
        }
    }
    ui.ctx().request_repaint();
}

fn paint_drop_target(ui: &Ui, rect: egui::Rect) {
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        4.0,
        egui::Color32::from_rgba_unmultiplied(30, 120, 210, 40),
    );
    painter.rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(2.0, egui::Color32::from_rgb(30, 120, 210)),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "Drop data here",
        egui::TextStyle::Heading.resolve(ui.style()),
        egui::Color32::from_rgb(20, 80, 150),
    );
}

impl<P> App<P>
where
    P: Default + 'static,
{
    pub fn new(
        creation_context: &eframe::CreationContext<'_>,
        definition: AppDefinition<P>,
    ) -> Self {
        creation_context
            .egui_ctx
            .set_visuals(egui::Visuals::light());
        Self::from_definition(definition)
    }

    pub(crate) fn from_definition(definition: AppDefinition<P>) -> Self {
        let view_ui = (0..definition.views.len())
            .map(|_| ViewUiState::single_axes())
            .collect();
        Self {
            definition,
            selected_view: 0,
            params: P::default(),
            plot_state: None,
            compute_time: None,
            view_ui,
            pending_drop_target: None,
        }
    }

    fn selected_option(&self) -> &ViewOption<P> {
        &self.definition.views[self.selected_view]
    }

    fn draw_header(&mut self, ui: &mut Ui) -> (bool, bool, bool) {
        let can_save = self
            .plot_state
            .as_ref()
            .and_then(PlotState::plotter)
            .is_some();
        let save_status_id = Self::save_status_id(ui);
        let (view_changed, (reset_requested, save_requested)) =
            egui::containers::Sides::new().shrink_left().show(
                ui,
                |ui| {
                    ui.heading("View: ");
                    ui.scope(|ui| {
                        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();

                        egui::ScrollArea::horizontal()
                            .id_salt("plot-app-view-options")
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    view_selector(
                                        &mut self.selected_view,
                                        self.definition.views,
                                        ui,
                                    )
                                })
                                .inner
                            })
                            .inner
                    })
                    .inner
                },
                |ui| {
                    let reset_requested = Self::reset_button(ui);
                    let save_requested = Self::save_button(ui, can_save, save_status_id);
                    if let Some(compute_time) = self.compute_time {
                        let milliseconds = compute_time.as_secs_f64() * 1_000.0;
                        ui.label(format!("Compute: {milliseconds:.3} ms"));
                    }
                    if let Some(save::SaveStatus::Error(error)) =
                        save::status(ui.ctx(), save_status_id)
                    {
                        ui.colored_label(ui.visuals().error_fg_color, "Save failed")
                            .on_hover_text(error);
                    }
                    (reset_requested, save_requested)
                },
            );
        (view_changed, reset_requested, save_requested)
    }

    fn save_status_id(ui: &Ui) -> egui::Id {
        ui.make_persistent_id("plot-app-save-status")
    }

    fn reset_button(ui: &mut Ui) -> bool {
        let shortcut = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::R);

        let shortcut_pressed = !ui.ctx().wants_keyboard_input()
            && ui.input_mut(|input| input.consume_shortcut(&shortcut));

        let shortcut_text = ui.ctx().format_shortcut(&shortcut);

        let button_clicked = ui
            .add(egui::Button::new("Reset").shortcut_text(shortcut_text))
            .clicked();

        button_clicked || shortcut_pressed
    }

    fn save_button(ui: &mut Ui, can_save: bool, status_id: egui::Id) -> bool {
        let pending = matches!(
            save::status(ui.ctx(), status_id),
            Some(save::SaveStatus::Pending)
        );
        let enabled = can_save && !pending;
        let shortcut = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::S);
        let shortcut_pressed = enabled
            && !ui.ctx().wants_keyboard_input()
            && ui.input_mut(|input| input.consume_shortcut(&shortcut));
        let shortcut_text = ui.ctx().format_shortcut(&shortcut);
        let button_clicked = ui
            .add_enabled(
                enabled,
                egui::Button::new("Save").shortcut_text(shortcut_text),
            )
            .clicked();

        button_clicked || shortcut_pressed
    }

    fn reset(&mut self, ui: &Ui) {
        // Plot navigation lives in egui's memory, independently of the cached data.
        for view in 0..self.definition.views.len() {
            let id = ui.make_persistent_id(egui::Id::new(("plot-app", view)));
            ui.data_mut(|data| data.remove::<egui_plot::PlotMemory>(id));
        }
        self.selected_view = 0;
        self.params = P::default();
        self.plot_state = None;
        self.compute_time = None;
        self.view_ui = (0..self.definition.views.len())
            .map(|_| ViewUiState::single_axes())
            .collect();
        self.pending_drop_target = None;
    }

    fn draw_controls(&mut self, ui: &mut Ui) -> bool {
        let controls = self.selected_option().controls;
        controls(&mut self.params).show(ui)
    }

    fn draw_control_row(&mut self, ui: &mut Ui) -> (bool, Option<StreamCommand>) {
        #[cfg(feature = "streaming")]
        let stream_info = if matches!(self.selected_option().action, ViewAction::Streaming(_)) {
            Some(match self.plot_state.as_ref() {
                Some(PlotState::Streaming(state)) => (
                    state.status(),
                    state.progress(),
                    state.error().map(str::to_owned),
                ),
                Some(PlotState::Immediate(Err(error))) => {
                    (StreamStatus::Failed, None, Some(error.to_string()))
                }
                _ => (StreamStatus::Ready, None, None),
            })
        } else {
            None
        };

        egui::containers::Sides::new().shrink_left().show(
            ui,
            |ui| {
                ui.scope(|ui| {
                    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();

                    egui::ScrollArea::horizontal()
                        .id_salt(("plot-app-controls", self.selected_view))
                        .auto_shrink([false, true])
                        .show(ui, |ui| self.draw_controls(ui))
                        .inner
                })
                .inner
            },
            |ui| {
                #[cfg(feature = "streaming")]
                {
                    stream_info.and_then(|(status, progress, error)| {
                        Self::draw_stream_controls(ui, status, progress, error.as_deref())
                    })
                }
                #[cfg(not(feature = "streaming"))]
                {
                    let _ = ui;
                    None
                }
            },
        )
    }

    #[cfg(feature = "streaming")]
    fn draw_stream_controls(
        ui: &mut Ui,
        status: StreamStatus,
        progress: Option<f32>,
        error: Option<&str>,
    ) -> Option<StreamCommand> {
        let command = match status {
            StreamStatus::Running => ui.button("Stop").clicked().then_some(StreamCommand::Stop),
            StreamStatus::Stopping => {
                ui.add_enabled(false, egui::Button::new("Stop"));
                None
            }
            StreamStatus::Ready
            | StreamStatus::Stopped
            | StreamStatus::Finished
            | StreamStatus::Failed => ui.button("Start").clicked().then_some(StreamCommand::Start),
        };
        if let Some(progress) = progress {
            ui.add(
                egui::ProgressBar::new(progress)
                    .show_percentage()
                    .desired_width(90.0)
                    .animate(status == StreamStatus::Running),
            );
        }
        if let Some(error) = error {
            ui.colored_label(ui.visuals().error_fg_color, status.label())
                .on_hover_text(error);
        } else {
            ui.label(status.label());
        }
        command
    }

    fn prepare_plot(&mut self) {
        let action = self.selected_option().action;
        self.plot_state = None;
        match action {
            ViewAction::Immediate(plot) => {
                let (result, compute_time) = timed(|| plot(&mut self.params));
                self.plot_state = Some(PlotState::Immediate(result));
                self.compute_time = Some(compute_time);
            }
            #[cfg(feature = "streaming")]
            ViewAction::Streaming(stream) => {
                let (result, compute_time) = timed(|| stream(&mut self.params));
                self.compute_time = Some(compute_time);
                self.plot_state = Some(match result {
                    Ok(plan) => PlotState::Streaming(StreamState::prepare(plan)),
                    Err(error) => PlotState::Immediate(Err(error)),
                });
            }
        }
    }

    fn draw_plot(&mut self, ui: &mut Ui) -> Option<egui::Rect> {
        let selected_view = self.selected_view;
        match &self.plot_state {
            Some(PlotState::Immediate(Ok(plotter))) => {
                let axes = &mut self.view_ui[selected_view].figure.axes[0];
                Some(show_axes_ui(ui, plotter, axes, ("plot-app", selected_view)))
            }
            Some(PlotState::Immediate(Err(error))) => {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                None
            }
            #[cfg(feature = "streaming")]
            Some(PlotState::Streaming(state)) => {
                let axes = &mut self.view_ui[selected_view].figure.axes[0];
                Some(show_axes_ui(
                    ui,
                    &state.plotter,
                    axes,
                    ("plot-app", selected_view),
                ))
            }
            None => None,
        }
    }

    fn screenshot_filename(&self) -> String {
        let title = filename_component(self.definition.title);
        let view = filename_component(self.selected_option().title);
        match (title.is_empty(), view.is_empty()) {
            (false, false) if title != view => format!("{title}-{view}.png"),
            (false, _) => format!("{title}.png"),
            (_, false) => format!("{view}.png"),
            _ => "plot.png".to_owned(),
        }
    }

    #[cfg(feature = "streaming")]
    fn maintain_stream(&mut self, _context: &egui::Context) {
        if let Some(PlotState::Streaming(state)) = &mut self.plot_state {
            let previous_status = state.status();
            state.drain();
            if previous_status != StreamStatus::Finished && state.status() == StreamStatus::Finished
            {
                self.compute_time = Some(state.elapsed());
            }
        }
    }

    #[cfg(not(feature = "streaming"))]
    fn maintain_stream(&mut self, _context: &egui::Context) {}

    #[cfg(feature = "streaming")]
    fn start_stream(&mut self) {
        let is_ready = matches!(
            self.plot_state.as_ref(),
            Some(PlotState::Streaming(state)) if state.status() == StreamStatus::Ready
        );
        if !is_ready {
            self.prepare_plot();
        }
        if let Some(PlotState::Streaming(state)) = &mut self.plot_state {
            state.start();
        }
    }

    #[cfg(feature = "streaming")]
    fn stop_stream(&mut self) {
        if let Some(PlotState::Streaming(state)) = &mut self.plot_state {
            state.stop();
        }
    }

    #[cfg(feature = "streaming")]
    fn schedule_stream_frame(&self, context: &egui::Context) {
        let live_stream = self.plot_state.as_ref().is_some_and(|state| match state {
            PlotState::Streaming(state) => state.needs_repaint_poll(),
            PlotState::Immediate(_) => false,
        });
        if live_stream {
            context.request_repaint_after(STREAM_REPAINT_INTERVAL);
        }
    }
}

fn filename_component(text: &str) -> String {
    let mut component = String::new();
    let mut separated = false;

    for character in text.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            component.push(character);
            separated = false;
        } else if !component.is_empty() && !separated {
            component.push('-');
            separated = true;
        }
    }

    if component.ends_with('-') {
        component.pop();
    }
    component
}

impl<P> App<P>
where
    P: Default + 'static,
{
    pub(crate) fn show_ui(&mut self, ui: &mut Ui) {
        self.maintain_stream(ui.ctx());
        let (view_changed, reset_requested, save_requested) = self.draw_header(ui);
        if reset_requested {
            self.reset(ui);
        }

        ui.separator();

        // Everything below the app header belongs in the saved image: controls,
        // plot titles and legends, the plot itself, and its summary panel.
        let capture_rect = ui.available_rect_before_wrap();

        let (controls_changed, stream_command) = self.draw_control_row(ui);

        let needs_preparation =
            view_changed || reset_requested || controls_changed || self.plot_state.is_none();
        if needs_preparation {
            self.prepare_plot();
            ui.ctx().request_repaint();
        }

        if matches!(stream_command, Some(StreamCommand::Start)) {
            #[cfg(feature = "streaming")]
            self.start_stream();
            ui.ctx().request_repaint();
        } else if matches!(stream_command, Some(StreamCommand::Stop)) {
            #[cfg(feature = "streaming")]
            self.stop_stream();
            ui.ctx().request_repaint();
        }

        ui.separator();

        let plot_rect = self.draw_plot(ui);
        handle_file_drop(
            ui,
            self.selected_view,
            &mut self.pending_drop_target,
            &mut self.view_ui,
            plot_rect.as_slice(),
        );

        if save_requested
            && self
                .plot_state
                .as_ref()
                .and_then(PlotState::plotter)
                .is_some()
        {
            save::request(
                ui.ctx(),
                capture_rect,
                self.screenshot_filename(),
                Self::save_status_id(ui),
            );
        }

        #[cfg(feature = "streaming")]
        self.schedule_stream_frame(ui.ctx());
    }
}

impl<P> eframe::App for App<P>
where
    P: Default + 'static,
{
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        save::handle_screenshot_events(ctx);
        egui::CentralPanel::default().show(ctx, |ui| self.show_ui(ui));
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_native<P>(definition: AppDefinition<P>) -> NativeResult
where
    P: Default + 'static,
{
    let title = definition.title;
    let mut native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([400.0, 300.0])
            .with_resizable(true),
        ..Default::default()
    };

    prefer_linux_x11_for_file_drop(&mut native_options);

    eframe::run_native(
        title,
        native_options,
        Box::new(|creation_context| Ok(Box::new(App::new(creation_context, definition)))),
    )
}

#[cfg(target_os = "linux")]
fn prefer_linux_x11_for_file_drop(native_options: &mut eframe::NativeOptions) {
    use winit::platform::x11::EventLoopBuilderExtX11 as _;

    // winit 0.30 does not emit file drag-and-drop events on Wayland. Prefer
    // X11 (including XWayland) when it is available so imports work on Linux.
    if std::env::var_os("DISPLAY").is_some_and(|display| !display.is_empty()) {
        native_options.event_loop_builder = Some(Box::new(|builder| {
            builder.with_x11();
        }));
    }
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "linux")))]
fn prefer_linux_x11_for_file_drop(_native_options: &mut eframe::NativeOptions) {}

/// Mount a new application instance into a canvas supplied by the host page.
///
/// Each call creates independent application state. Retain the returned handle
/// so that [`WebHandle::destroy`] can be called when the canvas is unmounted.
/// Dropping the handle does not destroy the runner because JavaScript garbage
/// collection is nondeterministic.
///
/// This function resolves only after eframe has started successfully and
/// returns any startup error to the caller.
#[cfg(target_arch = "wasm32")]
pub async fn mount_web<P>(
    canvas: web_sys::HtmlCanvasElement,
    definition: AppDefinition<P>,
) -> WebResult<WebHandle>
where
    P: Default + 'static,
{
    web::mount(canvas, App::from_definition(definition)).await
}
