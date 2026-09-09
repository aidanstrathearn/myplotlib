mod app;
#[cfg(not(target_arch = "wasm32"))]
mod appmenu;
mod controls;
mod figure;
mod plotter;

pub use app::{App, AppDefinition, AppError, AppResult, ControlsAction, PlotAction, ViewOption};
#[cfg(not(target_arch = "wasm32"))]
pub use app::{NativeResult, run_native};
#[cfg(target_arch = "wasm32")]
pub use app::{WebResult, run_web};
#[cfg(not(target_arch = "wasm32"))]
pub use appmenu::AppMenu;
pub use controls::{Slider, SliderGrid, SliderGroup};
pub use figure::Figure;
pub use plotter::{PlotLine, Plotter, ReferenceLine};

pub type Points = Vec<[f64; 2]>;
pub type Result<T = ()> = eframe::Result<T>;
