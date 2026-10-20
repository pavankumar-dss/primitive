// One of the parallel searchers started by worker.js. Each holds its own copy
// of the model; for every shape, all of them search at once, the one that
// found the best shape commits it, and the rest replay it (see step() in
// worker.js). Every message gets one reply, in order.
importScripts('wasm_exec.js');

let queue = Promise.resolve();
onmessage = ({ data }) => {
  queue = queue.then(() => handle(data)).then(
    (value) => postMessage({ type: 'ok', value }),
    (err) => postMessage({ type: 'error', message: String(err && err.message || err) }),
  );
};

async function handle(msg) {
  switch (msg.type) {
    case 'module': {
      const go = new Go();
      go.run(await WebAssembly.instantiate(msg.module, go.importObject));
      return null;
    }
    case 'init': {
      const { pixels, width, height, background, constraints, seed } = msg;
      // the output size only matters for rendering, which searchers never do
      return primitiveInit(pixels, width, height, Math.max(width, height), background, constraints, seed);
    }
    case 'search':
      return primitiveSearch(msg.mode, msg.alpha, 1, msg.fresh);
    case 'commit':
      return primitiveCommit(msg.repeat);
    case 'apply':
      return primitiveApply(msg.shapes);
  }
  throw new Error('unknown message ' + msg.type);
}
