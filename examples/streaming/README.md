# Streaming plots

This example demonstrates background calculations updating persistent plots. A
Monte Carlo calculation appends batches to a growing trace, while a numerical
Gaussian–Lorentzian convolution repeatedly replaces a complete trace as it
evolves from a Gaussian into a Voigt profile. The convolution view also fits a
Voigt profile to each published partial result, replaces a fitted trace, and
uses `StreamContext::update_params` to drive fitted-width sliders toward the
input widths. Both use
`ViewOption::streaming`, `StreamPlan`, and `StreamContext`; the normal Myplotlib
app owns the worker lifecycle and plot UI. A synchronous view plots the expected
Monte Carlo error with `ViewOption::new`, showing that streaming and immediate
views can live in the same app.

Run it natively from the repository root:

```sh
cargo run --manifest-path examples/streaming/Cargo.toml
```

Selecting a streaming view prepares its initial plot without starting the
calculation. Adjusting its controls rebuilds that initial plot, so the Gaussian
and Lorentzian inputs and the fit's initial guess can be inspected before
running the Voigt integration.
Start, Stop, progress, and status sit beside the view's controls. Reset, saving,
and compute time remain in the app header. Ready views show the time used to
prepare their initial plot; a completed stream replaces it with the background
job's total time. Changing controls or views cancels active work; press Start
when the new inputs are ready. The Expected error view remains synchronous and
computes immediately.

Build a threaded WebAssembly site and serve it with cross-origin isolation:

```sh
cargo myplotlib build-web \
  --site \
  --wasm-threads \
  --rayon-threads 4 \
  --manifest-path examples/streaming/Cargo.toml \
  --out-dir examples/streaming/dist \
  --release \
  --locked
cargo myplotlib serve --dir examples/streaming/dist
```

Then open <http://127.0.0.1:8080/>.

Native streaming uses a dedicated standard thread. Web streaming uses the Rayon
Web Worker pool initialized by the generated site. Both workers send owned
point batches to the UI, which applies them through `SeriesId`. A stream can
also queue application-parameter mutations with `StreamContext::update_params`;
the UI applies them before drawing controls. Workers never mutate the `Plotter`,
application parameters, or egui state directly.
