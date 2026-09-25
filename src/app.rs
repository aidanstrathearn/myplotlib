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
pub type ControlsAction<P> = for<'a> fn(&'a mut P) -> SliderGrid<'a>;

#[cfg(not(target_arch = "wasm32"))]
pub type NativeResult<T = ()> = eframe::Result<T>;

fn timed<T>(compute: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let result = compute();
    (result, start.elapsed())
}

pub struct ViewOption<P> {
    title: &'static str,
    plot: PlotAction<P>,
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
            plot,
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
    cached_plotter: Option<AppResult>,
    compute_time: Option<Duration>,
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
        Self {
            definition,
            selected_view: 0,
            params: P::default(),
            cached_plotter: None,
            compute_time: None,
        }
    }

    fn selected_option(&self) -> &ViewOption<P> {
        &self.definition.views[self.selected_view]
    }

    fn draw_header(&mut self, ui: &mut Ui) -> (bool, bool, bool) {
        let can_save = matches!(self.cached_plotter, Some(Ok(_)));
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
        self.cached_plotter = None;
        self.compute_time = None;
    }

    fn draw_controls(&mut self, ui: &mut Ui) -> bool {
        let controls = self.selected_option().controls;
        controls(&mut self.params).show(ui)
    }

    fn recompute_plot(&mut self) {
        let plot = self.selected_option().plot;
        let (result, compute_time) = timed(|| plot(&mut self.params));
        self.cached_plotter = Some(result);
        self.compute_time = Some(compute_time);
    }

    fn draw_plot(&self, ui: &mut Ui) {
        match &self.cached_plotter {
            Some(Ok(plotter)) => {
                plotter.show_ui(ui, ("plot-app", self.selected_view));
            }
            Some(Err(error)) => {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            }
            None => {}
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
        let (view_changed, reset_requested, save_requested) = self.draw_header(ui);
        let mut changed = view_changed;
        if reset_requested {
            self.reset(ui);
            changed = true;
        }

        ui.separator();

        // Everything below the app header belongs in the saved image: controls,
        // plot titles and legends, the plot itself, and its summary panel.
        let capture_rect = ui.available_rect_before_wrap();

        changed |= ui
            .scope(|ui| {
                ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();

                egui::ScrollArea::horizontal()
                    .id_salt(("plot-app-controls", self.selected_view))
                    .auto_shrink([false, true])
                    .show(ui, |ui| self.draw_controls(ui))
                    .inner
            })
            .inner;
        if changed || self.cached_plotter.is_none() {
            self.recompute_plot();
            ui.ctx().request_repaint();
        }

        ui.separator();

        self.draw_plot(ui);

        if save_requested && matches!(self.cached_plotter, Some(Ok(_))) {
            save::request(
                ui.ctx(),
                capture_rect,
                self.screenshot_filename(),
                Self::save_status_id(ui),
            );
        }
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
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([400.0, 300.0])
            .with_resizable(true),
        ..Default::default()
    };

    eframe::run_native(
        title,
        native_options,
        Box::new(|creation_context| Ok(Box::new(App::new(creation_context, definition)))),
    )
}

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
