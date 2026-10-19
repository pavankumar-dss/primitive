// Runs the Go primitive model (compiled to WebAssembly) off the main thread.
importScripts('wasm_exec.js');

const go = new Go();
const ready = WebAssembly.instantiateStreaming(fetch('primitive.wasm'), go.importObject)
  .then(({ instance }) => {
    go.run(instance);
    postMessage({ type: 'ready' });
  })
  .catch((err) => postMessage({ type: 'error', message: 'Could not load the WebAssembly module: ' + err.message }));

// Bumped on every start/stop so a running loop notices it was superseded.
let runId = 0;

// Yield to the event loop so "stop" messages are handled between shapes.
const channel = new MessageChannel();
const yieldQueue = [];
channel.port1.onmessage = () => yieldQueue.shift()();
const nextTick = () => new Promise((resolve) => {
  yieldQueue.push(resolve);
  channel.port2.postMessage(null);
});

async function run(id, job) {
  if (typeof primitiveInit !== 'function') return; // load failed; already reported
  const { pixels, width, height, mode, count, alpha, repeat, outputSize, background } = job;
  primitiveInit(new Uint8Array(pixels), width, height, outputSize, background);
  const start = performance.now();
  let lastPost = 0;
  let score = 0;
  for (let i = 1; i <= count; i++) {
    await nextTick();
    if (id !== runId) return;
    score = primitiveStep(mode, alpha, repeat);
    const now = performance.now();
    if (i === count || now - lastPost > 150) {
      lastPost = now;
      postMessage({ type: 'progress', done: i, count, score, elapsed: now - start, svg: primitiveSVG() });
    }
  }
  postMessage({ type: 'done', score, elapsed: performance.now() - start });
}

onmessage = async ({ data }) => {
  switch (data.type) {
    case 'start': {
      await ready;
      const id = ++runId;
      try {
        await run(id, data);
      } catch (err) {
        if (id === runId) postMessage({ type: 'error', message: String(err && err.message || err) });
      }
      break;
    }
    case 'stop':
      runId++;
      break;
    case 'png': {
      const bytes = primitivePNG();
      postMessage({ type: 'png', bytes }, bytes ? [bytes.buffer] : []);
      break;
    }
  }
};
