#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    myplotlib_streaming::run_native()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
