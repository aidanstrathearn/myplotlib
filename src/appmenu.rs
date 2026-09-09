use crate::figure::FigureApp;
use crate::{App, AppDefinition, Figure, NativeResult};
use eframe::egui;

trait MenuContent {
    fn show_ui(&mut self, ui: &mut egui::Ui);
}

impl<P: Default + 'static> MenuContent for App<P> {
    fn show_ui(&mut self, ui: &mut egui::Ui) {
        App::show_ui(self, ui);
    }
}

impl MenuContent for FigureApp {
    fn show_ui(&mut self, ui: &mut egui::Ui) {
        FigureApp::show_ui(self, ui);
    }
}

struct MenuEntry {
    label: String,
    factory: Option<Box<dyn FnOnce() -> Box<dyn MenuContent>>>,
    instance: Option<Box<dyn MenuContent>>,
}

impl MenuEntry {
    fn new(label: String, factory: impl FnOnce() -> Box<dyn MenuContent> + 'static) -> Self {
        Self {
            label,
            factory: Some(Box::new(factory)),
            instance: None,
        }
    }

    fn show_ui(&mut self, ui: &mut egui::Ui) {
        let instance = self
            .instance
            .get_or_insert_with(|| self.factory.take().expect("missing menu entry factory")());
        instance.show_ui(ui);
    }
}

/// A native window with a selector for independently configured apps and figures.
/// Entries are created on first selection and retained when switching away.
pub struct AppMenu {
    title: String,
    entries: Vec<MenuEntry>,
    selected: usize,
}

impl AppMenu {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            entries: Vec::new(),
            selected: 0,
        }
    }

    pub fn app<P: Default + 'static>(
        mut self,
        label: impl Into<String>,
        definition: AppDefinition<P>,
    ) -> Self {
        self.entries.push(MenuEntry::new(label.into(), move || {
            Box::new(App::from_definition(definition))
        }));
        self
    }

    pub fn figure(
        mut self,
        label: impl Into<String>,
        factory: impl FnOnce() -> Figure + 'static,
    ) -> Self {
        self.entries.push(MenuEntry::new(label.into(), move || {
            Box::new(FigureApp::new(factory()))
        }));
        self
    }

    pub fn run_native(self) -> NativeResult {
        let title = self.title.clone();
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1100.0, 700.0])
                .with_min_inner_size([600.0, 400.0])
                .with_resizable(true),
            ..Default::default()
        };
        eframe::run_native(
            &title,
            options,
            Box::new(move |creation_context| {
                creation_context
                    .egui_ctx
                    .set_visuals(egui::Visuals::light());
                Ok(Box::new(self))
            }),
        )
    }

    fn draw(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("app-menu-selector")
            .default_width(200.0)
            .show(ctx, |ui| {
                ui.heading(&self.title);
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (index, entry) in self.entries.iter().enumerate() {
                        ui.selectable_value(&mut self.selected, index, &entry.label);
                    }
                });
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(entry) = self.entries.get_mut(self.selected) {
                ui.push_id(("app-menu-entry", self.selected), |ui| entry.show_ui(ui));
            } else {
                ui.label("No apps or figures have been added.");
            }
        });
    }
}

impl eframe::App for AppMenu {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.draw(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    fn draw(menu: &mut AppMenu, ctx: &egui::Context) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 700.0),
            )),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| menu.draw(ctx));
        assert!(!output.shapes.is_empty());
    }

    #[test]
    fn figures_are_created_only_on_first_selection() {
        let calls = Rc::new(Cell::new(0));
        let mut menu = AppMenu::new("Figures");
        for label in ["First", "Second"] {
            let calls = calls.clone();
            menu = menu.figure(label, move || {
                calls.set(calls.get() + 1);
                Figure::subplots(1, 2)
            });
        }
        assert_eq!(calls.get(), 0);
        let ctx = egui::Context::default();
        draw(&mut menu, &ctx);
        assert_eq!(calls.get(), 1);
        menu.selected = 1;
        draw(&mut menu, &ctx);
        assert_eq!(calls.get(), 2);
        menu.selected = 0;
        draw(&mut menu, &ctx);
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn switching_preserves_instances_and_isolates_widget_memory() {
        struct StatefulContent {
            visits: usize,
            records: Rc<RefCell<Vec<(egui::Id, usize, usize)>>>,
        }
        impl MenuContent for StatefulContent {
            fn show_ui(&mut self, ui: &mut egui::Ui) {
                self.visits += 1;
                let id = ui.make_persistent_id("same-widget-name");
                let remembered = ui.data_mut(|data| {
                    let value = data.get_temp_mut_or_default::<usize>(id);
                    *value += 1;
                    *value
                });
                self.records
                    .borrow_mut()
                    .push((id, self.visits, remembered));
            }
        }
        let records = Rc::new(RefCell::new(Vec::new()));
        let mut menu = AppMenu::new("State");
        for label in ["First", "Second"] {
            let records = records.clone();
            menu.entries.push(MenuEntry::new(label.into(), move || {
                Box::new(StatefulContent { visits: 0, records })
            }));
        }
        let ctx = egui::Context::default();
        for selected in [0, 1, 0] {
            menu.selected = selected;
            draw(&mut menu, &ctx);
        }
        let records = records.borrow();
        assert_eq!(records.len(), 3);
        assert_ne!(records[0].0, records[1].0);
        assert_eq!(records[0].0, records[2].0);
        assert_eq!((records[0].1, records[0].2), (1, 1));
        assert_eq!((records[1].1, records[1].2), (1, 1));
        assert_eq!((records[2].1, records[2].2), (2, 2));
    }
}
