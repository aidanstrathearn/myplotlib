//! egui plot construction, drawing, and navigation bounds.

use super::axes::{TwinAxesMemory, log10_grid_marks, transform_points};
use super::error_bars::ImportedErrorBars;
use super::formatting::{
    format_hover_label, format_log10_tick, format_mapped_tick, format_twin_hover_label,
};
use super::{
    AxisScale, DEFAULT_MARKER_RADIUS, MATPLOTLIB_COLORS, Plotter, RenderData, SeriesStyle, YAxisId,
    YAxisSide,
};
use crate::{Points, data_import::ImportedSeries};
use eframe::egui;
use egui_plot::{
    AxisHints, HLine, HPlacement, Legend, Line, LineStyle, Plot, PlotMemory, PlotPoint, PlotPoints,
    PlotUi, Points as PlotMarkers, VLine,
};

const DEFAULT_LINE_WIDTH: f32 = 3.0;
const DEFAULT_LABEL_FONT_SIZE: f32 = 24.0;

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

impl Plotter {
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
            if let Some(uncertainties) = &series.y_uncertainties {
                plot_ui.add(ImportedErrorBars::new(
                    series,
                    uncertainties,
                    self.x_scale,
                    self.primary_y_axis().scale,
                    colour,
                ));
                continue;
            }
            let points = transform_points(
                &series.points,
                self.x_scale,
                self.primary_y_axis().scale,
                1.0,
                0.0,
            );
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
}
