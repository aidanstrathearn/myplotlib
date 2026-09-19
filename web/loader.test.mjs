import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const loaderSource = await readFile(new URL("./loader.js", import.meta.url), "utf8");
let loaderNumber = 0;
let moduleNumber = 0;

async function freshLoader() {
  const source = `${loaderSource}\n// test loader ${loaderNumber++}`;
  return import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);
}

function fakeModule(t, bridge) {
  const key = `__myplotlibTestModule${moduleNumber++}`;
  globalThis[key] = bridge;
  t.after(() => delete globalThis[key]);

  const source = `
    export default function init() {
      return globalThis[${JSON.stringify(key)}].init();
    }
    export function mountApp(canvas) {
      return globalThis[${JSON.stringify(key)}].mount(canvas);
    }
  `;
  return new URL(`data:text/javascript,${encodeURIComponent(source)}`);
}

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, reject, resolve };
}

function handle(name, destroyed) {
  return {
    name,
    destroy() {
      destroyed.push(name);
    },
  };
}

test("initializes a module once and owns independent canvas handles", async (t) => {
  const { mountApp, unmountApp } = await freshLoader();
  const destroyed = [];
  let initializations = 0;
  let mounts = 0;
  const module = fakeModule(t, {
    init() {
      initializations += 1;
    },
    mount() {
      mounts += 1;
      return handle(`handle-${mounts}`, destroyed);
    },
  });
  const firstCanvas = {};
  const secondCanvas = {};

  const [first, second] = await Promise.all([
    mountApp({ canvas: firstCanvas, module }),
    mountApp({ canvas: secondCanvas, module }),
  ]);

  assert.equal(initializations, 1);
  assert.equal(mounts, 2);
  assert.notEqual(first, second);
  assert.equal(firstCanvas[Symbol.for("myplotlib.webMount")].handle, first);
  assert.equal(secondCanvas[Symbol.for("myplotlib.webMount")].handle, second);

  await unmountApp(firstCanvas);
  await unmountApp(secondCanvas);
  await unmountApp(firstCanvas);
  assert.deepEqual(destroyed.sort(), ["handle-1", "handle-2"]);
});

test("destroys the previous handle before replacing a canvas mount", async (t) => {
  const { mountApp, unmountApp } = await freshLoader();
  const destroyed = [];
  let mounts = 0;
  const module = fakeModule(t, {
    init() {},
    mount() {
      mounts += 1;
      return handle(`handle-${mounts}`, destroyed);
    },
  });
  const canvas = {};

  const first = await mountApp({ canvas, module });
  const second = await mountApp({ canvas, module });

  assert.notEqual(first, second);
  assert.deepEqual(destroyed, ["handle-1"]);
  await unmountApp(canvas);
  assert.deepEqual(destroyed, ["handle-1", "handle-2"]);
});

test("the newest overlapping mount wins", async (t) => {
  const { mountApp, unmountApp } = await freshLoader();
  const destroyed = [];
  const firstHandle = deferred();
  const firstStarted = deferred();
  let mounts = 0;
  const module = fakeModule(t, {
    init() {},
    mount() {
      mounts += 1;
      if (mounts === 1) {
        firstStarted.resolve();
        return firstHandle.promise;
      }
      return handle("second", destroyed);
    },
  });
  const canvas = {};

  const superseded = mountApp({ canvas, module });
  await firstStarted.promise;
  const replacement = mountApp({ canvas, module });
  firstHandle.resolve(handle("first", destroyed));

  await assert.rejects(superseded, { name: "AbortError" });
  const replacementHandle = await replacement;
  assert.equal(replacementHandle.name, "second");
  assert.deepEqual(destroyed, ["first"]);

  await unmountApp(canvas);
  assert.deepEqual(destroyed, ["first", "second"]);
});

test("unmount during startup destroys the eventual handle exactly once", async (t) => {
  const { mountApp, unmountApp } = await freshLoader();
  const destroyed = [];
  const pendingHandle = deferred();
  const mountStarted = deferred();
  const module = fakeModule(t, {
    init() {},
    mount() {
      mountStarted.resolve();
      return pendingHandle.promise;
    },
  });
  const canvas = {};

  const mounting = mountApp({ canvas, module });
  await mountStarted.promise;
  const unmounting = unmountApp(canvas);
  pendingHandle.resolve(handle("pending", destroyed));

  await assert.rejects(mounting, { name: "AbortError" });
  await unmounting;
  await unmountApp(canvas);
  assert.deepEqual(destroyed, ["pending"]);
});

test("failed initialization can be retried", async (t) => {
  const { mountApp, unmountApp } = await freshLoader();
  const destroyed = [];
  let initializations = 0;
  let mounts = 0;
  const module = fakeModule(t, {
    init() {
      initializations += 1;
      if (initializations === 1) {
        throw new Error("initialization failed");
      }
    },
    mount() {
      mounts += 1;
      return handle("recovered", destroyed);
    },
  });
  const canvas = {};

  await assert.rejects(mountApp({ canvas, module }), /initialization failed/);
  await mountApp({ canvas, module });

  assert.equal(initializations, 2);
  assert.equal(mounts, 1);
  await unmountApp(canvas);
  assert.deepEqual(destroyed, ["recovered"]);
});

test("failed mounting leaves the canvas reusable", async (t) => {
  const { mountApp, unmountApp } = await freshLoader();
  const destroyed = [];
  let initializations = 0;
  let mounts = 0;
  const module = fakeModule(t, {
    init() {
      initializations += 1;
    },
    mount() {
      mounts += 1;
      if (mounts === 1) {
        throw new Error("mount failed");
      }
      return handle("recovered", destroyed);
    },
  });
  const canvas = {};

  await assert.rejects(mountApp({ canvas, module }), /mount failed/);
  await mountApp({ canvas, module });

  assert.equal(initializations, 1);
  assert.equal(mounts, 2);
  await unmountApp(canvas);
  assert.deepEqual(destroyed, ["recovered"]);
});

test("rejects non-canvas browser elements", async () => {
  const { mountApp, unmountApp } = await freshLoader();
  class Canvas {}
  const notCanvas = {
    ownerDocument: {
      defaultView: { HTMLCanvasElement: Canvas },
    },
  };

  assert.throws(
    () => mountApp({ canvas: notCanvas, module: new URL("data:text/javascript,") }),
    /HTMLCanvasElement/,
  );
  await assert.rejects(unmountApp(notCanvas), /HTMLCanvasElement/);
});
