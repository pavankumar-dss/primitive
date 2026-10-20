// One of the parallel searchers started by worker.js. Each holds its own copy
// of the model; for every shape, all of them search at once, the one that
// found the best shape commits it, and the rest replay it (see step() in
// worker.js). Every message gets one reply, in order.
importScripts('engine.js');

let engine = null;

let queue = Promise.resolve();
onmessage = ({ data }) => {
  queue = queue.then(() => handle(data)).then(
    (value) => postMessage({ type: 'ok', value }),
    (err) => postMessage({ type: 'error', message: String(err && err.message || err) }),
  );
};

async function handle(msg) {
  switch (msg.type) {
    case 'module':
      engine = await loadEngine(msg.module);
      return null;
    case 'init': {
      const { pixels, width, height, background, constraints, seed } = msg;
      // the output size only matters for rendering, which searchers never do
      return engine.init(pixels, width, height, Math.max(width, height), background, constraints, seed);
    }
    case 'search':
      return engine.search(msg.mode, msg.alpha, 1, msg.fresh);
    case 'commit':
      return engine.commit(msg.repeat);
    case 'apply':
      return engine.apply(msg.shapes);
  }
  throw new Error('unknown message ' + msg.type);
}
