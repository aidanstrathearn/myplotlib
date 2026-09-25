#[cfg(not(target_arch = "wasm32"))]
fn main() -> myplotlib::NativeResult {
    myplotlib_app_menu::run_native()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
