use eframe::egui;

pub type WebResult<T = ()> = std::result::Result<T, wasm_bindgen::JsValue>;

#[wasm_bindgen::prelude::wasm_bindgen]
pub struct WebHandle {
    runner: eframe::WebRunner,
}

#[wasm_bindgen::prelude::wasm_bindgen]
impl WebHandle {
    /// Shut down the application and release its browser resources.
    ///
    /// Web handles are finalized by JavaScript at an unspecified time, so
    /// dropping a handle does not destroy its runner. Call this method when
    /// the canvas is unmounted.
    pub fn destroy(&self) {
        self.runner.destroy();
    }
}

/// Mount an eframe application into a canvas supplied by the host page.
///
/// Each call creates independent application state. Retain the returned handle
/// so that [`WebHandle::destroy`] can be called when the canvas is unmounted.
/// Dropping the handle does not destroy the runner because JavaScript garbage
/// collection is nondeterministic.
///
/// This function resolves only after eframe has started successfully and
/// returns any startup error to the caller.
pub(crate) async fn mount(
    canvas: web_sys::HtmlCanvasElement,
    app: impl eframe::App + 'static,
) -> WebResult<WebHandle> {
    let runner = eframe::WebRunner::new();
    runner
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(move |creation_context| {
                creation_context
                    .egui_ctx
                    .set_visuals(egui::Visuals::light());
                Ok(Box::new(app))
            }),
        )
        .await?;

    Ok(WebHandle { runner })
}
