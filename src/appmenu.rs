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

    fn content(&mut self) -> &mut dyn MenuContent {
        self.instance
            .get_or_insert_with(|| self.factory.take().expect("missing menu entry factory")())
            .as_mut()
    }

    fn show_ui(&mut self, ui: &mut egui::Ui) {
        self.content().show_ui(ui);
    }
}

/// A native window with a selector for independently configured apps and figures.
/// Entries are created on first selection and retained when switching away.
/// Unmodified up/down arrows select entries. Apps handle their own view navigation.
/// Focused widgets retain their keyboard input.
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

    fn navigate(&mut self, ctx: &egui::Context) -> bool {
        if self.entries.is_empty() || ctx.wants_keyboard_input() {
            return false;
        }

        let keys = ctx.input_mut(|input| {
            let mut keys = Vec::new();
            input.events.retain(|event| {
                if let egui::Event::Key {
                    key: key @ (egui::Key::ArrowUp | egui::Key::ArrowDown),
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                    ..
                } = event
                {
                    keys.push(*key);
                    false
                } else {
                    true
                }
            });
            keys
        });
        if !keys.is_empty() {
            // egui schedules directional focus movement before these events are consumed.
            ctx.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
        }

        let previous = self.selected;
        for key in keys {
            match key {
                egui::Key::ArrowUp => self.selected = self.selected.saturating_sub(1),
                egui::Key::ArrowDown => {
                    self.selected = (self.selected + 1).min(self.entries.len() - 1);
                }
                _ => unreachable!(),
            }
        }
        self.selected != previous
    }

    fn draw(&mut self, ctx: &egui::Context) {
        let selection_changed = self.navigate(ctx);
        egui::SidePanel::left("app-menu-selector")
            .default_width(200.0)
            .show(ctx, |ui| {
                ui.heading(&self.title);
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (index, entry) in self.entries.iter().enumerate() {
                        let response = ui.selectable_value(&mut self.selected, index, &entry.label);
                        if selection_changed && index == self.selected {
                            response.scroll_to_me(None);
                        }
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
        draw_events(menu, ctx, Vec::new());
    }

    fn draw_events(menu: &mut AppMenu, ctx: &egui::Context, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 700.0),
            )),
            events,
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| menu.draw(ctx));
        assert!(!output.shapes.is_empty());
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

    #[test]
    fn arrows_select_entries_stop_at_ends_and_handle_empty_menus() {
        let ctx = egui::Context::default();
        let mut menu = AppMenu::new("Empty");
        for arrow in [
            egui::Key::ArrowUp,
            egui::Key::ArrowDown,
            egui::Key::ArrowLeft,
            egui::Key::ArrowRight,
        ] {
            draw_events(&mut menu, &ctx, key(arrow));
            assert_eq!(menu.selected, 0);
        }

        menu = menu
            .figure("First", || Figure::subplots(1, 1))
            .figure("Second", || Figure::subplots(1, 1));
        for (arrow, selected) in [
            (egui::Key::ArrowUp, 0),
            (egui::Key::ArrowDown, 1),
            (egui::Key::ArrowDown, 1),
            (egui::Key::ArrowLeft, 1),
            (egui::Key::ArrowRight, 1),
            (egui::Key::ArrowUp, 0),
        ] {
            draw_events(&mut menu, &ctx, key(arrow));
            assert_eq!(menu.selected, selected);
            assert!(!ctx.wants_keyboard_input());
        }
    }

    #[test]
    fn view_arrows_refresh_only_on_changes_and_preserve_each_apps_view() {
        use crate::{AppResult, SliderGrid, ViewOption};

        thread_local! {
            static COMPUTED_VIEWS: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
        }
        fn first(_: &mut ()) -> AppResult {
            COMPUTED_VIEWS.with_borrow_mut(|views| views.push(0));
            Err("first view".into())
        }
        fn second(_: &mut ()) -> AppResult {
            COMPUTED_VIEWS.with_borrow_mut(|views| views.push(1));
            Err("second view".into())
        }
        fn controls(_: &mut ()) -> SliderGrid<'_> {
            SliderGrid::new(1, [])
        }
        const VIEWS: &[ViewOption<()>] = &[
            ViewOption::new("First", first, controls),
            ViewOption::new("Second", second, controls),
        ];
        let mut menu = AppMenu::new("Apps")
            .app("One", AppDefinition::new("One", VIEWS))
            .app("Two", AppDefinition::new("Two", VIEWS));
        let ctx = egui::Context::default();
        // Navigation also works before the first lazy instance has been drawn.
        draw_events(&mut menu, &ctx, key(egui::Key::ArrowRight));
        COMPUTED_VIEWS.with_borrow(|views| assert_eq!(views, &[1]));
        draw_events(&mut menu, &ctx, key(egui::Key::ArrowRight));
        draw_events(&mut menu, &ctx, key(egui::Key::ArrowDown));
        draw_events(&mut menu, &ctx, key(egui::Key::ArrowLeft));
        draw_events(&mut menu, &ctx, key(egui::Key::ArrowUp));
        COMPUTED_VIEWS.with_borrow(|views| assert_eq!(views, &[1, 0]));
        draw_events(&mut menu, &ctx, key(egui::Key::ArrowLeft));
        draw_events(&mut menu, &ctx, key(egui::Key::ArrowLeft));
        COMPUTED_VIEWS.with_borrow(|views| assert_eq!(views, &[1, 0, 0]));
        draw_events(&mut menu, &ctx, key(egui::Key::Num2));
        COMPUTED_VIEWS.with_borrow(|views| assert_eq!(views, &[1, 0, 0, 1]));
    }

    #[test]
    fn focused_editor_keeps_arrow_input() {
        struct Editor(Rc<RefCell<String>>);
        impl MenuContent for Editor {
            fn show_ui(&mut self, ui: &mut egui::Ui) {
                ui.text_edit_singleline(&mut *self.0.borrow_mut())
                    .request_focus();
            }
        }
        let text = Rc::new(RefCell::new(String::from("abc")));
        let editor_text = text.clone();
        let mut menu = AppMenu::new("Editor");
        menu.entries.push(MenuEntry::new("Editor".into(), move || {
            Box::new(Editor(editor_text))
        }));
        menu = menu.figure("Figure", || Figure::subplots(1, 1));
        let ctx = egui::Context::default();
        draw(&mut menu, &ctx);
        assert!(ctx.wants_keyboard_input());
        for arrow in [
            egui::Key::ArrowDown,
            egui::Key::ArrowUp,
            egui::Key::ArrowLeft,
            egui::Key::ArrowRight,
        ] {
            draw_events(&mut menu, &ctx, key(arrow));
            assert_eq!(menu.selected, 0);
        }
        draw_events(&mut menu, &ctx, key(egui::Key::Home));
        draw_events(&mut menu, &ctx, key(egui::Key::ArrowRight));
        draw_events(&mut menu, &ctx, vec![egui::Event::Text("X".into())]);
        assert_eq!(&*text.borrow(), "aXbc");
    }

    #[test]
    fn modified_arrows_are_not_consumed() {
        let mut menu = AppMenu::new("Figures")
            .figure("First", || Figure::subplots(1, 1))
            .figure("Second", || Figure::subplots(1, 1));
        let ctx = egui::Context::default();
        for modifiers in [
            egui::Modifiers::SHIFT,
            egui::Modifiers::ALT,
            egui::Modifiers::CTRL,
            egui::Modifiers::COMMAND,
        ] {
            let mut events = key(egui::Key::ArrowDown);
            for event in &mut events {
                if let egui::Event::Key {
                    modifiers: mods, ..
                } = event
                {
                    *mods = modifiers;
                }
            }
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    assert!(!menu.navigate(ctx));
                    assert_eq!(ctx.input(|input| input.events.len()), 2);
                    assert_eq!(menu.selected, 0);
                },
            );
        }
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
