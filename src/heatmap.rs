use eframe::egui;
use std::cell::RefCell;

/// A uniformly spaced scalar grid. Row zero is the bottom row; bounds are
/// the outer cell edges. Nonfinite values are transparent.
///
/// Heatmaps use the primary y-axis and require linear spatial axes.
pub struct Heatmap {
    values: Vec<f64>,
    rows: usize,
    columns: usize,
    pub(crate) x_bounds: [f64; 2],
    pub(crate) y_bounds: [f64; 2],
    limits: (f64, f64),
    label: String,
    // Keep the handle alive, scoped to the egui context that owns the texture.
    texture: RefCell<Option<(egui::Context, egui::TextureHandle)>>,
}

impl Heatmap {
    /// Creates a grid from row-major values and increasing, finite outer bounds.
    /// Panics for empty/mismatched dimensions or invalid bounds.
    pub fn new(
        values: Vec<f64>,
        rows: usize,
        columns: usize,
        x_bounds: [f64; 2],
        y_bounds: [f64; 2],
    ) -> Self {
        assert!(
            rows > 0 && columns > 0,
            "heatmap dimensions must be nonzero"
        );
        assert_eq!(
            rows.checked_mul(columns),
            Some(values.len()),
            "heatmap dimensions must match values"
        );
        for bounds in [x_bounds, y_bounds] {
            assert!(
                bounds[0].is_finite()
                    && bounds[1].is_finite()
                    && bounds[0] < bounds[1]
                    && (bounds[1] - bounds[0]).is_finite()
                    && bounds[0] as f32 != bounds[1] as f32
                    && (bounds[1] - bounds[0]) <= f32::MAX as f64,
                "heatmap bounds must be finite, increasing, and representable by the renderer"
            );
        }
        let mut lower = f64::INFINITY;
        let mut upper = f64::NEG_INFINITY;
        for &value in &values {
            if value.is_finite() {
                lower = lower.min(value);
                upper = upper.max(value);
            }
        }
        let limits = if lower.is_finite() {
            (lower, upper)
        } else {
            (0.0, 1.0)
        };
        Self {
            values,
            rows,
            columns,
            x_bounds,
            y_bounds,
            limits,
            label: String::new(),
            texture: RefCell::new(None),
        }
    }

    /// Sets fixed colour limits. Values outside them use the endpoint colours.
    pub fn color_limits(&mut self, lower: f64, upper: f64) -> &mut Self {
        assert!(
            lower.is_finite() && upper.is_finite() && lower < upper,
            "heatmap colour limits must be finite and increasing"
        );
        self.limits = (lower, upper);
        *self.texture.get_mut() = None;
        self
    }

    /// Sets the colour bar label.
    pub fn label(&mut self, label: impl Into<String>) -> &mut Self {
        self.label = label.into();
        self
    }

    fn normalized(&self, value: f64) -> f64 {
        let (lower, upper) = self.limits;
        if lower == upper {
            return 0.5;
        }
        // Scale first to avoid overflow for limits near the ends of f64.
        let scale = lower.abs().max(upper.abs()).max(f64::MIN_POSITIVE);
        ((value / scale - lower / scale) / (upper / scale - lower / scale)).clamp(0.0, 1.0)
    }

    fn image(&self) -> egui::ColorImage {
        let pixels = (0..self.rows)
            .rev()
            .flat_map(|row| {
                (0..self.columns).map(move |column| {
                    let value = self.values[row * self.columns + column];
                    if value.is_finite() {
                        palette(self.normalized(value))
                    } else {
                        egui::Color32::TRANSPARENT
                    }
                })
            })
            .collect();
        egui::ColorImage::new([self.columns, self.rows], pixels)
    }

    pub(crate) fn texture(&self, ctx: &egui::Context) -> egui::TextureId {
        let mut cached = self.texture.borrow_mut();
        if cached.as_ref().is_none_or(|(owner, _)| owner != ctx) {
            *cached = Some((
                ctx.clone(),
                ctx.load_texture("heatmap", self.image(), egui::TextureOptions::NEAREST),
            ));
        }
        cached.as_ref().unwrap().1.id()
    }

    fn cell(&self, x: f64, y: f64) -> Option<(usize, usize, f64)> {
        if !x.is_finite()
            || !y.is_finite()
            || x < self.x_bounds[0]
            || x > self.x_bounds[1]
            || y < self.y_bounds[0]
            || y > self.y_bounds[1]
        {
            return None;
        }
        let column = (((x - self.x_bounds[0]) / (self.x_bounds[1] - self.x_bounds[0])
            * self.columns as f64) as usize)
            .min(self.columns - 1);
        let row = (((y - self.y_bounds[0]) / (self.y_bounds[1] - self.y_bounds[0])
            * self.rows as f64) as usize)
            .min(self.rows - 1);
        Some((row, column, self.values[row * self.columns + column]))
    }

    pub(crate) fn hover(&self, plot: &egui_plot::PlotUi<'_>) {
        if let Some(point) = plot
            .pointer_coordinate()
            .filter(|_| plot.response().hovered())
        {
            if let Some((row, column, value)) = self.cell(point.x, point.y) {
                plot.response().clone().on_hover_text(format!(
                    "x: {:.6}\ny: {:.6}\nrow: {row}, column: {column}\nvalue: {value}",
                    point.x, point.y
                ));
            }
        }
    }

    pub(crate) fn colorbar(&self, ui: &mut egui::Ui) {
        if !self.label.is_empty() {
            ui.label(&self.label);
        }
        ui.label(format!("{:.4e}", self.limits.1));
        let height = (ui.available_height()
            - ui.text_style_height(&egui::TextStyle::Body)
            - ui.spacing().item_spacing.y)
            .max(1.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, height), egui::Sense::hover());
        for i in 0..128 {
            let top = rect.top() + rect.height() * i as f32 / 128.0;
            let bottom = rect.top() + rect.height() * (i + 1) as f32 / 128.0;
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(rect.left(), top),
                    egui::pos2(rect.right(), bottom),
                ),
                0.0,
                palette(1.0 - i as f64 / 127.0),
            );
        }
        ui.label(format!("{:.4e}", self.limits.0));
    }
}

// A fixed sequential palette, interpolated through viridis colour anchors.
fn palette(t: f64) -> egui::Color32 {
    const COLORS: [[u8; 3]; 5] = [
        [68, 1, 84],
        [59, 82, 139],
        [33, 145, 140],
        [94, 201, 98],
        [253, 231, 37],
    ];
    let position = t.clamp(0.0, 1.0) * 4.0;
    let index = (position as usize).min(3);
    let fraction = position - index as f64;
    let channel = |c| {
        (COLORS[index][c] as f64 * (1.0 - fraction) + COLORS[index + 1][c] as f64 * fraction)
            .round() as u8
    };
    egui::Color32::from_rgb(channel(0), channel(1), channel(2))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn orientation_lookup_and_missing_values() {
        let h = Heatmap::new(vec![0.0, 1.0, 2.0, f64::NAN], 2, 2, [0.0, 2.0], [0.0, 2.0]);
        assert_eq!(h.cell(0.5, 0.5), Some((0, 0, 0.0)));
        assert_eq!(h.cell(0.5, 1.5), Some((1, 0, 2.0)));
        assert!(h.cell(-0.1, 1.0).is_none());
        assert!(h.cell(2.0, 2.0).unwrap().2.is_nan());
        assert_eq!(
            h.image().pixels,
            vec![
                palette(1.0),
                egui::Color32::TRANSPARENT,
                palette(0.0),
                palette(0.5)
            ]
        );
    }
    #[test]
    fn limits_handle_constant_missing_and_extreme_data() {
        let mut h = Heatmap::new(vec![5.0], 1, 1, [0.0, 1.0], [0.0, 1.0]);
        assert_eq!(h.normalized(5.0), 0.5);
        h.color_limits(-f64::MAX, f64::MAX);
        assert_eq!(h.normalized(0.0), 0.5);
        assert_eq!(h.normalized(f64::MAX), 1.0);
        let missing = Heatmap::new(vec![f64::NAN], 1, 1, [0.0, 1.0], [0.0, 1.0]);
        assert_eq!(missing.limits, (0.0, 1.0));
    }
    #[test]
    #[should_panic(expected = "dimensions must match")]
    fn rejects_mismatched_dimensions() {
        Heatmap::new(vec![0.0], 2, 2, [0.0, 1.0], [0.0, 1.0]);
    }
    #[test]
    #[should_panic(expected = "heatmap bounds")]
    fn rejects_reversed_bounds() {
        Heatmap::new(vec![0.0], 1, 1, [1.0, 0.0], [0.0, 1.0]);
    }
}
