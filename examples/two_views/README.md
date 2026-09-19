# Two views example

This package runs the same Myplotlib application natively or mounts two
independent instances into canvases in a web page.

Run it natively from the Myplotlib repository root:

```sh
cargo run --manifest-path examples/two_views/Cargo.toml
```

Install the Myplotlib build command from the repository root:

```sh
cargo install --path crates/cargo-myplotlib --locked
```

Build the self-contained web package and serve the example:

```sh
cargo myplotlib build-web \
  --manifest-path examples/two_views/Cargo.toml \
  --out-dir examples/two_views/pkg \
  --release \
  --locked
cd examples/two_views
python3 -m http.server 8080
```

Then open <http://127.0.0.1:8080/>. The generated `pkg/app.js` owns Wasm
initialization and application lifetime, so the example does not copy or
directly import Myplotlib's loader.

Applications built from different crates expose the same small API from their
own generated entry modules:

```js
import { mount as mountMts, unmount as unmountMts } from "/apps/mts/app.js";
import { mount as mountDfb } from "/apps/dfb/app.js";

await mountMts(mtsCanvas);
await mountDfb(dfbCanvas);

// Call this before removing a mounted canvas from a long-lived page.
await unmountMts(mtsCanvas);
```
