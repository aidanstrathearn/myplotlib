//! Plot state and the public plotting API.

mod axes;
mod error_bars;
mod formatting;
mod render;

pub use axes::{AxisScale, YAxis, YAxisId, YAxisSide};

use crate::Points;
use eframe::egui;
use std::fmt::Display;

const DEFAULT_MARKER_RADIUS: f32 = 4.0;
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
    y_gain: f64,
    y_offset: f64,
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
            y_gain: 1.0,
            y_offset: 0.0,
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

    /// Replaces complete rows of the existing heatmap, with row zero at the bottom.
    /// Preserves bounds, colour limits, labels, and pan/zoom state. Returns an
    /// error if no heatmap exists or the update is invalid; see [`crate::Heatmap::set_rows`].
    pub fn set_heatmap_rows(
        &mut self,
        first_row: usize,
        values: &[f64],
    ) -> std::result::Result<(), crate::HeatmapUpdateError> {
        self.heatmap
            .as_mut()
            .ok_or(crate::HeatmapUpdateError::MissingHeatmap)?
            .set_rows(first_row, values)
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// Shifts all data series vertically when rendered, without changing their
    /// stored points. Streamed replacements and appends use the same offset.
    /// Imported overlays, reference lines, and heatmaps keep their coordinates.
    /// The offset must be finite and is applied after [`Self::y_gain`] and before
    /// any logarithmic scaling.
    pub fn y_offset(&mut self, offset: f64) {
        assert!(offset.is_finite(), "y offset must be finite");
        self.y_offset = offset;
    }

    /// Multiplies all data series vertically when rendered, without changing
    /// their stored points. Streamed replacements and appends use the same gain.
    /// Imported overlays, reference lines, and heatmaps keep their coordinates.
    /// The gain must be finite and defaults to 1. It is applied before the y
    /// offset and any logarithmic scaling: `displayed_y = gain * y + offset`.
    /// This amplitude calibration is independent of [`Self::yscale`], which
    /// selects a linear or logarithmic axis.
    pub fn y_gain(&mut self, gain: f64) {
        assert!(gain.is_finite(), "y gain must be finite");
        self.y_gain = gain;
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

    /// Updates the first summary with this label, or adds it if absent.
    pub fn set_summary(
        &mut self,
        label: impl Into<String>,
        value: impl Display,
    ) -> &mut SummaryItem {
        let label = label.into();
        if let Some(index) = self.summary.iter().position(|item| item.label == label) {
            let item = &mut self.summary[index];
            item.value = value.to_string();
            item
        } else {
            self.add_summary(label, value)
        }
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
}

#[cfg(test)]
mod tests;
