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

If an application's bindings export `initThreadPool`, mounting also requires a
positive integer thread count. The loader initializes one Rayon pool before
the first canvas mounts and requires every later mount from that module to use
the same count:

```js
await mount(document.querySelector("#plot-canvas"), { threads: 8 });
```

Applications without `initThreadPool` continue to mount without options.

`app.js` is the public application entry module emitted by
`cargo myplotlib build-web`. The command writes it as `app.js`, copies the
runtime as `myplotlib-loader.js`, and asks wasm-pack to emit `bindings.js` and
`bindings_bg.wasm` beside them. It also exports `cargoMyplotlibVersion` and
`myplotlibWebPackageVersion`, allowing hosts to identify the build tool release
and generated-package contract.

A host page uses only the generated entry module:

```js
import { mount, unmount } from "./pkg/app.js";

await mount(document.querySelector("#plot-canvas"));
await unmount(document.querySelector("#plot-canvas"));
```

The page must be cross-origin isolated before mounting a threaded application.
