//! Imported measurements drawn as markers with capped uncertainty intervals.

use super::axes::transform_error_interval;
use super::formatting::format_value;
use super::{AxisScale, DEFAULT_MARKER_RADIUS};
use crate::{Points, data_import::ImportedSeries};
use eframe::egui;
use egui_plot::{
    ClosestElem, Cursor, LabelFormatter, PlotBounds, PlotConfig, PlotGeometry, PlotItem,
    PlotItemBase, PlotPoint, PlotPoints, PlotTransform, Points as PlotMarkers,
};

const ERROR_BAR_CAP_HALF_WIDTH: f32 = 4.0;
const ERROR_BAR_LINE_WIDTH: f32 = 1.5;

struct ImportedErrorBar {
    original: [f64; 2],
    uncertainty: f64,
    center: PlotPoint,
    // A valid marker can remain visible when its interval crosses zero on a log axis.
    interval: Option<[f64; 2]>,
}

/// Keeps markers and their error bars in one item for bounds, legend, and hover handling.
pub(super) struct ImportedErrorBars {
    markers: PlotMarkers<'static>,
    bars: Vec<ImportedErrorBar>,
}

impl ImportedErrorBars {
    pub(super) fn new(
        series: &ImportedSeries,
        uncertainties: &[f64],
        x_scale: AxisScale,
        y_scale: AxisScale,
        colour: egui::Color32,
    ) -> Self {
        let bars = series
            .points
            .iter()
            .zip(uncertainties)
            .filter_map(|(&[x, y], &uncertainty)| {
                let center = PlotPoint::new(x_scale.forward(x)?, y_scale.forward(y)?);
                (center.x.is_finite() && center.y.is_finite()).then(|| ImportedErrorBar {
                    original: [x, y],
                    uncertainty,
                    center,
                    interval: transform_error_interval(y, uncertainty, y_scale),
                })
            })
            .collect::<Vec<_>>();
        let points = bars
            .iter()
            .map(|bar| [bar.center.x, bar.center.y])
            .collect::<Points>();
        let markers = PlotMarkers::new(
            format!("imported_series_{}", series.id),
            PlotPoints::from(points),
        )
        .name(&series.label)
        .color(colour)
        .radius(DEFAULT_MARKER_RADIUS);
        Self { markers, bars }
    }

    fn hover_label(&self, index: usize) -> String {
        let bar = &self.bars[index];
        format!(
            "{}\nx = {}\ny = {} ± {}",
            self.name(),
            format_value(bar.original[0]),
            format_value(bar.original[1]),
            format_value(bar.uncertainty),
        )
    }
}

impl PlotItem for ImportedErrorBars {
    fn shapes(&self, ui: &egui::Ui, transform: &PlotTransform, shapes: &mut Vec<egui::Shape>) {
        let stroke = egui::Stroke::new(
            ERROR_BAR_LINE_WIDTH * if self.highlighted() { 2.0 } else { 1.0 },
            self.color(),
        );
        let cap = egui::vec2(ERROR_BAR_CAP_HALF_WIDTH, 0.0);
        for bar in &self.bars {
            if let Some([lower, upper]) = bar.interval {
                let lower = transform.position_from_point(&PlotPoint::new(bar.center.x, lower));
                let upper = transform.position_from_point(&PlotPoint::new(bar.center.x, upper));
                shapes.push(egui::Shape::line_segment([lower, upper], stroke));
                shapes.push(egui::Shape::line_segment(
                    [lower - cap, lower + cap],
                    stroke,
                ));
                shapes.push(egui::Shape::line_segment(
                    [upper - cap, upper + cap],
                    stroke,
                ));
            }
        }
        self.markers.shapes(ui, transform, shapes);
    }

    fn initialize(&mut self, x_range: std::ops::RangeInclusive<f64>) {
        self.markers.initialize(x_range);
    }

    fn color(&self) -> egui::Color32 {
        PlotItem::color(&self.markers)
    }

    fn geometry(&self) -> PlotGeometry<'_> {
        self.markers.geometry()
    }

    fn bounds(&self) -> PlotBounds {
        let mut bounds = self.markers.bounds();
        for bar in &self.bars {
            if let Some([lower, upper]) = bar.interval {
                bounds.extend_with(&PlotPoint::new(bar.center.x, lower));
                bounds.extend_with(&PlotPoint::new(bar.center.x, upper));
            }
        }
        bounds
    }

    fn base(&self) -> &PlotItemBase {
        self.markers.base()
    }

    fn base_mut(&mut self) -> &mut PlotItemBase {
        self.markers.base_mut()
    }

    fn find_closest(&self, point: egui::Pos2, transform: &PlotTransform) -> Option<ClosestElem> {
        self.bars
            .iter()
            .enumerate()
            .map(|(index, bar)| {
                let dist_sq = if let Some([lower, upper]) = bar.interval {
                    let lower = transform.position_from_point(&PlotPoint::new(bar.center.x, lower));
                    let upper = transform.position_from_point(&PlotPoint::new(bar.center.x, upper));
                    egui::Rect::from_two_pos(lower, upper).distance_sq_to_pos(point)
                } else {
                    point.distance_sq(transform.position_from_point(&bar.center))
                };
                ClosestElem { index, dist_sq }
            })
            .min_by(|left, right| left.dist_sq.total_cmp(&right.dist_sq))
    }

    fn on_hover(
        &self,
        response: &egui::Response,
        elem: ClosestElem,
        shapes: &mut Vec<egui::Shape>,
        cursors: &mut Vec<Cursor>,
        plot: &PlotConfig<'_>,
        _label_formatter: &LabelFormatter<'_>,
    ) {
        let index = elem.index;
        let formatter: LabelFormatter<'_> = Some(Box::new(move |_, _| self.hover_label(index)));
        self.markers
            .on_hover(response, elem, shapes, cursors, plot, &formatter);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{MATPLOTLIB_COLORS, Plotter};
    use super::*;

    fn uncertain_series(points: Points, uncertainties: Vec<f64>) -> ImportedSeries {
        ImportedSeries {
            id: 1,
            label: "ratio".to_owned(),
            source_name: "measurements.csv".to_owned(),
            points,
            y_uncertainties: Some(uncertainties),
        }
    }

    #[test]
    fn error_bar_bounds_and_hover_use_the_original_measurements() {
        let series = uncertain_series(vec![[1.0, 10.0], [2.0, 20.0]], vec![2.0, 5.0]);
        let item = ImportedErrorBars::new(
            &series,
            series.y_uncertainties.as_ref().unwrap(),
            AxisScale::Linear,
            AxisScale::Linear,
            MATPLOTLIB_COLORS[0],
        );
        assert_eq!(item.bounds().min(), [1.0, 8.0]);
        assert_eq!(item.bounds().max(), [2.0, 25.0]);
        assert_eq!(item.hover_label(1), "ratio\nx = 2\ny = 20 ± 5");

        let mut plot = Plotter::new();
        plot.y_gain(100.0);
        plot.y_offset(1000.0);
        assert_eq!(plot.candidate_y_domains(&[series])[0].range(), (8.0, 25.0),);
    }

    #[test]
    fn logarithmic_error_bars_transform_endpoints_and_preserve_point_alignment() {
        let series = uncertain_series(
            vec![[1.0, 10.0], [0.0, 100.0], [10.0, 1.0], [100.0, -1.0]],
            vec![9.0, 50.0, 2.0, 0.5],
        );
        let item = ImportedErrorBars::new(
            &series,
            series.y_uncertainties.as_ref().unwrap(),
            AxisScale::Log10,
            AxisScale::Log10,
            MATPLOTLIB_COLORS[0],
        );
        assert_eq!(item.bars.len(), 2);
        assert_eq!(item.bars[0].interval, Some([0.0, 19.0_f64.log10()]));
        assert_eq!(item.bars[1].center, PlotPoint::new(1.0, 0.0));
        assert_eq!(item.bars[1].interval, None);
        assert_eq!(item.hover_label(1), "ratio\nx = 10\ny = 1 ± 2");
        assert_eq!(item.bounds().min(), [0.0, 0.0]);
        assert_eq!(item.bounds().max(), [1.0, 19.0_f64.log10()]);

        let mut plot = Plotter::new();
        plot.yscale(AxisScale::Log10);
        let series = uncertain_series(vec![[1.0, 1.0], [2.0, 10.0]], vec![2.0, 9.0]);
        assert_eq!(
            plot.candidate_y_domains(&[series])[0].range(),
            (0.0, 19.0_f64.log10()),
        );
    }

    #[test]
    fn error_bar_caps_keep_their_screen_width_when_zooming_and_stems_can_be_hovered() {
        let series = uncertain_series(vec![[1.0, 10.0], [2.0, 20.0]], vec![8.0, 2.0]);
        let item = ImportedErrorBars::new(
            &series,
            series.y_uncertainties.as_ref().unwrap(),
            AxisScale::Linear,
            AxisScale::Linear,
            MATPLOTLIB_COLORS[0],
        );
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                for bounds in [
                    PlotBounds::from_min_max([0.0, 0.0], [3.0, 30.0]),
                    PlotBounds::from_min_max([0.5, 5.0], [2.5, 25.0]),
                ] {
                    let transform = PlotTransform::new(
                        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
                        bounds,
                        false,
                    );
                    let mut shapes = Vec::new();
                    item.shapes(ui, &transform, &mut shapes);
                    let caps = shapes
                        .iter()
                        .filter_map(|shape| match shape {
                            egui::Shape::LineSegment { points, .. }
                                if points[0].y == points[1].y =>
                            {
                                Some(points)
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(caps.len(), 4);
                    for cap in caps {
                        assert_eq!(cap[1].x - cap[0].x, 2.0 * ERROR_BAR_CAP_HALF_WIDTH);
                    }
                    let near_upper_end = transform.position_from_point(&PlotPoint::new(1.0, 17.0));
                    let closest = item.find_closest(near_upper_end, &transform).unwrap();
                    assert_eq!(closest.index, 0);
                    assert_eq!(closest.dist_sq, 0.0);
                }
            });
        });
    }
}
