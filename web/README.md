# Myplotlib web runtime

`loader.js` is Myplotlib's low-level browser runtime. Its lifecycle contract is:

- each Wasm module is initialized once per canonical URL;
- canvases mounted from the same module have independent application state;
- mounting on an occupied canvas destroys its previous application first;
- when mounts overlap on one canvas, the newest mount wins and superseded
  mounts reject with `AbortError`;
- unmounting during startup destroys any handle that startup eventually
  creates;
- failed initialization and mounting leave the canvas reusable; and
- `unmountApp` is idempotent and destroys each handle at most once.

`app.js` is the public application entry module emitted by
`cargo myplotlib build-web`. The command writes it as `app.js`, copies the
runtime as `myplotlib-loader.js`, and asks wasm-pack to emit `bindings.js` and
`bindings_bg.wasm` beside them.

A host page uses only the generated entry module:

```js
import { mount, unmount } from "./pkg/app.js";

await mount(document.querySelector("#plot-canvas"));
await unmount(document.querySelector("#plot-canvas"));
```

The `options` argument to `mount` is reserved for startup policy such as the
Rayon thread count.
