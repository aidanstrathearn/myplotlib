#[cfg(not(target_arch = "wasm32"))]
fn main() -> myplotlib::NativeResult {
    myplotlib_two_views::run_native()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
