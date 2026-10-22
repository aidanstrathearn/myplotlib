use eframe::egui;
use eframe::egui::{RichText, Ui, WidgetText};
use std::ops::RangeInclusive;

pub struct Slider<'a> {
    label: WidgetText,
    widget: egui::Slider<'a>,
}

impl<'a> Slider<'a> {
    pub fn new<Num: egui::emath::Numeric>(
        label: impl Into<WidgetText>,
        value: &'a mut Num,
        range: RangeInclusive<Num>,
    ) -> Self {
        Self {
            label: label.into(),
            widget: egui::Slider::new(value, range),
        }
    }

    pub fn from_get_set(
        label: impl Into<WidgetText>,
        range: RangeInclusive<f64>,
        get_set_value: impl 'a + FnMut(Option<f64>) -> f64,
    ) -> Self {
        Self {
            label: label.into(),
            widget: egui::Slider::from_get_set(range, get_set_value),
        }
    }

    pub fn step_by(mut self, step: f64) -> Self {
        self.widget = self.widget.step_by(step);
        self
    }

    pub fn logarithmic(mut self, logarithmic: bool) -> Self {
        self.widget = self.widget.logarithmic(logarithmic);
        self
    }

    pub fn custom_formatter(
        mut self,
        formatter: impl 'a + Fn(f64, RangeInclusive<usize>) -> String,
    ) -> Self {
        self.widget = self.widget.custom_formatter(formatter);
        self
    }
}

pub struct SliderGroup<'a> {
    title: RichText,
    sliders: Vec<Slider<'a>>,
}

impl<'a> SliderGroup<'a> {
    pub fn new(title: impl Into<RichText>, sliders: impl IntoIterator<Item = Slider<'a>>) -> Self {
        Self {
            title: title.into(),
            sliders: sliders.into_iter().collect(),
        }
    }
}

pub struct SliderGrid<'a> {
    groups: Vec<SliderGroup<'a>>,
    max_rows: usize,
}

impl<'a> SliderGrid<'a> {
    pub fn new(max_rows: usize, groups: impl IntoIterator<Item = SliderGroup<'a>>) -> Self {
        assert!(max_rows > 0, "a slider grid must allow at least one row");
        Self {
            groups: groups.into_iter().collect(),
            max_rows,
        }
    }

    pub fn empty() -> Self {
        Self {
            groups: Vec::new(),
            max_rows: 1,
        }
    }

    pub fn show(self, ui: &mut Ui) -> bool {
        if self.groups.is_empty() {
            return false;
        }

        let mut changed = false;
        let max_rows = self.max_rows;
        let grid_id = ui.id().with("plot-app-slider-grid");

        egui::Grid::new(grid_id).show(ui, |ui| {
            for (group_index, group) in self.groups.into_iter().enumerate() {
                ui.vertical(|ui| {
                    ui.heading(group.title);
                    changed |= show_sliders(ui, grid_id.with(group_index), max_rows, group.sliders);
                });
            }
            ui.end_row();
        });

        changed
    }
}

impl Default for SliderGrid<'_> {
    fn default() -> Self {
        Self::empty()
    }
}

fn show_sliders(ui: &mut Ui, id: egui::Id, max_rows: usize, sliders: Vec<Slider<'_>>) -> bool {
    let slider_count = sliders.len();
    if slider_count == 0 {
        return false;
    }

    let row_count = slider_count.min(max_rows);
    let column_count = slider_count.div_ceil(max_rows);
    let mut sliders: Vec<_> = sliders.into_iter().map(Some).collect();
    let mut changed = false;

    egui::Grid::new(id).show(ui, |ui| {
        for row in 0..row_count {
            for column in 0..column_count {
                let index = column * max_rows + row;
                if let Some(slider) = sliders.get_mut(index).and_then(Option::take) {
                    ui.label(slider.label);
                    changed |= ui.add(slider.widget).changed();
                } else {
                    ui.label("");
                    ui.label("");
                }
            }
            ui.end_row();
        }
    });

    changed
}
