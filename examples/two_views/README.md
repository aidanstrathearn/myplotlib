# Two views example

This package runs the same Myplotlib application natively or mounts two
independent instances into canvases in a web page.

Run it natively from the Myplotlib repository root:

```sh
cargo run --manifest-path examples/two_views/Cargo.toml
```

Build the web package with deterministic output names:

```sh
cd examples/two_views
wasm-pack build --target web --release --out-dir pkg --out-name app --no-typescript --no-pack
python3 -m http.server 8080
```

Then open <http://127.0.0.1:8080>. The generated ES module is `pkg/app.js`
and its WebAssembly module is `pkg/app_bg.wasm`.
