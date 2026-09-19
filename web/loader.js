const mountKey = Symbol.for("myplotlib.webMount");
const modulePromises = new Map();

function moduleUrl(specifier) {
  if (specifier instanceof URL) {
    return specifier.href;
  }

  if (typeof specifier !== "string") {
    throw new TypeError("module must be a URL or URL string");
  }

  return new URL(specifier, document.baseURI).href;
}

function loadModule(specifier) {
  const url = moduleUrl(specifier);
  const existing = modulePromises.get(url);
  if (existing !== undefined) {
    return existing;
  }

  const loading = import(url).then(async (bindings) => {
    if (typeof bindings.default !== "function") {
      throw new TypeError(`${url} does not export a wasm-pack initializer`);
    }
    if (typeof bindings.mountApp !== "function") {
      throw new TypeError(`${url} does not export mountApp`);
    }

    await bindings.default();
    return bindings;
  });

  modulePromises.set(url, loading);
  loading.catch(() => {
    if (modulePromises.get(url) === loading) {
      modulePromises.delete(url);
    }
  });

  return loading;
}

function abortError() {
  return new DOMException("The Myplotlib mount was superseded", "AbortError");
}

function assertCanvas(canvas) {
  if (
    canvas === null ||
    (typeof canvas !== "object" && typeof canvas !== "function")
  ) {
    throw new TypeError("canvas must be an HTMLCanvasElement");
  }

  const Canvas =
    canvas.ownerDocument?.defaultView?.HTMLCanvasElement ??
    globalThis.HTMLCanvasElement;
  if (typeof Canvas === "function" && !(canvas instanceof Canvas)) {
    throw new TypeError("canvas must be an HTMLCanvasElement");
  }
}

function destroyHandle(state) {
  if (state.handle !== undefined && !state.destroyed) {
    state.destroyed = true;
    state.handle.destroy();
  }
}

async function dispose(state) {
  if (state === undefined) {
    return;
  }

  state.cancelled = true;
  try {
    await state.ready;
  } catch {
    // A failed or superseded mount has no live application to preserve.
  }
  destroyHandle(state);
}

/**
 * Initialize a wasm-pack module and mount its Myplotlib application.
 *
 * The module is initialized once for each canonical URL. The returned Rust
 * handle is also retained on the canvas so JavaScript garbage collection
 * cannot finalize it while the canvas remains mounted.
 *
 * @param {{canvas: HTMLCanvasElement, module: string | URL}} options
 * @returns {Promise<object>} the application's exported WebHandle
 */
export function mountApp({ canvas, module }) {
  assertCanvas(canvas);

  const previous = canvas[mountKey];
  if (previous !== undefined) {
    previous.cancelled = true;
  }

  const state = {
    cancelled: false,
    destroyed: false,
    handle: undefined,
    ready: undefined,
  };
  canvas[mountKey] = state;

  state.ready = (async () => {
    await dispose(previous);
    if (state.cancelled || canvas[mountKey] !== state) {
      throw abortError();
    }

    const bindings = await loadModule(module);
    if (state.cancelled || canvas[mountKey] !== state) {
      throw abortError();
    }

    state.handle = await bindings.mountApp(canvas);
    if (state.cancelled || canvas[mountKey] !== state) {
      destroyHandle(state);
      throw abortError();
    }

    return state.handle;
  })();

  state.ready.catch(() => {
    if (canvas[mountKey] === state) {
      delete canvas[mountKey];
    }
  });

  return state.ready;
}

/**
 * Destroy the application mounted on a canvas, if one exists.
 *
 * @param {HTMLCanvasElement} canvas
 * @returns {Promise<void>}
 */
export async function unmountApp(canvas) {
  assertCanvas(canvas);

  const state = canvas?.[mountKey];
  if (state === undefined) {
    return;
  }

  state.cancelled = true;
  await dispose(state);
  if (canvas[mountKey] === state) {
    delete canvas[mountKey];
  }
}
