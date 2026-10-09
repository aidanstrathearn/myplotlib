mod app;
mod appmenu;
mod controls;
mod data_import;
mod figure;
mod plotter;
mod save;
#[cfg(feature = "streaming")]
mod stream;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
pub use app::mount_web;
pub use app::{App, AppDefinition, AppError, AppResult, ControlsAction, PlotAction, ViewOption};
#[cfg(not(target_arch = "wasm32"))]
pub use app::{NativeResult, run_native};
#[cfg(feature = "streaming")]
pub use app::{StreamAction, StreamPlanResult};
pub use appmenu::AppMenu;
pub use controls::{Slider, SliderGrid, SliderGroup};
pub use figure::Figure;
pub use plotter::{AxisScale, PlotLine, Plotter, ReferenceLine, SeriesId, SummaryItem};
#[cfg(feature = "streaming")]
pub use stream::{CancelToken, PlotSink, StreamClosed, StreamError, StreamPlan, StreamResult};
#[cfg(all(target_arch = "wasm32", feature = "streaming"))]
pub use wasm_bindgen_rayon::init_thread_pool;
#[cfg(target_arch = "wasm32")]
pub use web::{WebHandle, WebResult};

pub type Points = Vec<[f64; 2]>;
pub type Result<T = ()> = eframe::Result<T>;
