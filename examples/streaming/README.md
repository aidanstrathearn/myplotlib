# Streaming plot

This example demonstrates a Monte Carlo calculation running away from the UI
thread and appending batched results to a persistent `Plotter`. It uses
`ViewOption::streaming`, `StreamPlan`, `PlotSink`, and `CancelToken`; the normal
Myplotlib app owns the worker lifecycle and the plot UI. A second, synchronous
view plots the expected statistical error with `ViewOption::new`, showing that
streaming and immediate views can live in the same app.

Run it natively from the repository root:

```sh
cargo run --manifest-path examples/streaming/Cargo.toml
```

The target sample slider cancels the active calculation and starts a new run
after a short debounce. The app header provides Stop, Start or Restart, Reset,
progress, saving, and status while the plot remains interactive. Switching to
the Expected error view cancels the stream and computes its plot immediately;
switching back starts a new Monte Carlo run.

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
