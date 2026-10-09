# Streaming plot prototype

This example demonstrates a Monte Carlo calculation running away from the UI
thread and appending batched results to a persistent `Plotter`. The worker,
message, cancellation, and status types are private to the example so their
behaviour can be evaluated before Myplotlib exposes a public streaming app API.

Run it natively from the repository root:

```sh
cargo run --manifest-path examples/streaming/Cargo.toml
```

The target sample slider cancels the active calculation and starts a new run
after a short debounce. Stop, Start, Restart, and Reset exercise the worker
lifecycle while the plot remains interactive.

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

The native prototype uses a dedicated standard thread. The web prototype uses
the Rayon Web Worker pool initialized by the generated site. Both workers send
owned point batches to the UI, which applies them through `SeriesId`; workers
never mutate the `Plotter` or egui state directly.
