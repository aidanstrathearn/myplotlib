import {
  mountApp as mountWithMyplotlib,
  unmountApp as unmountWithMyplotlib,
} from "./myplotlib-loader.js";

const bindingsModule = new URL("./bindings.js", import.meta.url);

/**
 * Mount this application into a canvas supplied by the host page.
 *
 * @param {HTMLCanvasElement} canvas
 * @param {{threads?: number}} options application startup options
 * @returns {Promise<object>} the application's exported WebHandle
 */
export function mount(canvas, options = {}) {
  return mountWithMyplotlib({
    ...options,
    canvas,
    module: bindingsModule,
  });
}

/**
 * Destroy the application mounted on a canvas, if one exists.
 *
 * @param {HTMLCanvasElement} canvas
 * @returns {Promise<void>}
 */
export function unmount(canvas) {
  return unmountWithMyplotlib(canvas);
}
