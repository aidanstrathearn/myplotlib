use crate::Points;
use eframe::egui;
use egui_plot::{
    GridInput, GridMark, HLine, Legend, Line, LineStyle, Plot, PlotPoint, PlotPoints, PlotUi, VLine,
};

const DEFAULT_LINE_WIDTH: f32 = 3.0;
const DEFAULT_LABEL_FONT_SIZE: f32 = 24.0;
const MIN_LOG10_EXPONENT: f64 = -323.0;
const MAX_LOG10_EXPONENT: f64 = 308.0;

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

/// Coordinate scaling applied independently to each plot axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AxisScale {
    /// Values are plotted without transformation.
    #[default]
    Linear,
    /// Positive values are plotted by their base-10 logarithm.
    Log10,
}

impl AxisScale {
    fn forward(self, value: f64) -> Option<f64> {
        match self {
            Self::Linear => Some(value),
            Self::Log10 if value.is_finite() && value > 0.0 => Some(value.log10()),
            Self::Log10 => None,
        }
    }

    fn inverse(self, value: f64) -> f64 {
        match self {
            Self::Linear => value,
            Self::Log10 => 10.0_f64.powf(value),
        }
    }

    fn transform_required(self, value: f64, description: &str) -> f64 {
        self.forward(value).unwrap_or_else(|| {
            panic!("{description} must be finite and greater than zero on a log10 axis")
        })
    }

    fn is_logarithmic(self) -> bool {
        self == Self::Log10
    }
}

fn transform_line(points: &Points, x_scale: AxisScale, y_scale: AxisScale) -> Vec<Points> {
    if !x_scale.is_logarithmic() && !y_scale.is_logarithmic() {
        return vec![points.clone()];
    }

    let mut segments = Vec::new();
    let mut current_segment = Vec::new();

    for &[x, y] in points {
        let transformed = x_scale
            .forward(x)
            .zip(y_scale.forward(y))
            .map(|(x, y)| [x, y])
            .filter(|[x, y]| x.is_finite() && y.is_finite());

        if let Some(point) = transformed {
            current_segment.push(point);
        } else if !current_segment.is_empty() {
            segments.push(std::mem::take(&mut current_segment));
        }
    }

    if !current_segment.is_empty() {
        segments.push(current_segment);
    }

    segments
}

fn log10_grid_marks(input: GridInput) -> Vec<GridMark> {
    let (lower, upper) = input.bounds;
    if !lower.is_finite() || !upper.is_finite() || lower >= upper {
        return Vec::new();
    }

    let first_decade = lower.floor().max(MIN_LOG10_EXPONENT) as i32;
    let last_decade = upper.ceil().min(MAX_LOG10_EXPONENT) as i32;
    if first_decade > last_decade {
        return Vec::new();
    }

    let major_stride = input.base_step_size.ceil().max(1.0) as i32;
    let medium_stride = major_stride * 5;
    let coarse_stride = major_stride * 10;
    let first_major = first_decade.div_euclid(major_stride) * major_stride;
    let mut marks = Vec::new();

    for exponent in (first_major..=last_decade).step_by(major_stride as usize) {
        let value = exponent as f64;
        if (lower..upper).contains(&value) {
            let step_size = if exponent.rem_euclid(coarse_stride) == 0 {
                coarse_stride
            } else if exponent.rem_euclid(medium_stride) == 0 {
                medium_stride
            } else {
                major_stride
            };
            marks.push(GridMark {
                value,
                step_size: f64::from(step_size),
            });
        }
    }

    // Only show the conventional 2..9 minor marks when their narrowest gap is visible.
    let minor_step_size = (10.0_f64 / 9.0).log10();
    if major_stride == 1 && input.base_step_size <= minor_step_size {
        for exponent in first_decade..last_decade {
            for multiplier in 2..10 {
                let value = exponent as f64 + (multiplier as f64).log10();
                if (lower..upper).contains(&value) {
                    marks.push(GridMark {
                        value,
                        step_size: minor_step_size,
                    });
                }
            }
        }
    }

    marks.sort_by(|left, right| left.value.total_cmp(&right.value));
    marks
}

fn trim_decimal(mut value: String) -> String {
    if value.contains('.') {
        while value.ends_with('0') {
            value.pop();
        }
        if value.ends_with('.') {
            value.pop();
        }
    }
    value
}

fn format_value(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }

    let absolute = value.abs();
    if !(0.001..10_000.0).contains(&absolute) {
        format!("{value:.3e}")
    } else {
        trim_decimal(format!("{value:.6}"))
    }
}

fn format_log10_tick(mark: GridMark, _range: &std::ops::RangeInclusive<f64>) -> String {
    let mut exponent = mark.value.floor() as i32;
    let mut mantissa = 10.0_f64.powf(mark.value - f64::from(exponent)).round() as i32;

    // Floating-point error can put a decade tick just below its integer exponent.
    if mantissa == 10 {
        mantissa = 1;
        exponent += 1;
    }

    format!("{mantissa}e{exponent}")
}

fn format_hover_label(
    name: &str,
    point: &PlotPoint,
    x_scale: AxisScale,
    y_scale: AxisScale,
) -> String {
    let prefix = if name.is_empty() {
        String::new()
    } else {
        format!("{name}\n")
    };
    let x = format_value(x_scale.inverse(point.x));
    let y = format_value(y_scale.inverse(point.y));
    format!("{prefix}x = {x}\ny = {y}")
}

struct RenderData {
    series_segments: Vec<Vec<Points>>,
    x_limits: Option<(f64, f64)>,
    y_limits: Option<(f64, f64)>,
    horizontal_lines: Vec<f64>,
    vertical_lines: Vec<f64>,
}

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
    y_limits: Option<(f64, f64)>,
    y_limits_pending: std::cell::Cell<bool>,
    x_scale: AxisScale,
    y_scale: AxisScale,
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

    /// Sets the initial and reset y bounds, while allowing subsequent pan and zoom.
    pub fn ylim(&mut self, lower: f64, upper: f64) {
        self.y_limits = Some((lower, upper));
        self.y_limits_pending.set(true);
    }

    /// Sets the scaling used by the x axis.
    pub fn xscale(&mut self, scale: AxisScale) {
        self.x_scale = scale;
    }

    /// Sets the scaling used by the y axis.
    pub fn yscale(&mut self, scale: AxisScale) {
        self.y_scale = scale;
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

    fn transform_for_rendering(&self) -> RenderData {
        let series_segments = self
            .series
            .iter()
            .map(|line| transform_line(&line.points, self.x_scale, self.y_scale))
            .collect();
        let x_limits = self.x_limits.map(|(lower, upper)| {
            (
                self.x_scale.transform_required(lower, "x limits"),
                self.x_scale.transform_required(upper, "x limits"),
            )
        });
        let y_limits = self.y_limits.map(|(lower, upper)| {
            (
                self.y_scale.transform_required(lower, "y limits"),
                self.y_scale.transform_required(upper, "y limits"),
            )
        });
        let horizontal_lines = self
            .horizontal_lines
            .iter()
            .map(|line| {
                self.y_scale
                    .transform_required(line.value, "horizontal reference line")
            })
            .collect();
        let vertical_lines = self
            .vertical_lines
            .iter()
            .map(|line| {
                self.x_scale
                    .transform_required(line.value, "vertical reference line")
            })
            .collect();

        RenderData {
            series_segments,
            x_limits,
            y_limits,
            horizontal_lines,
            vertical_lines,
        }
    }

    fn apply_bounds(
        &self,
        plot_ui: &mut PlotUi<'_>,
        x_limits: Option<(f64, f64)>,
        y_limits: Option<(f64, f64)>,
        changed_scales: [bool; 2],
    ) {
        if changed_scales[0] || changed_scales[1] {
            let mut auto_bounds = plot_ui.auto_bounds();
            auto_bounds.x |= changed_scales[0];
            auto_bounds.y |= changed_scales[1];
            plot_ui.set_auto_bounds(auto_bounds);
        }

        if let Some((lower, upper)) = x_limits {
            // Apply new limits once; ordinary frames preserve mouse navigation.
            if self.x_limits_pending.replace(false)
                || plot_ui.auto_bounds().x
                || plot_ui.response().double_clicked()
                || changed_scales[0]
            {
                plot_ui.set_plot_bounds_x(lower..=upper);
            }
        }

        if let Some((lower, upper)) = y_limits {
            // Apply new limits once; ordinary frames preserve mouse navigation.
            if self.y_limits_pending.replace(false)
                || plot_ui.auto_bounds().y
                || plot_ui.response().double_clicked()
                || changed_scales[1]
            {
                plot_ui.set_plot_bounds_y(lower..=upper);
            }
        }
    }

    fn add_series(&self, plot_ui: &mut PlotUi<'_>, series_segments: Vec<Vec<Points>>) {
        for (index, (line, segments)) in self.series.iter().zip(series_segments).enumerate() {
            let colour = MATPLOTLIB_COLORS[index % MATPLOTLIB_COLORS.len()];
            let legend_name = line.label.as_deref().unwrap_or_default();
            let segment_count = segments.len();

            for (segment_index, points) in segments.into_iter().enumerate() {
                let line_name = if segment_count == 1 {
                    format!("series_{index}")
                } else {
                    format!("series_{index}_segment_{segment_index}")
                };
                let plot_line = Line::new(line_name, PlotPoints::from(points))
                    .name(legend_name)
                    .color(colour)
                    .width(DEFAULT_LINE_WIDTH);

                plot_ui.line(plot_line);
            }
        }
    }

    fn add_horizontal_lines(&self, plot_ui: &mut PlotUi<'_>, values: Vec<f64>) {
        let colour_offset = self.series.len();

        for (index, (line, value)) in self.horizontal_lines.iter().zip(values).enumerate() {
            let colour = MATPLOTLIB_COLORS[(colour_offset + index) % MATPLOTLIB_COLORS.len()];
            let legend_name = line.label.as_deref().unwrap_or_default();

            let plot_line = HLine::new(format!("hline_{index}"), value)
                .name(legend_name)
                .color(colour)
                .width(DEFAULT_LINE_WIDTH)
                .style(LineStyle::dashed_dense());

            plot_ui.hline(plot_line);
        }
    }

    fn add_vertical_lines(&self, plot_ui: &mut PlotUi<'_>, values: Vec<f64>) {
        let colour_offset = self.series.len() + self.horizontal_lines.len();

        for (index, (line, value)) in self.vertical_lines.iter().zip(values).enumerate() {
            let colour = MATPLOTLIB_COLORS[(colour_offset + index) % MATPLOTLIB_COLORS.len()];
            let legend_name = line.label.as_deref().unwrap_or_default();

            let plot_line = VLine::new(format!("vline_{index}"), value)
                .name(legend_name)
                .color(colour)
                .width(DEFAULT_LINE_WIDTH)
                .style(LineStyle::dashed_dense());

            plot_ui.vline(plot_line);
        }
    }

    /// Opens a native window and blocks until it is closed.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn show(self) -> crate::Result {
        crate::Figure::from_plotter(self).show()
    }

    /// Draws into the available UI area. Use a stable, distinct ID for each plot.
    pub fn show_ui(&self, ui: &mut egui::Ui, id: impl std::hash::Hash) -> egui::Rect {
        let render_data = self.transform_for_rendering();

        let axes_rect = ui.available_rect_before_wrap();

        if !self.title.is_empty() {
            ui.vertical_centered(|ui| {
                ui.heading(&self.title);
            });

            ui.add_space(4.0);
        }

        let plot_height = (axes_rect.bottom() - ui.next_widget_position().y).max(1.0);
        let plot_id = ui.make_persistent_id(egui::Id::new(&id));
        let scale_memory_id = plot_id.with("myplotlib_axis_scales");
        let previous_scales = ui.data_mut(|data| {
            let previous = data.get_temp::<[AxisScale; 2]>(scale_memory_id);
            data.insert_temp(scale_memory_id, [self.x_scale, self.y_scale]);
            previous
        });
        let changed_scales = previous_scales
            .map(|previous| [previous[0] != self.x_scale, previous[1] != self.y_scale])
            .unwrap_or([false, false]);

        let mut plot = Plot::new(id)
            .legend(Legend::default())
            .x_axis_label(egui::RichText::new(&self.x_label).size(DEFAULT_LABEL_FONT_SIZE))
            .y_axis_label(egui::RichText::new(&self.y_label).size(DEFAULT_LABEL_FONT_SIZE))
            .allow_drag(true)
            .allow_scroll(true)
            .allow_zoom(true)
            .allow_boxed_zoom(true)
            .allow_double_click_reset(true)
            .width(axes_rect.width())
            .height(plot_height);

        if self.x_scale.is_logarithmic() {
            plot = plot
                .x_grid_spacer(log10_grid_marks)
                .x_axis_formatter(format_log10_tick);
        }
        if self.y_scale.is_logarithmic() {
            plot = plot
                .y_grid_spacer(log10_grid_marks)
                .y_axis_formatter(format_log10_tick);
        }
        if self.x_scale.is_logarithmic() || self.y_scale.is_logarithmic() {
            let x_scale = self.x_scale;
            let y_scale = self.y_scale;
            plot = plot.label_formatter(move |name, point| {
                format_hover_label(name, point, x_scale, y_scale)
            });
        }

        // Apply plot text styling without changing the surrounding UI or plot ID scope.
        let original_style = ui.style().clone();
        ui.style_mut().text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::proportional(DEFAULT_LABEL_FONT_SIZE),
        );

        let response = plot.show(ui, |plot_ui| {
            let RenderData {
                series_segments,
                x_limits,
                y_limits,
                horizontal_lines,
                vertical_lines,
            } = render_data;

            self.apply_bounds(plot_ui, x_limits, y_limits, changed_scales);
            self.add_series(plot_ui, series_segments);
            self.add_horizontal_lines(plot_ui, horizontal_lines);
            self.add_vertical_lines(plot_ui, vertical_lines);
        });
        ui.set_style(original_style);

        response.response.rect
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms_and_restores_log10_values() {
        assert_eq!(AxisScale::Linear.forward(12.5), Some(12.5));
        assert_eq!(AxisScale::Log10.forward(0.1), Some(-1.0));
        assert_eq!(AxisScale::Log10.forward(1.0), Some(0.0));
        assert_eq!(AxisScale::Log10.forward(1000.0), Some(3.0));
        assert_eq!(AxisScale::Log10.inverse(3.0), 1000.0);
        assert_eq!(AxisScale::Log10.forward(0.0), None);
        assert_eq!(AxisScale::Log10.forward(-1.0), None);
    }

    #[test]
    fn nonpositive_log_values_split_lines() {
        let points = vec![[1.0, 10.0], [10.0, 0.0], [100.0, 1000.0]];
        let segments = transform_line(&points, AxisScale::Log10, AxisScale::Log10);

        assert_eq!(segments, vec![vec![[0.0, 1.0]], vec![[2.0, 3.0]]]);
    }

    #[test]
    fn log_grid_contains_decades_and_minor_marks() {
        let marks = log10_grid_marks(GridInput {
            bounds: (-1.0, 2.0),
            base_step_size: 0.04,
        });

        assert!(marks.iter().any(|mark| mark.value == -1.0));
        assert!(marks.iter().any(|mark| mark.value == 0.0));
        assert!(marks.iter().any(|mark| mark.value == 1.0));
        assert!(
            marks
                .iter()
                .any(|mark| (mark.value - 2.0_f64.log10()).abs() < 1e-12)
        );
    }

    #[test]
    fn log_grid_uses_conventional_ticks_within_one_decade() {
        let marks = log10_grid_marks(GridInput {
            bounds: (2.0, 2.8),
            base_step_size: 0.01,
        });
        let four_hundred = 2.0 + 4.0_f64.log10();

        assert!(
            marks
                .iter()
                .any(|mark| (mark.value - four_hundred).abs() < 1e-12)
        );
    }

    #[test]
    fn log_grid_keeps_coarser_decade_marks_for_wide_ranges() {
        let marks = log10_grid_marks(GridInput {
            bounds: (-8.0, 8.0),
            base_step_size: 0.5,
        });
        let step_size_at = |exponent| {
            marks
                .iter()
                .find(|mark| mark.value == exponent)
                .map(|mark| mark.step_size)
        };

        assert_eq!(step_size_at(3.0), Some(1.0));
        assert_eq!(step_size_at(5.0), Some(5.0));
        assert_eq!(step_size_at(0.0), Some(10.0));
    }

    #[test]
    fn log_grid_excludes_marks_outside_visible_bounds() {
        let lower = -4.5;
        let upper = 6.5;
        let marks = log10_grid_marks(GridInput {
            bounds: (lower, upper),
            base_step_size: 0.2,
        });

        assert!(
            marks
                .iter()
                .all(|mark| (lower..upper).contains(&mark.value))
        );
        assert!(!marks.iter().any(|mark| mark.value == 7.0));
    }

    #[test]
    fn log_ticks_and_hover_labels_use_original_values() {
        let broad_range = -1.0..=2.0;
        assert_eq!(
            format_log10_tick(
                GridMark {
                    value: -2.0,
                    step_size: 1.0,
                },
                &broad_range,
            ),
            "1e-2"
        );
        assert_eq!(
            format_log10_tick(
                GridMark {
                    value: 0.0,
                    step_size: 1.0,
                },
                &broad_range,
            ),
            "1e0"
        );
        assert_eq!(
            format_log10_tick(
                GridMark {
                    value: 2.0,
                    step_size: 1.0,
                },
                &broad_range,
            ),
            "1e2"
        );
        assert_eq!(
            format_log10_tick(
                GridMark {
                    value: 2.0_f64.log10(),
                    step_size: 0.1,
                },
                &broad_range,
            ),
            "2e0"
        );

        assert_eq!(
            format_log10_tick(
                GridMark {
                    value: 2.0 + 4.0_f64.log10(),
                    step_size: 0.1,
                },
                &broad_range,
            ),
            "4e2"
        );
        assert_eq!(
            format_log10_tick(
                GridMark {
                    value: 3.0 - 1e-12,
                    step_size: 1.0,
                },
                &broad_range,
            ),
            "1e3"
        );

        let label = format_hover_label(
            "Signal",
            &PlotPoint::new(1.0, -2.0),
            AxisScale::Log10,
            AxisScale::Log10,
        );
        assert_eq!(label, "Signal\nx = 10\ny = 0.01");
    }

    #[test]
    #[should_panic(expected = "x limits must be finite and greater than zero on a log10 axis")]
    fn rejects_nonpositive_log_limits() {
        AxisScale::Log10.transform_required(0.0, "x limits");
    }
}
