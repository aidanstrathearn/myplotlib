# Streaming plots

This example demonstrates background calculations updating persistent plots. A
Monte Carlo calculation appends batches to a growing trace, while a numerical
Gaussian–Lorentzian convolution repeatedly replaces a complete trace as it
evolves from a Gaussian into a Voigt profile. Both use
`ViewOption::streaming`, `StreamPlan`, and `StreamContext`; the normal Myplotlib
app owns the worker lifecycle and plot UI. A synchronous view plots the expected
Monte Carlo error with `ViewOption::new`, showing that streaming and immediate
views can live in the same app.

Run it natively from the repository root:

```sh
cargo run --manifest-path examples/streaming/Cargo.toml
```

Changing a streaming view's controls cancels the active calculation and starts
a new run after a short debounce. The app header provides Stop, Start or
Restart, Reset, progress, saving, and status while the plot remains interactive.
Switching to the Expected error view cancels the stream and computes its plot
immediately; switching to either streaming view starts a new background
calculation.

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
point batches to the UI, which applies them through `SeriesId`; workers never
mutate the `Plotter` or egui state directly.
