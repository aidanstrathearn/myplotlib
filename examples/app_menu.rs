#[cfg(not(target_arch = "wasm32"))]
fn main() -> myplotlib::NativeResult {
    use myplotlib::{
        AppDefinition, AppMenu, AppResult, Figure, Plotter, Slider, SliderGrid, SliderGroup,
        ViewOption,
    };

    struct Params {
        amplitude: f64,
    }

    impl Default for Params {
        fn default() -> Self {
            Self { amplitude: 1.0 }
        }
    }

    fn controls(params: &mut Params) -> SliderGrid<'_> {
        SliderGrid::new(
            1,
            [SliderGroup::new(
                "Wave",
                [Slider::new("Amplitude", &mut params.amplitude, 0.0..=3.0)],
            )],
        )
    }

    fn wave(params: &mut Params) -> AppResult {
        let x: Vec<_> = (0..=200).map(|i| i as f64 * 0.05).collect();
        let y: Vec<_> = x.iter().map(|&x| params.amplitude * x.sin()).collect();
        let mut plot = Plotter::new();
        plot.plot(&x, &y).label("Sine");
        Ok(plot)
    }

    fn figure() -> Figure {
        let x: Vec<_> = (0..=200).map(|i| i as f64 * 0.05).collect();
        let mut figure = Figure::subplots(1, 2);
        figure.suptitle("Sine and cosine");
        figure
            .axes_mut(0, 0)
            .plot(&x, &x.iter().map(|x| x.sin()).collect::<Vec<_>>())
            .label("Sine");
        figure
            .axes_mut(0, 1)
            .plot(&x, &x.iter().map(|x| x.cos()).collect::<Vec<_>>())
            .label("Cosine");
        figure
    }

    const VIEWS: &[ViewOption<Params>] = &[ViewOption::new("Wave", wave, controls)];
    AppMenu::new("Plot examples")
        .app(
            "Adjustable wave",
            AppDefinition::new("Wave", "wave-canvas", VIEWS),
        )
        .figure("Two subplots", figure)
        .run_native()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
