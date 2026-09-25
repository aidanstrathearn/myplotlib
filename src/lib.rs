mod app;
mod appmenu;
mod controls;
mod figure;
mod plotter;
mod save;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
pub use app::mount_web;
pub use app::{App, AppDefinition, AppError, AppResult, ControlsAction, PlotAction, ViewOption};
#[cfg(not(target_arch = "wasm32"))]
pub use app::{NativeResult, run_native};
pub use appmenu::AppMenu;
pub use controls::{Slider, SliderGrid, SliderGroup};
pub use figure::Figure;
pub use plotter::{AxisScale, PlotLine, Plotter, ReferenceLine, SummaryItem};
#[cfg(target_arch = "wasm32")]
pub use web::{WebHandle, WebResult};

pub type Points = Vec<[f64; 2]>;
pub type Result<T = ()> = eframe::Result<T>;
