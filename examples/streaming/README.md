# Streaming plots

This example demonstrates background calculations updating persistent plots. A
Monte Carlo calculation appends batches to a growing trace, while a numerical
Gaussian–Lorentzian convolution repeatedly replaces a complete trace as it
evolves from a Gaussian into a Voigt profile. The convolution view also fits a
Voigt profile to each published partial result and uses
`StreamContext::update_params` to drive fitted-width sliders toward the input
widths. The fitted trace is bound to those parameters with
`StreamPlan::bind_points`, so the UI refreshes it without a second worker plot
update. Both use
`ViewOption::streaming`, `StreamPlan`, and `StreamContext`; the normal Myplotlib
app owns the worker lifecycle and plot UI. A synchronous view plots the expected
Monte Carlo error with `ViewOption::new`, showing that streaming and immediate
views can live in the same app.

Run it natively from the repository root:

```sh
cargo run --manifest-path examples/streaming/Cargo.toml
```

By default, selecting a streaming view prepares its initial plot without
starting the calculation. Adjusting its controls rebuilds that initial plot, so
the Gaussian and Lorentzian inputs and the fit's initial guess can be inspected
before running the Voigt integration.
Start/Stop, an Autoplay toggle, progress, and status appear in a horizontal
Streaming strip between the view's control grid and plot. Reset, saving, and
compute time remain in the app header.
Ready views show the time used to prepare their initial plot; a completed stream
replaces it with the background job's total time. Changing controls or views
cancels active work; press Start when the new inputs are ready, or enable
Autoplay to run automatically. The Expected error view remains synchronous and
computes immediately.

Autoplay is controlled by the user and is off by default for each view. Turning
it on starts an idle calculation immediately. It then starts fresh calculations
when the view's inputs change or the user returns to that view. The setting is
remembered across input changes and view switches during the session. Turning
it off lets the current calculation continue; Stop cancels it. Stopping or
finishing a calculation does not automatically restart it. Reset turns
Autoplay off for all views.

Each plan can customize its control-strip label:

```rust
StreamPlan::new(plot, job)
    .label("Monte Carlo sampling")
```

The default label is `Streaming`. Autoplay is no longer configured with
`StreamPlan::autoplay`; use the toggle in the Streaming strip.

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
the UI applies them before drawing controls and refreshes parameter-bound point
series once per drained batch. Workers never mutate the `Plotter`, application
parameters, or egui state directly.
