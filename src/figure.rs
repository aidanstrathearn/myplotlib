use crate::Plotter;
#[cfg(any(not(target_arch = "wasm32"), test))]
use eframe::egui;

pub struct Figure {
    rows: usize,
    columns: usize,
    axes: Vec<Plotter>,
    title: String,
}

impl Figure {
    pub fn subplots(rows: usize, columns: usize) -> Self {
        assert!(rows > 0, "a figure must contain at least one row");
        assert!(columns > 0, "a figure must contain at least one column");

        let axes = (0..rows * columns).map(|_| Plotter::new()).collect();

        Self {
            rows,
            columns,
            axes,
            title: String::new(),
        }
    }

    #[cfg(any(not(target_arch = "wasm32"), test))]
    pub(crate) fn from_plotter(plotter: Plotter) -> Self {
        Self {
            rows: 1,
            columns: 1,
            axes: vec![plotter],
            title: String::new(),
        }
    }

    pub fn axes(&self, row: usize, column: usize) -> &Plotter {
        let index = self.axes_index(row, column);
        &self.axes[index]
    }

    pub fn axes_mut(&mut self, row: usize, column: usize) -> &mut Plotter {
        let index = self.axes_index(row, column);
        &mut self.axes[index]
    }

    pub fn suptitle(&mut self, title: impl Into<String>) {
        self.title = title.into();
    }

    fn axes_index(&self, row: usize, column: usize) -> usize {
        assert!(row < self.rows, "subplot row {row} is out of bounds");
        assert!(
            column < self.columns,
            "subplot column {column} is out of bounds"
        );
        row * self.columns + column
    }

    /// Opens a native window containing every subplot and blocks until it is closed.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn show(self) -> crate::Result {
        let window_title = if !self.title.is_empty() {
            self.title.clone()
        } else if self.axes.len() == 1 && !self.axes[0].title.is_empty() {
            self.axes[0].title.clone()
        } else {
            "Plot".to_owned()
        };

        let initial_width = (500.0 * self.columns as f32).clamp(800.0, 1600.0);
        let initial_height = (400.0 * self.rows as f32).clamp(600.0, 1200.0);

        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([initial_width, initial_height])
                .with_min_inner_size([400.0, 300.0])
                .with_resizable(true),
            ..Default::default()
        };

        eframe::run_native(
            &window_title,
            options,
            Box::new(move |creation_context| {
                creation_context
                    .egui_ctx
                    .set_visuals(egui::Visuals::light());

                Ok(Box::new(FigureApp::new(self)))
            }),
        )
    }
}

#[cfg(any(not(target_arch = "wasm32"), test))]
struct FigureApp {
    figure: Figure,
    plot_rects: Vec<egui::Rect>,
}

#[cfg(any(not(target_arch = "wasm32"), test))]
impl FigureApp {
    fn new(figure: Figure) -> Self {
        let plot_rects = vec![egui::Rect::NOTHING; figure.axes.len()];

        Self { figure, plot_rects }
    }

    fn draw(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.figure.title.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.heading(&self.figure.title);
                });

                ui.add_space(8.0);
            }

            let spacing = 8.0;
            let available_size = ui.available_size();
            let total_horizontal_spacing = spacing * self.figure.columns.saturating_sub(1) as f32;
            let total_vertical_spacing = spacing * self.figure.rows.saturating_sub(1) as f32;
            let cell_width =
                (available_size.x - total_horizontal_spacing) / self.figure.columns as f32;
            let cell_height = (available_size.y - total_vertical_spacing) / self.figure.rows as f32;
            let (figure_rect, _) = ui.allocate_exact_size(available_size, egui::Sense::hover());

            for (index, plotter) in self.figure.axes.iter().enumerate() {
                let row = index / self.figure.columns;
                let column = index % self.figure.columns;
                let cell_min = egui::pos2(
                    figure_rect.min.x + column as f32 * (cell_width + spacing),
                    figure_rect.min.y + row as f32 * (cell_height + spacing),
                );
                let cell_rect =
                    egui::Rect::from_min_size(cell_min, egui::vec2(cell_width, cell_height));
                let mut cell_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt(("subplot_cell", index))
                        .max_rect(cell_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                cell_ui.set_clip_rect(cell_rect);

                self.plot_rects[index] = plotter.show_ui(&mut cell_ui, ("subplot", index));
            }
        });
    }
}

#[cfg(any(not(target_arch = "wasm32"), test))]
impl eframe::App for FigureApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.draw(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_labeled_and_unlabeled_series_with_a_reference_line() {
        let x = [0.0, 1.0, 2.0];
        let increasing = [0.0, 1.0, 4.0];
        let decreasing = [4.0, 1.0, 0.0];

        let mut plotter = Plotter::new();
        plotter.plot(&x, &increasing).label("Increasing");
        plotter.plot(&x, &decreasing).label("Decreasing");
        plotter.plot(&x, &increasing);
        plotter
            .axvline(1.0)
            .label(format!("Reference (x = {:.3e})", 1.0));
        plotter.xlabel("x");
        plotter.ylabel("y");
        plotter.title("Plot rendering test");

        let mut app = FigureApp::new(Figure::from_plotter(plotter));
        let context = egui::Context::default();
        let output = context.run(Default::default(), |context| app.draw(context));

        assert!(!output.shapes.is_empty());
    }

    #[test]
    fn renders_a_grid_of_independent_subplots() {
        let x = [0.0, 1.0, 2.0];
        let y = [0.0, 1.0, 4.0];

        let mut figure = Figure::subplots(2, 2);
        figure.suptitle("Four plots");

        for row in 0..2 {
            for column in 0..2 {
                let axes = figure.axes_mut(row, column);
                axes.plot(&x, &y).label(format!("Series {row},{column}"));
                axes.title(format!("Plot {row},{column}"));
            }
        }

        let mut app = FigureApp::new(figure);
        let context = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 800.0),
            )),
            ..Default::default()
        };
        let output = context.run(input, |context| app.draw(context));

        assert!(!output.shapes.is_empty());
        assert_eq!(app.plot_rects.len(), 4);

        for rect in &app.plot_rects {
            assert!(rect.width() > 400.0, "subplot is too narrow: {rect:?}");
            assert!(rect.height() > 200.0, "subplot is too short: {rect:?}");
        }

        assert!(app.plot_rects[0].center().x < app.plot_rects[1].center().x);
        assert!(app.plot_rects[0].center().y < app.plot_rects[2].center().y);
    }

    #[test]
    #[should_panic(expected = "subplot column 2 is out of bounds")]
    fn rejects_out_of_bounds_subplot_coordinates() {
        let figure = Figure::subplots(2, 2);
        let _ = figure.axes(0, 2);
    }
}
