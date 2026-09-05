use crate::Points;
use eframe::egui;
use egui_plot::{HLine, Legend, Line, LineStyle, Plot, PlotPoints, VLine};

const MATPLOTLIB_COLORS: [egui::Color32; 10] = [
    egui::Color32::from_rgb(31, 119, 180),
    egui::Color32::from_rgb(255, 127, 14),
    egui::Color32::from_rgb(44, 160, 44),
    egui::Color32::from_rgb(214, 39, 40),
    egui::Color32::from_rgb(148, 103, 189),
    egui::Color32::from_rgb(140, 86, 75),
    egui::Color32::from_rgb(227, 119, 194),
    egui::Color32::from_rgb(127, 127, 127),
    egui::Color32::from_rgb(188, 189, 34),
    egui::Color32::from_rgb(23, 190, 207),
];

pub struct PlotLine {
    points: Points,
    label: Option<String>,
}

impl PlotLine {
    pub fn label(&mut self, label: impl Into<String>) -> &mut Self {
        self.label = Some(label.into());
        self
    }
}

pub struct ReferenceLine {
    value: f64,
    label: Option<String>,
}

impl ReferenceLine {
    pub fn label(&mut self, label: impl Into<String>) -> &mut Self {
        self.label = Some(label.into());
        self
    }
}

#[derive(Default)]
pub struct Plotter {
    series: Vec<PlotLine>,
    horizontal_lines: Vec<ReferenceLine>,
    vertical_lines: Vec<ReferenceLine>,
    x_label: String,
    y_label: String,
    pub(crate) title: String,
    x_limits: Option<(f64, f64)>,
    x_limits_pending: std::cell::Cell<bool>,
}

impl Plotter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn plot(&mut self, x: &[f64], y: &[f64]) -> &mut PlotLine {
        assert_eq!(
            x.len(),
            y.len(),
            "x and y must contain the same number of values"
        );

        let points = x.iter().zip(y.iter()).map(|(&x, &y)| [x, y]).collect();

        self.series.push(PlotLine {
            points,
            label: None,
        });

        self.series.last_mut().unwrap()
    }

    pub fn add_points(&mut self, points: Points) -> &mut PlotLine {
        self.series.push(PlotLine {
            points,
            label: None,
        });

        self.series.last_mut().unwrap()
    }

    /// Sets the initial and reset x bounds, while allowing subsequent pan and zoom.
    pub fn xlim(&mut self, lower: f64, upper: f64) {
        self.x_limits = Some((lower, upper));
        self.x_limits_pending.set(true);
    }

    pub fn axhline(&mut self, y: f64) -> &mut ReferenceLine {
        self.horizontal_lines.push(ReferenceLine {
            value: y,
            label: None,
        });

        self.horizontal_lines.last_mut().unwrap()
    }

    pub fn axvline(&mut self, x: f64) -> &mut ReferenceLine {
        self.vertical_lines.push(ReferenceLine {
            value: x,
            label: None,
        });

        self.vertical_lines.last_mut().unwrap()
    }

    pub fn xlabel(&mut self, label: impl Into<String>) {
        self.x_label = label.into();
    }

    pub fn ylabel(&mut self, label: impl Into<String>) {
        self.y_label = label.into();
    }

    pub fn title(&mut self, title: impl Into<String>) {
        self.title = title.into();
    }

    /// Opens a native window and blocks until it is closed.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn show(self) -> crate::Result {
        crate::Figure::from_plotter(self).show()
    }

    /// Draws into the available UI area. Use a stable, distinct ID for each plot.
    pub fn show_ui(&self, ui: &mut egui::Ui, id: impl std::hash::Hash) -> egui::Rect {
        let axes_rect = ui.available_rect_before_wrap();

        if !self.title.is_empty() {
            ui.vertical_centered(|ui| {
                ui.heading(&self.title);
            });

            ui.add_space(4.0);
        }

        let plot_height = (axes_rect.bottom() - ui.next_widget_position().y).max(1.0);
        let plot = Plot::new(id)
            .legend(Legend::default())
            .x_axis_label(&self.x_label)
            .y_axis_label(&self.y_label)
            .allow_drag(true)
            .allow_scroll(true)
            .allow_zoom(true)
            .allow_boxed_zoom(true)
            .allow_double_click_reset(true)
            .width(axes_rect.width())
            .height(plot_height);

        let response = plot.show(ui, |plot_ui| {
            if let Some((lower, upper)) = self.x_limits {
                // Apply new limits once; ordinary frames preserve mouse navigation.
                if self.x_limits_pending.replace(false)
                    || plot_ui.auto_bounds().x
                    || plot_ui.response().double_clicked()
                {
                    plot_ui.set_plot_bounds_x(lower..=upper);
                }
            }

            for (index, line) in self.series.iter().enumerate() {
                let colour = MATPLOTLIB_COLORS[index % MATPLOTLIB_COLORS.len()];
                let legend_name = line.label.as_deref().unwrap_or_default();
                let line_name = format!("series_{index}");
                let points = line.points.clone();

                let plot_line = Line::new(line_name, PlotPoints::from(points))
                    .name(legend_name)
                    .color(colour)
                    .width(1.9);

                plot_ui.line(plot_line);
            }

            let colour_offset = self.series.len();

            for (index, line) in self.horizontal_lines.iter().enumerate() {
                let colour = MATPLOTLIB_COLORS[(colour_offset + index) % MATPLOTLIB_COLORS.len()];
                let legend_name = line.label.as_deref().unwrap_or_default();

                let plot_line = HLine::new(format!("hline_{index}"), line.value)
                    .name(legend_name)
                    .color(colour)
                    .style(LineStyle::dashed_dense());

                plot_ui.hline(plot_line);
            }

            let colour_offset = colour_offset + self.horizontal_lines.len();

            for (index, line) in self.vertical_lines.iter().enumerate() {
                let colour = MATPLOTLIB_COLORS[(colour_offset + index) % MATPLOTLIB_COLORS.len()];
                let legend_name = line.label.as_deref().unwrap_or_default();

                let plot_line = VLine::new(format!("vline_{index}"), line.value)
                    .name(legend_name)
                    .color(colour)
                    .style(LineStyle::dashed_dense());

                plot_ui.vline(plot_line);
            }
        });

        response.response.rect
    }
}
