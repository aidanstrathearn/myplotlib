//! Axis configuration, coordinate transforms, and automatic domains.

use super::{Plotter, RenderData, SeriesStyle};
use crate::{Points, data_import::ImportedSeries};
use egui_plot::{GridInput, GridMark};

const MIN_LOG10_EXPONENT: f64 = -323.0;
const MAX_LOG10_EXPONENT: f64 = 308.0;

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
pub struct YAxisId(pub(super) usize);

impl YAxisId {
    pub(super) const PRIMARY: Self = Self(0);
}

/// Configuration for one y-axis.
pub struct YAxis {
    pub(super) side: YAxisSide,
    pub(super) label: String,
    pub(super) limits: Option<(f64, f64)>,
    pub(super) limits_pending: std::cell::Cell<bool>,
    pub(super) scale: AxisScale,
}

impl YAxis {
    pub(super) fn primary() -> Self {
        Self {
            side: YAxisSide::Left,
            label: String::new(),
            limits: None,
            limits_pending: std::cell::Cell::new(false),
            scale: AxisScale::Linear,
        }
    }

    pub(super) fn new(side: YAxisSide) -> Self {
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
    pub(super) fn forward(self, value: f64) -> Option<f64> {
        match self {
            Self::Linear => Some(value),
            Self::Log10 if value.is_finite() && value > 0.0 => Some(value.log10()),
            Self::Log10 => None,
        }
    }

    pub(super) fn inverse(self, value: f64) -> f64 {
        match self {
            Self::Linear => value,
            Self::Log10 => 10.0_f64.powf(value),
        }
    }

    pub(super) fn transform_required(self, value: f64, description: &str) -> f64 {
        self.forward(value).unwrap_or_else(|| {
            panic!("{description} must be finite and greater than zero on a log10 axis")
        })
    }

    pub(super) fn is_logarithmic(self) -> bool {
        self == Self::Log10
    }
}

fn transform_line(
    points: &Points,
    x_scale: AxisScale,
    y_scale: AxisScale,
    y_gain: f64,
    y_offset: f64,
) -> Vec<Points> {
    if !x_scale.is_logarithmic() && !y_scale.is_logarithmic() {
        return vec![if y_gain == 1.0 && y_offset == 0.0 {
            points.clone()
        } else {
            points
                .iter()
                .map(|&[x, y]| [x, y_gain * y + y_offset])
                .collect()
        }];
    }

    let mut segments = Vec::new();
    let mut current_segment = Vec::new();

    for &[x, y] in points {
        let transformed = x_scale
            .forward(x)
            .zip(y_scale.forward(y_gain * y + y_offset))
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

pub(super) fn transform_points(
    points: &Points,
    x_scale: AxisScale,
    y_scale: AxisScale,
    y_gain: f64,
    y_offset: f64,
) -> Points {
    points
        .iter()
        .filter_map(|&[x, y]| {
            x_scale
                .forward(x)
                .zip(y_scale.forward(y_gain * y + y_offset))
                .map(|(x, y)| [x, y])
                .filter(|[x, y]| x.is_finite() && y.is_finite())
        })
        .collect()
}

pub(super) fn transform_error_interval(
    y: f64,
    uncertainty: f64,
    scale: AxisScale,
) -> Option<[f64; 2]> {
    let lower = scale.forward(y - uncertainty)?;
    let upper = scale.forward(y + uncertainty)?;
    (lower.is_finite() && upper.is_finite()).then_some([lower, upper])
}

pub(super) fn log10_grid_marks(input: GridInput) -> Vec<GridMark> {
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct AxisDomain {
    lower: f64,
    upper: f64,
}

impl AxisDomain {
    const FALLBACK: Self = Self {
        lower: 0.0,
        upper: 1.0,
    };

    pub(super) fn new(lower: f64, upper: f64, description: &str) -> Self {
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

    pub(super) fn map_to(self, value: f64, target: Self) -> f64 {
        target.lower + (value - self.lower) * target.span() / self.span()
    }

    pub(super) fn range(self) -> (f64, f64) {
        (self.lower, self.upper)
    }

    pub(super) fn approximately_matches(self, range: std::ops::RangeInclusive<f64>) -> bool {
        let tolerance = self.lower.abs().max(self.upper.abs()).max(1.0) * f64::EPSILON * 16.0;
        (*range.start() - self.lower).abs() <= tolerance
            && (*range.end() - self.upper).abs() <= tolerance
    }
}

#[derive(Clone, Debug, PartialEq)]
struct YAxisSignature {
    side: YAxisSide,
    scale: AxisScale,
    limits: Option<(f64, f64)>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct YAxesSignature(Vec<YAxisSignature>);

#[derive(Clone)]
pub(super) struct TwinAxesMemory {
    pub(super) signature: YAxesSignature,
    pub(super) domains: Vec<AxisDomain>,
}

impl Plotter {
    pub(super) fn transform_for_rendering(&self) -> RenderData {
        let y_axis = self.primary_y_axis();
        let series_segments = self
            .series
            .iter()
            .map(|series| {
                self.y_axis(series.y_axis);
                match series.style {
                    SeriesStyle::Line => transform_line(
                        &series.points,
                        self.x_scale,
                        y_axis.scale,
                        self.y_gain,
                        self.y_offset,
                    ),
                    SeriesStyle::Points => vec![transform_points(
                        &series.points,
                        self.x_scale,
                        y_axis.scale,
                        self.y_gain,
                        self.y_offset,
                    )],
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

    pub(super) fn y_axes_signature(&self) -> YAxesSignature {
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

    pub(super) fn candidate_y_domains(&self, imported: &[ImportedSeries]) -> Vec<AxisDomain> {
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
                include(series.y_axis, self.y_gain * point[1] + self.y_offset);
            }
        }
        for line in &self.horizontal_lines {
            let axis = line.y_axis.expect("horizontal lines have a y-axis");
            include(axis, line.value);
        }
        for series in imported {
            for (index, point) in series.points.iter().enumerate() {
                include(YAxisId::PRIMARY, point[1]);
                if let Some(uncertainties) = &series.y_uncertainties
                    && self.x_scale.forward(point[0]).is_some()
                    && transform_error_interval(
                        point[1],
                        uncertainties[index],
                        self.primary_y_axis().scale,
                    )
                    .is_some()
                {
                    include(YAxisId::PRIMARY, point[1] - uncertainties[index]);
                    include(YAxisId::PRIMARY, point[1] + uncertainties[index]);
                }
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

    pub(super) fn transform_for_twin_axes(&self, domains: &[AxisDomain]) -> RenderData {
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
                        let mut segments = transform_line(
                            &series.points,
                            self.x_scale,
                            axis.scale,
                            self.y_gain,
                            self.y_offset,
                        );
                        for segment in &mut segments {
                            map_points(segment, series.y_axis);
                        }
                        segments
                    }
                    SeriesStyle::Points => {
                        let mut points = transform_points(
                            &series.points,
                            self.x_scale,
                            axis.scale,
                            self.y_gain,
                            self.y_offset,
                        );
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui;
    use egui_plot::PlotMemory;

    #[test]
    fn y_calibration_precedes_log_scaling_and_twin_axis_domain_mapping() {
        let mut plot = Plotter::new();
        plot.yscale(AxisScale::Log10);
        plot.add_points(vec![[0.0, -4.5], [1.0, 0.0]]);
        plot.y_gain(2.0);
        plot.y_offset(10.0);
        assert_eq!(
            plot.transform_for_rendering().series_segments[0],
            vec![vec![[0.0, 0.0], [1.0, 1.0]]]
        );
        let right = plot.add_y_axis(YAxisSide::Right);
        plot.add_points(vec![[0.0, 0.0], [1.0, 5.0]]).y_axis(right);
        let domains = plot.candidate_y_domains(&[]);
        assert_eq!(domains[0].range(), (0.0, 1.0));
        assert_eq!(domains[1].range(), (10.0, 20.0));
        assert_eq!(
            plot.transform_for_twin_axes(&domains).series_segments[1],
            vec![vec![[0.0, 0.0], [1.0, 1.0]]]
        );
    }

    #[test]
    fn y_calibration_keeps_imported_points_and_heatmap_bounds_in_their_original_units() {
        let mut plot = Plotter::new();
        plot.add_points(vec![[0.0, 1.0], [1.0, 2.0]]);
        plot.y_gain(2.0);
        plot.y_offset(10.0);
        let imported = ImportedSeries {
            id: 1,
            label: "Experiment".to_owned(),
            source_name: "experiment.csv".to_owned(),
            points: vec![[0.0, 20.0], [1.0, 30.0]],
            y_uncertainties: None,
        };

        assert_eq!(
            plot.candidate_y_domains(std::slice::from_ref(&imported))[0],
            AxisDomain::new(12.0, 30.0, "test")
        );
        plot.heatmap(vec![1.0], 1, 1, [0.0, 1.0], [-3.0, -2.0]);
        assert_eq!(
            plot.candidate_y_domains(&[imported])[0],
            AxisDomain::new(-3.0, 30.0, "test")
        );
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
    fn imported_series_contribute_to_the_primary_axis_domain() {
        let mut plot = Plotter::new();
        let right = plot.add_y_axis(YAxisSide::Right);
        plot.y_axis_mut(right).limits(950.0, 1050.0);
        let imported = ImportedSeries {
            id: 1,
            label: "CSV values".to_owned(),
            source_name: "values.csv".to_owned(),
            points: vec![[0.0, 10.0], [1.0, 20.0]],
            y_uncertainties: Some(vec![2.0, 5.0]),
        };

        let domains = plot.candidate_y_domains(&[imported]);

        assert_eq!(domains[0], AxisDomain::new(8.0, 25.0, "test"));
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
        let segments = transform_line(&points, AxisScale::Log10, AxisScale::Log10, 1.0, 0.0);

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
    #[should_panic(expected = "x limits must be finite and greater than zero on a log10 axis")]
    fn rejects_nonpositive_log_limits() {
        AxisScale::Log10.transform_required(0.0, "x limits");
    }
}
