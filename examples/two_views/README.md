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
cd ../..
python3 -m http.server 8080
```

Then open <http://127.0.0.1:8080/examples/two_views/>. Serving the repository
root makes the shared `web/loader.js` available to the example. The loader
initializes `pkg/app.js` once, mounts it into both canvases, and retains each
application handle until its canvas is explicitly unmounted.

Applications built from different crates use the same API with a different
module URL for each canvas:

```js
import { mountApp, unmountApp } from "/web/loader.js";

await mountApp({ canvas: mtsCanvas, module: "/apps/mts/app.js" });
await mountApp({ canvas: dfbCanvas, module: "/apps/dfb/app.js" });

// Call this before removing a mounted canvas from a long-lived page.
await unmountApp(mtsCanvas);
```
