use crate::{Points, data_import::ImportedSeries};
use eframe::egui;
use egui_plot::{
    AxisHints, GridInput, GridMark, HLine, HPlacement, Legend, Line, LineStyle, Plot, PlotMemory,
    PlotPoint, PlotPoints, PlotUi, Points as PlotMarkers, VLine,
};
use std::fmt::Display;

const DEFAULT_LINE_WIDTH: f32 = 3.0;
const DEFAULT_MARKER_RADIUS: f32 = 4.0;
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

/// Side of a plot on which a y-axis is displayed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YAxisSide {
    Left,
    Right,
}

/// Identifies a y-axis within the [`Plotter`] that created it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct YAxisId(usize);

impl YAxisId {
    const PRIMARY: Self = Self(0);
}

/// Configuration for one y-axis.
pub struct YAxis {
    side: YAxisSide,
    label: String,
    limits: Option<(f64, f64)>,
    limits_pending: std::cell::Cell<bool>,
    scale: AxisScale,
}

impl YAxis {
    fn primary() -> Self {
        Self {
            side: YAxisSide::Left,
            label: String::new(),
            limits: None,
            limits_pending: std::cell::Cell::new(false),
            scale: AxisScale::Linear,
        }
    }

    fn new(side: YAxisSide) -> Self {
        Self {
            side,
            ..Self::primary()
        }
    }

    /// Sets the text displayed beside this axis.
    pub fn label(&mut self, label: impl Into<String>) -> &mut Self {
        self.label = label.into();
        self
    }

    /// Sets the initial and reset bounds for this axis.
    pub fn limits(&mut self, lower: f64, upper: f64) -> &mut Self {
        self.limits = Some((lower, upper));
        self.limits_pending.set(true);
        self
    }

    /// Sets the coordinate scale for this axis.
    pub fn scale(&mut self, scale: AxisScale) -> &mut Self {
        self.scale = scale;
        self
    }
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

fn transform_points(points: &Points, x_scale: AxisScale, y_scale: AxisScale) -> Points {
    points
        .iter()
        .filter_map(|&[x, y]| {
            x_scale
                .forward(x)
                .zip(y_scale.forward(y))
                .map(|(x, y)| [x, y])
                .filter(|[x, y]| x.is_finite() && y.is_finite())
        })
        .collect()
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

fn format_tick_value(value: f64, step_size: f64) -> String {
    if !value.is_finite() || !step_size.is_finite() || step_size <= 0.0 {
        return format_value(value);
    }

    let absolute = value.abs();
    if value != 0.0 && !(0.001..10_000.0).contains(&absolute) {
        let exponent = absolute.log10().floor();
        let scaled_step = step_size / 10.0_f64.powf(exponent);
        let decimals = (-scaled_step.log10()).ceil().clamp(0.0, 3.0) as usize;
        return format!("{value:.decimals$e}");
    }

    let decimals = (-step_size.log10()).ceil().clamp(0.0, 6.0) as usize;
    format!("{value:.decimals$}")
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

#[derive(Clone, Copy, Debug, PartialEq)]
struct AxisDomain {
    lower: f64,
    upper: f64,
}

impl AxisDomain {
    const FALLBACK: Self = Self {
        lower: 0.0,
        upper: 1.0,
    };

    fn new(lower: f64, upper: f64, description: &str) -> Self {
        assert!(
            lower.is_finite() && upper.is_finite() && lower < upper,
            "{description} must be finite and have lower < upper"
        );
        Self { lower, upper }
    }

    fn from_values(lower: f64, upper: f64) -> Self {
        if !lower.is_finite() || !upper.is_finite() {
            return Self::FALLBACK;
        }

        if lower < upper {
            return Self { lower, upper };
        }

        let padding = if lower == 0.0 { 1.0 } else { lower.abs() * 0.5 };
        Self {
            lower: lower - padding,
            upper: upper + padding,
        }
    }

    fn span(self) -> f64 {
        self.upper - self.lower
    }

    fn map_to(self, value: f64, target: Self) -> f64 {
        target.lower + (value - self.lower) * target.span() / self.span()
    }

    fn range(self) -> (f64, f64) {
        (self.lower, self.upper)
    }

    fn approximately_matches(self, range: std::ops::RangeInclusive<f64>) -> bool {
        let tolerance = self.lower.abs().max(self.upper.abs()).max(1.0) * f64::EPSILON * 16.0;
        (*range.start() - self.lower).abs() <= tolerance
            && (*range.end() - self.upper).abs() <= tolerance
    }
}

fn format_mapped_tick(
    mark: GridMark,
    primary_domain: AxisDomain,
    axis_domain: AxisDomain,
    scale: AxisScale,
) -> String {
    let transformed = primary_domain.map_to(mark.value, axis_domain);
    let next_transformed = primary_domain.map_to(mark.value + mark.step_size, axis_domain);
    let value = scale.inverse(transformed);
    let next_value = scale.inverse(next_transformed);
    format_tick_value(value, (next_value - value).abs())
}

fn paint_mirrored_right_y_label(
    ui: &egui::Ui,
    plot_rect: egui::Rect,
    complete_rect: egui::Rect,
    label: &str,
) {
    let color = ui.visuals().text_color();
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(DEFAULT_LABEL_FONT_SIZE),
        color,
    );
    let center = egui::pos2(
        complete_rect.right() - galley.size().y * 0.625,
        plot_rect.center().y,
    );
    let pos = center - galley.size() * 0.5;
    ui.painter().add(
        egui::epaint::TextShape::new(pos, galley, color)
            .with_angle_and_anchor(std::f32::consts::FRAC_PI_2, egui::Align2::CENTER_CENTER),
    );
}

#[derive(Clone, Debug, PartialEq)]
struct YAxisSignature {
    side: YAxisSide,
    scale: AxisScale,
    limits: Option<(f64, f64)>,
}

#[derive(Clone, Debug, PartialEq)]
struct YAxesSignature(Vec<YAxisSignature>);

#[derive(Clone)]
struct TwinAxesMemory {
    signature: YAxesSignature,
    domains: Vec<AxisDomain>,
}

fn format_twin_hover_label(
    name: &str,
    point: &PlotPoint,
    x_scale: AxisScale,
    axes: &[(String, YAxisSide, AxisScale, AxisDomain)],
    primary_domain: AxisDomain,
) -> String {
    let mut label = if name.is_empty() {
        String::new()
    } else {
        format!("{name}\n")
    };
    label.push_str(&format!("x = {}", format_value(x_scale.inverse(point.x))));

    for (axis_label, side, scale, domain) in axes {
        let transformed = primary_domain.map_to(point.y, *domain);
        let value = format_value(scale.inverse(transformed));
        let axis_label = if axis_label.is_empty() {
            match side {
                YAxisSide::Left => "y (left)",
                YAxisSide::Right => "y (right)",
            }
        } else {
            axis_label
        };
        label.push_str(&format!("\n{axis_label} = {value}"));
    }

    label
}

struct RenderData {
    series_segments: Vec<Vec<Points>>,
    x_limits: Option<(f64, f64)>,
    y_limits: Option<(f64, f64)>,
    horizontal_lines: Vec<f64>,
    vertical_lines: Vec<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SeriesStyle {
    Line,
    Points,
}

pub struct PlotLine {
    id: SeriesId,
    points: Points,
    label: Option<String>,
    style: SeriesStyle,
    y_axis: YAxisId,
}

impl PlotLine {
    pub fn label(&mut self, label: impl Into<String>) -> &mut Self {
        self.label = Some(label.into());
        self
    }

    /// Renders this series as unconnected point markers.
    pub fn points_only(&mut self) -> &mut Self {
        self.style = SeriesStyle::Points;
        self
    }

    /// Assigns this series to a y-axis created by the same plotter.
    pub fn y_axis(&mut self, axis: YAxisId) -> &mut Self {
        self.y_axis = axis;
        self
    }

    /// Returns the stable identifier used to update this series.
    pub fn id(&self) -> SeriesId {
        self.id
    }
}

/// Identifies a data series within the [`Plotter`] that created it.
///
/// Series identifiers remain valid for the lifetime of their plotter because
/// series cannot currently be removed or reordered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SeriesId(usize);

pub struct ReferenceLine {
    value: f64,
    label: Option<String>,
    y_axis: Option<YAxisId>,
}

impl ReferenceLine {
    pub fn label(&mut self, label: impl Into<String>) -> &mut Self {
        self.label = Some(label.into());
        self
    }

    /// Assigns a horizontal reference line to a y-axis.
    ///
    /// This has no effect on vertical reference lines, which use the shared x-axis.
    pub fn y_axis(&mut self, axis: YAxisId) -> &mut Self {
        if self.y_axis.is_some() {
            self.y_axis = Some(axis);
        }
        self
    }
}

/// A labeled value displayed in the summary below a plot.
pub struct SummaryItem {
    label: String,
    value: String,
}

pub struct Plotter {
    heatmap: Option<crate::Heatmap>,
    series: Vec<PlotLine>,
    horizontal_lines: Vec<ReferenceLine>,
    vertical_lines: Vec<ReferenceLine>,
    summary: Vec<SummaryItem>,
    x_label: String,
    y_axes: Vec<YAxis>,
    pub(crate) title: String,
    x_limits: Option<(f64, f64)>,
    x_limits_pending: std::cell::Cell<bool>,
    x_scale: AxisScale,
}

impl Default for Plotter {
    fn default() -> Self {
        Self {
            heatmap: None,
            series: Vec::new(),
            horizontal_lines: Vec::new(),
            vertical_lines: Vec::new(),
            summary: Vec::new(),
            x_label: String::new(),
            y_axes: vec![YAxis::primary()],
            title: String::new(),
            x_limits: None,
            x_limits_pending: std::cell::Cell::new(false),
            x_scale: AxisScale::Linear,
        }
    }
}

impl Plotter {
    /// Sets the single heatmap on the primary axes, replacing any previous grid.
    /// Values are row-major with row zero at the bottom; bounds are outer cell edges.
    /// Panics for empty/mismatched dimensions or invalid bounds; see [`crate::Heatmap::new`].
    pub fn heatmap(
        &mut self,
        values: Vec<f64>,
        rows: usize,
        columns: usize,
        x_bounds: [f64; 2],
        y_bounds: [f64; 2],
    ) -> &mut crate::Heatmap {
        self.heatmap = Some(crate::Heatmap::new(
            values, rows, columns, x_bounds, y_bounds,
        ));
        self.heatmap.as_mut().unwrap()
    }

    pub fn new() -> Self {
        Self::default()
    }

    fn primary_y_axis(&self) -> &YAxis {
        &self.y_axes[YAxisId::PRIMARY.0]
    }

    fn primary_y_axis_mut(&mut self) -> &mut YAxis {
        &mut self.y_axes[YAxisId::PRIMARY.0]
    }

    fn y_axis(&self, id: YAxisId) -> &YAxis {
        self.y_axes.get(id.0).unwrap_or_else(|| {
            panic!(
                "y-axis id {} is out of bounds for a plotter with {} y-axes",
                id.0,
                self.y_axes.len()
            )
        })
    }

    pub fn plot(&mut self, x: &[f64], y: &[f64]) -> &mut PlotLine {
        assert_eq!(
            x.len(),
            y.len(),
            "x and y must contain the same number of values"
        );

        let points = x.iter().zip(y.iter()).map(|(&x, &y)| [x, y]).collect();

        let id = SeriesId(self.series.len());
        self.series.push(PlotLine {
            id,
            points,
            label: None,
            style: SeriesStyle::Line,
            y_axis: YAxisId::PRIMARY,
        });

        self.series.last_mut().unwrap()
    }

    pub fn add_points(&mut self, points: Points) -> &mut PlotLine {
        let id = SeriesId(self.series.len());
        self.series.push(PlotLine {
            id,
            points,
            label: None,
            style: SeriesStyle::Line,
            y_axis: YAxisId::PRIMARY,
        });

        self.series.last_mut().unwrap()
    }

    /// Appends points to an existing series.
    ///
    /// Pan and zoom state are preserved because the existing series is
    /// updated in place. Panics if `id` was not created by this plotter.
    pub fn append_points(
        &mut self,
        id: SeriesId,
        points: impl IntoIterator<Item = [f64; 2]>,
    ) -> &mut PlotLine {
        let series = self.series_mut(id);
        series.points.extend(points);
        series
    }

    /// Replaces every point in an existing series.
    ///
    /// Pan and zoom state are preserved because the existing series is
    /// updated in place. Panics if `id` was not created by this plotter.
    pub fn replace_points(&mut self, id: SeriesId, points: Points) -> &mut PlotLine {
        let series = self.series_mut(id);
        series.points = points;
        series
    }

    /// Removes every point from an existing series.
    ///
    /// Panics if `id` was not created by this plotter.
    pub fn clear_points(&mut self, id: SeriesId) -> &mut PlotLine {
        let series = self.series_mut(id);
        series.points.clear();
        series
    }

    fn series_mut(&mut self, id: SeriesId) -> &mut PlotLine {
        let series_count = self.series.len();
        self.series.get_mut(id.0).unwrap_or_else(|| {
            panic!(
                "series id {} is out of bounds for a plotter with {series_count} series",
                id.0
            )
        })
    }

    /// Sets the initial and reset x bounds, while allowing subsequent pan and zoom.
    pub fn xlim(&mut self, lower: f64, upper: f64) {
        self.x_limits = Some((lower, upper));
        self.x_limits_pending.set(true);
    }

    /// Sets the initial and reset y bounds, while allowing subsequent pan and zoom.
    pub fn ylim(&mut self, lower: f64, upper: f64) {
        self.primary_y_axis_mut().limits(lower, upper);
    }

    /// Sets the scaling used by the x axis.
    pub fn xscale(&mut self, scale: AxisScale) {
        self.x_scale = scale;
    }

    /// Sets the scaling used by the y axis.
    pub fn yscale(&mut self, scale: AxisScale) {
        self.primary_y_axis_mut().scale(scale);
    }

    /// Adds another y-axis and returns its stable identifier.
    pub fn add_y_axis(&mut self, side: YAxisSide) -> YAxisId {
        let id = YAxisId(self.y_axes.len());
        self.y_axes.push(YAxis::new(side));
        id
    }

    /// Returns mutable configuration for a y-axis created by this plotter.
    pub fn y_axis_mut(&mut self, id: YAxisId) -> &mut YAxis {
        let axis_count = self.y_axes.len();
        self.y_axes.get_mut(id.0).unwrap_or_else(|| {
            panic!(
                "y-axis id {} is out of bounds for a plotter with {axis_count} y-axes",
                id.0
            )
        })
    }

    pub fn axhline(&mut self, y: f64) -> &mut ReferenceLine {
        self.horizontal_lines.push(ReferenceLine {
            value: y,
            label: None,
            y_axis: Some(YAxisId::PRIMARY),
        });

        self.horizontal_lines.last_mut().unwrap()
    }

    pub fn axvline(&mut self, x: f64) -> &mut ReferenceLine {
        self.vertical_lines.push(ReferenceLine {
            value: x,
            label: None,
            y_axis: None,
        });

        self.vertical_lines.last_mut().unwrap()
    }

    /// Adds a labeled value to the summary displayed below the plot.
    pub fn add_summary(
        &mut self,
        label: impl Into<String>,
        value: impl Display,
    ) -> &mut SummaryItem {
        self.summary.push(SummaryItem {
            label: label.into(),
            value: value.to_string(),
        });

        self.summary.last_mut().unwrap()
    }

    pub fn xlabel(&mut self, label: impl Into<String>) {
        self.x_label = label.into();
    }

    pub fn ylabel(&mut self, label: impl Into<String>) {
        self.primary_y_axis_mut().label(label);
    }

    pub fn title(&mut self, title: impl Into<String>) {
        self.title = title.into();
    }

    fn transform_for_rendering(&self) -> RenderData {
        let y_axis = self.primary_y_axis();
        let series_segments = self
            .series
            .iter()
            .map(|series| {
                self.y_axis(series.y_axis);
                match series.style {
                    SeriesStyle::Line => transform_line(&series.points, self.x_scale, y_axis.scale),
                    SeriesStyle::Points => {
                        vec![transform_points(&series.points, self.x_scale, y_axis.scale)]
                    }
                }
            })
            .collect();
        let x_limits = self.x_limits.map(|(lower, upper)| {
            (
                self.x_scale.transform_required(lower, "x limits"),
                self.x_scale.transform_required(upper, "x limits"),
            )
        });
        let y_limits = y_axis.limits.map(|(lower, upper)| {
            (
                y_axis.scale.transform_required(lower, "y limits"),
                y_axis.scale.transform_required(upper, "y limits"),
            )
        });
        let horizontal_lines = self
            .horizontal_lines
            .iter()
            .map(|line| {
                y_axis
                    .scale
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

    fn y_axes_signature(&self) -> YAxesSignature {
        YAxesSignature(
            self.y_axes
                .iter()
                .map(|axis| YAxisSignature {
                    side: axis.side,
                    scale: axis.scale,
                    limits: axis.limits,
                })
                .collect(),
        )
    }

    fn candidate_y_domains(&self, imported: &[ImportedSeries]) -> Vec<AxisDomain> {
        let mut bounds = vec![(f64::INFINITY, f64::NEG_INFINITY); self.y_axes.len()];

        let mut include = |axis_id: YAxisId, value: f64| {
            let axis = self.y_axis(axis_id);
            if let Some(value) = axis.scale.forward(value).filter(|value| value.is_finite()) {
                bounds[axis_id.0].0 = bounds[axis_id.0].0.min(value);
                bounds[axis_id.0].1 = bounds[axis_id.0].1.max(value);
            }
        };

        if let Some(heatmap) = &self.heatmap {
            for value in heatmap.y_bounds {
                include(YAxisId::PRIMARY, value);
            }
        }
        for series in &self.series {
            self.y_axis(series.y_axis);
            for point in &series.points {
                include(series.y_axis, point[1]);
            }
        }
        for line in &self.horizontal_lines {
            let axis = line.y_axis.expect("horizontal lines have a y-axis");
            include(axis, line.value);
        }
        for series in imported {
            for point in &series.points {
                include(YAxisId::PRIMARY, point[1]);
            }
        }

        self.y_axes
            .iter()
            .enumerate()
            .map(|(index, axis)| {
                if let Some((lower, upper)) = axis.limits {
                    let lower = axis.scale.transform_required(lower, "y limits");
                    let upper = axis.scale.transform_required(upper, "y limits");
                    AxisDomain::new(lower, upper, "y limits")
                } else {
                    AxisDomain::from_values(bounds[index].0, bounds[index].1)
                }
            })
            .collect()
    }

    fn transform_for_twin_axes(&self, domains: &[AxisDomain]) -> RenderData {
        let primary_domain = domains[YAxisId::PRIMARY.0];
        let map_points = |points: &mut Points, axis: YAxisId| {
            let domain = domains[axis.0];
            for point in points {
                point[1] = domain.map_to(point[1], primary_domain);
            }
        };

        let series_segments = self
            .series
            .iter()
            .map(|series| {
                let axis = self.y_axis(series.y_axis);
                match series.style {
                    SeriesStyle::Line => {
                        let mut segments = transform_line(&series.points, self.x_scale, axis.scale);
                        for segment in &mut segments {
                            map_points(segment, series.y_axis);
                        }
                        segments
                    }
                    SeriesStyle::Points => {
                        let mut points = transform_points(&series.points, self.x_scale, axis.scale);
                        map_points(&mut points, series.y_axis);
                        vec![points]
                    }
                }
            })
            .collect();
        let x_limits = self.x_limits.map(|(lower, upper)| {
            (
                self.x_scale.transform_required(lower, "x limits"),
                self.x_scale.transform_required(upper, "x limits"),
            )
        });
        let y_limits = self
            .y_axes
            .iter()
            .any(|axis| axis.limits.is_some())
            .then(|| primary_domain.range());
        let horizontal_lines = self
            .horizontal_lines
            .iter()
            .map(|line| {
                let axis_id = line.y_axis.expect("horizontal lines have a y-axis");
                let axis = self.y_axis(axis_id);
                let value = axis
                    .scale
                    .transform_required(line.value, "horizontal reference line");
                domains[axis_id.0].map_to(value, primary_domain)
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
        y_limits_pending: bool,
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
            if y_limits_pending
                || plot_ui.auto_bounds().y
                || plot_ui.response().double_clicked()
                || changed_scales[1]
            {
                plot_ui.set_plot_bounds_y(lower..=upper);
            }
        }
    }

    fn add_series(&self, plot_ui: &mut PlotUi<'_>, series_segments: Vec<Vec<Points>>) {
        for (index, (series, segments)) in self.series.iter().zip(series_segments).enumerate() {
            let colour = MATPLOTLIB_COLORS[index % MATPLOTLIB_COLORS.len()];
            let legend_name = series.label.as_deref().unwrap_or_default();

            if series.style == SeriesStyle::Points {
                let points = segments.into_iter().flatten().collect::<Points>();
                let markers = PlotMarkers::new(format!("series_{index}"), PlotPoints::from(points))
                    .name(legend_name)
                    .color(colour)
                    .radius(DEFAULT_MARKER_RADIUS);
                plot_ui.points(markers);
                continue;
            }

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

    fn add_imported_series(&self, plot_ui: &mut PlotUi<'_>, imported: &[ImportedSeries]) {
        let colour_offset =
            self.series.len() + self.horizontal_lines.len() + self.vertical_lines.len();

        for (index, series) in imported.iter().enumerate() {
            let colour = MATPLOTLIB_COLORS[(colour_offset + index) % MATPLOTLIB_COLORS.len()];
            let points =
                transform_points(&series.points, self.x_scale, self.primary_y_axis().scale);
            let markers = PlotMarkers::new(
                format!("imported_series_{}", series.id),
                PlotPoints::from(points),
            )
            .name(&series.label)
            .color(colour)
            .radius(DEFAULT_MARKER_RADIUS);

            plot_ui.points(markers);
        }
    }

    fn show_summary(&self, ui: &mut egui::Ui, plot_id: egui::Id) {
        if self.summary.is_empty() {
            return;
        }

        let scroll_style = egui::style::ScrollStyle::solid();
        let row_height = ui
            .text_style_height(&egui::TextStyle::Body)
            .max(ui.spacing().interact_size.y);
        let panel_height =
            row_height + scroll_style.allocated_width() + 2.0 * ui.spacing().item_spacing.y;

        egui::TopBottomPanel::bottom(plot_id.with("myplotlib_summary_panel"))
            .resizable(false)
            .exact_height(panel_height)
            .show_inside(ui, |ui| {
                ui.spacing_mut().scroll = scroll_style;
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);

                egui::ScrollArea::horizontal()
                    .id_salt(plot_id.with("myplotlib_summary_scroll"))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for (index, item) in self.summary.iter().enumerate() {
                                if index > 0 {
                                    ui.separator();
                                }
                                ui.label(egui::RichText::new(&item.label).strong());
                                ui.label(&item.value);
                            }
                        });
                    });
            });
    }

    /// Opens a native window and blocks until it is closed.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn show(self) -> crate::Result {
        crate::Figure::from_plotter(self).show()
    }

    /// Draws into the available UI area. Use a stable, distinct ID for each plot.
    pub fn show_ui(&self, ui: &mut egui::Ui, id: impl std::hash::Hash) -> egui::Rect {
        self.show_ui_with_overlays(ui, id, &[])
    }

    pub(crate) fn show_ui_with_overlays(
        &self,
        ui: &mut egui::Ui,
        id: impl std::hash::Hash,
        imported: &[ImportedSeries],
    ) -> egui::Rect {
        if let Some(heatmap) = &self.heatmap {
            assert!(
                self.x_scale == AxisScale::Linear
                    && self.primary_y_axis().scale == AxisScale::Linear,
                "heatmaps require linear x and primary y axes"
            );
            let rect = ui.available_rect_before_wrap();
            let bar_width = 100.0_f32.min(rect.width() * 0.25);
            let mut bar_ui =
                ui.new_child(egui::UiBuilder::new().id_salt("heatmap_colorbar").max_rect(
                    egui::Rect::from_min_max(
                        egui::pos2(rect.right() - bar_width, rect.top()),
                        rect.max,
                    ),
                ));
            bar_ui.set_clip_rect(bar_ui.max_rect());
            heatmap.colorbar(&mut bar_ui);
            let mut plot_ui =
                ui.new_child(egui::UiBuilder::new().id_salt("heatmap_plot").max_rect(
                    egui::Rect::from_min_max(
                        rect.min,
                        egui::pos2(rect.right() - bar_width - 8.0, rect.bottom()),
                    ),
                ));
            let result = self.show_plot_ui(&mut plot_ui, id, imported);
            ui.advance_cursor_after_rect(rect);
            return result;
        }
        self.show_plot_ui(ui, id, imported)
    }

    fn show_plot_ui(
        &self,
        ui: &mut egui::Ui,
        id: impl std::hash::Hash,
        imported: &[ImportedSeries],
    ) -> egui::Rect {
        let plot_id = ui.make_persistent_id(egui::Id::new(&id));
        let y_limits_pending = self.y_axes.iter().fold(false, |pending, axis| {
            axis.limits_pending.replace(false) || pending
        });
        let current_scales = (
            self.x_scale,
            self.y_axes
                .iter()
                .map(|axis| axis.scale)
                .collect::<Vec<_>>(),
        );
        let scale_memory_id = plot_id.with("myplotlib_axis_scales");
        let previous_scales = ui.data_mut(|data| {
            let previous = data.get_temp::<(AxisScale, Vec<AxisScale>)>(scale_memory_id);
            data.insert_temp(scale_memory_id, current_scales.clone());
            previous
        });
        let changed_scales = previous_scales
            .map(|previous| {
                [
                    previous.0 != current_scales.0,
                    previous.1 != current_scales.1,
                ]
            })
            .unwrap_or([false, false]);

        let (twin_domains, twin_domains_refreshed) = if self.y_axes.len() > 1 {
            let signature = self.y_axes_signature();
            let memory_id = plot_id.with("myplotlib_twin_y_axes");
            let previous = ui.data_mut(|data| data.get_temp::<TwinAxesMemory>(memory_id));
            let plot_memory = PlotMemory::load(ui.ctx(), plot_id);
            let plot_was_auto = plot_memory
                .as_ref()
                .is_none_or(|memory| memory.auto_bounds.y);
            let plot_was_at_base = plot_memory.as_ref().zip(previous.as_ref()).is_some_and(
                |(plot_memory, axes_memory)| {
                    axes_memory.domains[YAxisId::PRIMARY.0]
                        .approximately_matches(plot_memory.bounds().range_y())
                },
            );
            let has_automatic_axis = self.y_axes.iter().any(|axis| axis.limits.is_none());
            let refresh = y_limits_pending
                || changed_scales[1]
                || previous
                    .as_ref()
                    .is_none_or(|memory| memory.signature != signature)
                || has_automatic_axis && (plot_was_auto || plot_was_at_base);
            let domains = if refresh {
                self.candidate_y_domains(imported)
            } else {
                previous.expect("checked above").domains
            };
            ui.data_mut(|data| {
                data.insert_temp(
                    memory_id,
                    TwinAxesMemory {
                        signature,
                        domains: domains.clone(),
                    },
                );
            });
            (Some(domains), refresh)
        } else {
            (None, false)
        };
        let render_data = twin_domains.as_deref().map_or_else(
            || self.transform_for_rendering(),
            |domains| self.transform_for_twin_axes(domains),
        );

        if !self.title.is_empty() {
            ui.vertical_centered(|ui| {
                ui.heading(&self.title);
            });

            ui.add_space(4.0);
        }

        self.show_summary(ui, plot_id);

        let axes_rect = ui.available_rect_before_wrap();
        let plot_height = (axes_rect.bottom() - ui.next_widget_position().y).max(1.0);
        let primary_y_axis = self.primary_y_axis();
        let mirrored_right_axis = self
            .y_axes
            .iter()
            .position(|axis| axis.side == YAxisSide::Right && !axis.label.is_empty());
        let mirrored_right_label =
            mirrored_right_axis.map(|index| self.y_axes[index].label.clone());

        let mut plot = Plot::new(id)
            .legend(Legend::default())
            .x_axis_label(egui::RichText::new(&self.x_label).size(DEFAULT_LABEL_FONT_SIZE))
            .allow_drag(true)
            .allow_scroll(true)
            .allow_zoom(true)
            .allow_boxed_zoom(true)
            .allow_double_click_reset(true)
            .width(axes_rect.width())
            .height(plot_height);

        if let Some(domains) = twin_domains.as_deref() {
            let primary_domain = domains[YAxisId::PRIMARY.0];
            let axis_hints = self
                .y_axes
                .iter()
                .enumerate()
                .map(|(index, axis)| {
                    let mut hints = AxisHints::new_y()
                        .label(
                            egui::RichText::new(if mirrored_right_axis == Some(index) {
                                " "
                            } else {
                                &axis.label
                            })
                            .size(DEFAULT_LABEL_FONT_SIZE),
                        )
                        .placement(match axis.side {
                            YAxisSide::Left => HPlacement::Left,
                            YAxisSide::Right => HPlacement::Right,
                        });
                    if index == YAxisId::PRIMARY.0 && axis.scale.is_logarithmic() {
                        hints = hints.formatter(format_log10_tick);
                    } else if index != YAxisId::PRIMARY.0 {
                        let domain = domains[index];
                        let scale = axis.scale;
                        hints = hints.formatter(move |mark, _| {
                            format_mapped_tick(mark, primary_domain, domain, scale)
                        });
                    }
                    hints
                })
                .collect();
            plot = plot.custom_y_axes(axis_hints);

            let hover_axes = self
                .y_axes
                .iter()
                .zip(domains)
                .map(|(axis, &domain)| (axis.label.clone(), axis.side, axis.scale, domain))
                .collect::<Vec<_>>();
            let x_scale = self.x_scale;
            plot = plot.label_formatter(move |name, point| {
                format_twin_hover_label(name, point, x_scale, &hover_axes, primary_domain)
            });
        } else {
            plot = plot.y_axis_label(
                egui::RichText::new(&primary_y_axis.label).size(DEFAULT_LABEL_FONT_SIZE),
            );
        }

        if self.x_scale.is_logarithmic() {
            plot = plot
                .x_grid_spacer(log10_grid_marks)
                .x_axis_formatter(format_log10_tick);
        }
        if primary_y_axis.scale.is_logarithmic() {
            plot = plot
                .y_grid_spacer(log10_grid_marks)
                .y_axis_formatter(format_log10_tick);
        }
        if twin_domains.is_none()
            && (self.x_scale.is_logarithmic() || primary_y_axis.scale.is_logarithmic())
        {
            let x_scale = self.x_scale;
            let y_scale = primary_y_axis.scale;
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

            self.apply_bounds(
                plot_ui,
                x_limits,
                y_limits,
                y_limits_pending || twin_domains_refreshed,
                changed_scales,
            );
            if let Some(heatmap) = &self.heatmap {
                let [left, right] = heatmap.x_bounds;
                let [bottom, top] = heatmap.y_bounds;
                plot_ui.image(egui_plot::PlotImage::new(
                    "",
                    heatmap.texture(plot_ui.ctx()),
                    PlotPoint::new(left + (right - left) / 2.0, bottom + (top - bottom) / 2.0),
                    egui::vec2((right - left) as f32, (top - bottom) as f32),
                ));
                heatmap.hover(plot_ui);
            }
            self.add_series(plot_ui, series_segments);
            self.add_horizontal_lines(plot_ui, horizontal_lines);
            self.add_vertical_lines(plot_ui, vertical_lines);
            self.add_imported_series(plot_ui, imported);
        });
        ui.set_style(original_style);

        if let Some(label) = mirrored_right_label {
            paint_mirrored_right_y_label(ui, response.response.rect, axes_rect, &label);
        }

        response.response.rect
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heatmap_auto_bounds_and_navigation_are_preserved() {
        let mut plot = Plotter::new();
        plot.heatmap(vec![1.0], 1, 1, [10.0, 20.0], [30.0, 50.0]);
        let context = egui::Context::default();
        let mut plot_id = egui::Id::NULL;
        let mut draw = || {
            let _ = context.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    plot_id = ui.make_persistent_id(egui::Id::new("heatmap_bounds"));
                    plot.show_plot_ui(ui, "heatmap_bounds", &[]);
                });
            });
            plot_id
        };
        let id = draw();
        let mut memory = PlotMemory::load(&context, id).unwrap();
        assert!(memory.bounds().min()[0] <= 10.0 && memory.bounds().max()[0] >= 20.0);
        assert!(memory.bounds().min()[1] <= 30.0 && memory.bounds().max()[1] >= 50.0);
        let navigated = egui_plot::PlotBounds::from_min_max([12.0, 35.0], [15.0, 40.0]);
        memory.set_bounds(navigated);
        memory.auto_bounds = egui::Vec2b::FALSE;
        memory.store(&context, id);
        draw();
        assert_eq!(*PlotMemory::load(&context, id).unwrap().bounds(), navigated);
    }

    #[test]
    fn heatmap_extent_sets_primary_twin_domain() {
        let mut plot = Plotter::new();
        plot.heatmap(vec![1.0], 1, 1, [10.0, 20.0], [30.0, 50.0]);
        let secondary = plot.add_y_axis(YAxisSide::Right);
        plot.plot(&[10.0, 20.0], &[100.0, 200.0]).y_axis(secondary);
        let domains = plot.candidate_y_domains(&[]);
        assert_eq!(domains[0].range(), (30.0, 50.0));
        assert_eq!(domains[1].range(), (100.0, 200.0));
    }

    #[test]
    fn series_ids_update_existing_data_without_changing_its_identity() {
        let mut plot = Plotter::new();
        let first = plot.add_points(vec![[0.0, 1.0]]).id();
        let second = plot.add_points(vec![[10.0, 11.0]]).id();

        assert_ne!(first, second);

        plot.append_points(first, [[1.0, 2.0], [2.0, 3.0]]);
        assert_eq!(
            plot.series[first.0].points,
            vec![[0.0, 1.0], [1.0, 2.0], [2.0, 3.0]]
        );
        assert_eq!(plot.series[second.0].points, vec![[10.0, 11.0]]);

        plot.replace_points(first, vec![[4.0, 5.0]]);
        assert_eq!(plot.series[first.0].points, vec![[4.0, 5.0]]);

        plot.clear_points(first);
        assert!(plot.series[first.0].points.is_empty());
    }

    #[test]
    fn series_can_be_rendered_as_points_without_changing_its_identity() {
        let mut plot = Plotter::new();
        let series = plot.add_points(vec![[1.0, 2.0]]).points_only().id();

        assert_eq!(plot.series[series.0].style, SeriesStyle::Points);

        plot.replace_points(series, vec![[3.0, 4.0]]);

        assert_eq!(plot.series[series.0].style, SeriesStyle::Points);
        assert_eq!(plot.series[series.0].points, vec![[3.0, 4.0]]);
    }

    #[test]
    fn twin_axes_map_series_and_reference_lines_into_the_primary_domain() {
        let mut plot = Plotter::new();
        plot.ylim(0.0, 100.0);
        plot.add_points(vec![[0.0, 0.0], [1.0, 50.0], [2.0, 100.0]]);

        let right = plot.add_y_axis(YAxisSide::Right);
        plot.y_axis_mut(right)
            .label("Pressure (kPa)")
            .limits(950.0, 1050.0);
        plot.add_points(vec![[0.0, 950.0], [1.0, 1000.0], [2.0, 1050.0]])
            .y_axis(right);
        plot.axhline(1000.0).y_axis(right);

        let domains = plot.candidate_y_domains(&[]);
        let render_data = plot.transform_for_twin_axes(&domains);

        assert_eq!(domains[0], AxisDomain::new(0.0, 100.0, "test"));
        assert_eq!(domains[1], AxisDomain::new(950.0, 1050.0, "test"));
        assert_eq!(
            render_data.series_segments[1],
            vec![vec![[0.0, 0.0], [1.0, 50.0], [2.0, 100.0]]]
        );
        assert_eq!(render_data.horizontal_lines, vec![50.0]);
        assert_eq!(render_data.y_limits, Some((0.0, 100.0)));
    }

    #[test]
    fn twin_axes_transform_each_scale_before_mapping() {
        let mut plot = Plotter::new();
        plot.ylim(-1.0, 1.0);
        let right = plot.add_y_axis(YAxisSide::Right);
        plot.y_axis_mut(right)
            .limits(1.0, 100.0)
            .scale(AxisScale::Log10);
        plot.add_points(vec![[0.0, 1.0], [1.0, 10.0], [2.0, 100.0]])
            .y_axis(right);

        let domains = plot.candidate_y_domains(&[]);
        let render_data = plot.transform_for_twin_axes(&domains);

        assert_eq!(domains[1], AxisDomain::new(0.0, 2.0, "test"));
        assert_eq!(
            render_data.series_segments[0],
            vec![vec![[0.0, -1.0], [1.0, 0.0], [2.0, 1.0]]]
        );
    }

    #[test]
    fn mapped_ticks_round_to_their_effective_axis_step() {
        let mark = GridMark {
            value: 1.0,
            step_size: 1.0,
        };

        assert_eq!(
            format_mapped_tick(
                mark,
                AxisDomain::new(0.0, 3.0, "test"),
                AxisDomain::new(0.0, 1.0, "test"),
                AxisScale::Linear,
            ),
            "0.3"
        );
        assert_eq!(format_tick_value(100.242424, 0.5), "100.2");
    }

    #[test]
    fn imported_series_contribute_to_the_primary_axis_domain() {
        let mut plot = Plotter::new();
        let right = plot.add_y_axis(YAxisSide::Right);
        plot.y_axis_mut(right).limits(950.0, 1050.0);
        let imported = ImportedSeries {
            id: 1,
            label: "CSV values".to_owned(),
            source_name: "values.csv".to_owned(),
            points: vec![[0.0, 10.0], [1.0, 20.0]],
        };

        let domains = plot.candidate_y_domains(&[imported]);

        assert_eq!(domains[0], AxisDomain::new(10.0, 20.0, "test"));
        assert_eq!(domains[1], AxisDomain::new(950.0, 1050.0, "test"));
    }

    #[test]
    fn automatic_twin_domains_follow_data_until_the_user_navigates() {
        fn input(time: f64, events: Vec<egui::Event>) -> egui::RawInput {
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                time: Some(time),
                events,
                ..Default::default()
            }
        }

        fn render(
            ctx: &egui::Context,
            plot: &Plotter,
            input: egui::RawInput,
        ) -> (egui::Id, PlotMemory) {
            let mut id = egui::Id::NULL;
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    id = ui.make_persistent_id(egui::Id::new("automatic-twin-test"));
                    plot.show_ui(ui, "automatic-twin-test");
                });
            });
            (id, PlotMemory::load(ctx, id).unwrap())
        }

        fn domains(ctx: &egui::Context, id: egui::Id) -> Vec<AxisDomain> {
            ctx.data_mut(|data| {
                data.get_temp::<TwinAxesMemory>(id.with("myplotlib_twin_y_axes"))
                    .unwrap()
                    .domains
            })
        }

        let ctx = egui::Context::default();
        let mut plot = Plotter::new();
        plot.add_points(vec![[0.0, 0.0], [1.0, 100.0]]);
        let right = plot.add_y_axis(YAxisSide::Right);
        let right_series = plot
            .add_points(vec![[0.0, 950.0], [1.0, 1050.0]])
            .y_axis(right)
            .id();

        let (id, first) = render(&ctx, &plot, input(0.0, vec![]));
        assert_eq!(domains(&ctx, id)[right.0].upper, 1050.0);

        plot.append_points(right_series, [[2.0, 1100.0]]);
        let (_, updated) = render(&ctx, &plot, input(0.1, vec![]));
        assert_eq!(domains(&ctx, id)[right.0].upper, 1100.0);

        let position = updated.transform().frame().center();
        render(
            &ctx,
            &plot,
            input(0.2, vec![egui::Event::PointerMoved(position)]),
        );
        let (_, zoomed) = render(&ctx, &plot, input(0.3, vec![egui::Event::Zoom(2.0)]));
        assert!(!zoomed.auto_bounds.y);

        plot.append_points(right_series, [[3.0, 1200.0]]);
        render(&ctx, &plot, input(0.4, vec![]));
        assert_eq!(domains(&ctx, id)[right.0].upper, 1100.0);
        assert!(first.auto_bounds.y);
    }

    #[test]
    fn point_series_filter_values_outside_logarithmic_axes() {
        let mut plot = Plotter::new();
        plot.add_points(vec![[1.0, 10.0], [10.0, 0.0], [100.0, 1000.0]])
            .points_only();
        plot.xscale(AxisScale::Log10);
        plot.yscale(AxisScale::Log10);

        let render_data = plot.transform_for_rendering();

        assert_eq!(
            render_data.series_segments,
            vec![vec![vec![[0.0, 1.0], [2.0, 3.0]]]]
        );
    }

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
