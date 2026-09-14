use crate::{Plotter, SliderGrid};
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

#[cfg(target_arch = "wasm32")]
pub type WebResult<T = ()> = std::result::Result<T, wasm_bindgen::JsValue>;

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
    #[cfg(not(target_arch = "wasm32"))]
    title: &'static str,
    #[cfg(target_arch = "wasm32")]
    canvas_id: &'static str,
    views: &'static [ViewOption<P>],
}

impl<P> AppDefinition<P> {
    pub const fn new(
        _title: &'static str,
        _canvas_id: &'static str,
        views: &'static [ViewOption<P>],
    ) -> Self {
        assert!(!views.is_empty(), "an app must have at least one view");
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            title: _title,
            #[cfg(target_arch = "wasm32")]
            canvas_id: _canvas_id,
            views,
        }
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
        for (index, key) in (0..options.len()).zip(VIEW_KEYS) {
            let shortcut = egui::KeyboardShortcut::new(egui::Modifiers::NONE, key);

            if ui.input_mut(|input| input.consume_shortcut(&shortcut)) {
                changed |= *selected != index;
                *selected = index;
                break;
            }
        }
    }

    ui.heading("View: ");
    ui.horizontal(|ui| {
        for (index, option) in options.iter().enumerate() {
            let label = if index < VIEW_KEYS.len() {
                format!("[{}] {}", index + 1, option.title)
            } else {
                option.title.to_owned()
            };
            changed |= ui.selectable_value(selected, index, label).changed();
        }
    });

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

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn navigate_view(&mut self, step: isize) {
        let selected = self
            .selected_view
            .saturating_add_signed(step)
            .min(self.definition.views.len() - 1);
        if selected != self.selected_view {
            self.selected_view = selected;
            self.cached_plotter = None;
            self.compute_time = None;
        }
    }

    fn draw_header(&mut self, ui: &mut Ui) -> (bool, bool) {
        let mut changed = false;
        let mut reset_requested = false;

        ui.horizontal(|ui| {
            changed |= view_selector(&mut self.selected_view, self.definition.views, ui);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                reset_requested = Self::reset_button(ui);
                if let Some(compute_time) = self.compute_time {
                    let milliseconds = compute_time.as_secs_f64() * 1_000.0;
                    ui.label(format!("Compute: {milliseconds:.3} ms"));
                }
            });
        });

        (changed, reset_requested)
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
}

impl<P> App<P>
where
    P: Default + 'static,
{
    pub(crate) fn show_ui(&mut self, ui: &mut Ui) {
        egui::ScrollArea::both().show(ui, |ui| {
            let (view_changed, reset_requested) = self.draw_header(ui);
            let mut changed = view_changed;
            if reset_requested {
                self.reset(ui);
                changed = true;
            }

            ui.separator();

            changed |= self.draw_controls(ui);
            if changed || self.cached_plotter.is_none() {
                self.recompute_plot();
                ui.ctx().request_repaint();
            }

            ui.separator();

            self.draw_plot(ui);
        });
    }
}

impl<P> eframe::App for App<P>
where
    P: Default + 'static,
{
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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

#[cfg(target_arch = "wasm32")]
pub fn run_web<P>(definition: AppDefinition<P>) -> WebResult
where
    P: Default + 'static,
{
    use wasm_bindgen::JsCast as _;

    let window =
        web_sys::window().ok_or_else(|| wasm_bindgen::JsValue::from_str("missing window"))?;
    let document = window
        .document()
        .ok_or_else(|| wasm_bindgen::JsValue::from_str("missing document"))?;
    let canvas = document
        .get_element_by_id(definition.canvas_id)
        .ok_or_else(|| {
            wasm_bindgen::JsValue::from_str(&format!("missing #{}", definition.canvas_id))
        })?
        .dyn_into::<web_sys::HtmlCanvasElement>()?;

    let runner = Box::leak(Box::new(eframe::WebRunner::new()));
    wasm_bindgen_futures::spawn_local(async move {
        runner
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|creation_context| Ok(Box::new(App::new(creation_context, definition)))),
            )
            .await
            .expect("failed to start eframe");
    });

    Ok(())
}
