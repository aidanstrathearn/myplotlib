/*
 * Copyright 2022 Google Inc. All Rights Reserved.
 * Modified by the Myplotlib project in 2026.
 * Changes include worker initialization, URL handling, and lifecycle management.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *     http://www.apache.org/licenses/LICENSE-2.0
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

function waitForMsgType(target, type) {
  return new Promise((resolve) => {
    target.addEventListener("message", function onMsg({ data }) {
      if (data == null || data.type !== type) return;
      target.removeEventListener("message", onMsg);
      resolve(data);
    });
  });
}

waitForMsgType(self, "wasm_bindgen_worker_init").then(async (data) => {
  const pkg = await import(data.mainJS);
  await pkg.default({ module_or_path: data.module, memory: data.memory });
  postMessage({ type: "wasm_bindgen_worker_ready" });
  pkg.wbg_rayon_start_worker(data.receiver);
});

let workers;

export async function startWorkers(module, memory, builder) {
  const threadCount = builder.numThreads();
  if (threadCount === 0) {
    throw new Error("num_threads must be > 0.");
  }

  const workerInit = {
    type: "wasm_bindgen_worker_init",
    module,
    memory,
    receiver: builder.receiver(),
    mainJS: builder.mainJS(),
  };

  const workerModule = new URL(import.meta.url);
  let workerUrl = workerModule;
  let blobUrl;

  if (workerModule.origin !== globalThis.location.origin) {
    const response = await fetch(workerModule);
    if (!response.ok) {
      throw new Error(
        `failed to fetch Rayon worker: ${response.status} ${response.statusText}`,
      );
    }
    blobUrl = URL.createObjectURL(await response.blob());
    workerUrl = blobUrl;
  }

  try {
    workers = await Promise.all(
      Array.from({ length: threadCount }, async () => {
        const worker = new Worker(workerUrl, { type: "module" });
        worker.postMessage(workerInit);
        await waitForMsgType(worker, "wasm_bindgen_worker_ready");
        return worker;
      }),
    );
  } finally {
    if (blobUrl !== undefined) {
      URL.revokeObjectURL(blobUrl);
    }
  }

  builder.build();
}
