# Two views example

This package runs the same Myplotlib application natively or mounts two
independent instances into canvases in a web page. Its Voigt view evaluates a
Gaussian average of a Lorentzian with Rayon and overlays the analytic Voigt
profile.

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
  --wasm-threads \
  --manifest-path examples/two_views/Cargo.toml \
  --out-dir examples/two_views/dist \
  --release \
  --locked
cargo myplotlib serve --dir examples/two_views
```

Then open <http://127.0.0.1:8080/>. The generated `dist/app.js` owns Wasm
initialization and application lifetime, so the example does not copy or
directly import Myplotlib's loader. The server sends COOP and COEP headers on
every response so threaded builds are cross-origin isolated. The page chooses
up to eight workers from `navigator.hardwareConcurrency` by default. Its
thread selector reloads the page with a `?threads=` query parameter; both
mounted application instances then share the selected Rayon pool.

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
