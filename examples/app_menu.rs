#[cfg(not(target_arch = "wasm32"))]
#[path = "app_menu/src/lib.rs"]
mod app_menu;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> myplotlib::NativeResult {
    app_menu::run_native()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
