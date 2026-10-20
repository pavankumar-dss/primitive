// Runs the primitive engine (see engine.js) off the main thread. This
// worker's model only follows along, for previews and exports: the search
// for each shape is spread over a pool of search workers (search-worker.js),
// each with its own copy of the model.
importScripts('engine.js');

let engine = null;
let wasmModule = null;
const ready = WebAssembly.compileStreaming(fetch(ENGINE_FILE))
  .then((module) => {
    wasmModule = module;
    return loadEngine(module);
  })
  .then((e) => {
    engine = e;
    postMessage({ type: 'ready', maxThreads: MAX_THREADS });
  })
  .catch((err) => postMessage({ type: 'error', message: 'Could not load the WebAssembly module: ' + err.message }));

// ---------------------------------------------------------------- search pool

const SEARCHES = 16;   // independent searches per shape, as in the command-line tool
const MAX_THREADS = SEARCHES;
const pool = [];

function spawn() {
  if (typeof Worker !== 'function') {
    throw new Error('This browser cannot start workers from a worker. Try a recent Chrome, Edge, Firefox or Safari.');
  }
  const worker = new Worker('search-worker.js');
  const pending = [];
  worker.onmessage = ({ data }) => {
    const p = pending.shift();
    if (data.type === 'error') p.reject(new Error(data.message));
    else p.resolve(data.value);
  };
  worker.onerror = (e) => {
    e.preventDefault();
    for (const p of pending.splice(0)) p.reject(new Error(e.message || 'A search worker crashed.'));
  };
  const call = (msg) => new Promise((resolve, reject) => {
    pending.push({ resolve, reject });
    worker.postMessage(msg);
  });
  // A message we don't wait for: a failure also fails the next call, since
  // the searcher's engine is then broken or never loaded.
  const send = (msg) => { call(msg).catch(() => {}); };
  send({ type: 'module', module: wasmModule });
  return { call, send };
}

// Finds and adds the next shape. Searchers take the 16 searches one at a
// time as they become free, so faster cores (or ones the OS favors) do more
// of them; a fixed split would wait for the slowest. Returns the new score.
async function step(id, searchers, mode, alpha, repeat) {
  let left = SEARCHES;
  const energies = await Promise.all(searchers.map(async (s) => {
    let energy = Infinity;
    for (let fresh = true; left > 0; fresh = false) {
      left--;
      energy = await s.call({ type: 'search', mode, alpha, fresh });
    }
    return energy;
  }));
  if (id !== runId) return null;
  let best = 0;
  energies.forEach((e, k) => { if (e < energies[best]) best = k; });
  const shapes = await searchers[best].call({ type: 'commit', repeat });
  if (id !== runId) return null;
  if (shapes.length) searchers.forEach((s, k) => { if (k !== best) s.send({ type: 'apply', shapes }); });
  return engine.apply(shapes);
}

const seed = () => crypto.getRandomValues(new Uint32Array(1))[0];

// ---------------------------------------------------------------- running

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
  if (!engine) return; // load failed; already reported
  const { width, height, mode, count, alpha, repeat, outputSize, background, constraints } = job;
  const pixels = new Uint8Array(job.pixels);
  if (constraints && constraints.region) constraints.region = new Uint8Array(constraints.region);
  engine.init(pixels, width, height, outputSize, background, constraints || null, seed());

  const threads = Math.max(1, Math.min(MAX_THREADS, job.threads || 1));
  while (pool.length < threads) pool.push(spawn());
  const searchers = pool.slice(0, threads);
  for (const s of searchers) s.send({ type: 'init', pixels, width, height, background, constraints: constraints || null, seed: seed() });

  const start = performance.now();
  let lastPost = 0;
  let score = 0;
  for (let i = 1; i <= count; i++) {
    await nextTick();
    if (id !== runId) return;
    score = await step(id, searchers, mode, alpha, repeat);
    if (id !== runId) return;
    const now = performance.now();
    if (i === count || now - lastPost > 150 + engine.shapes() / 10) {
      lastPost = now;
      postMessage({ type: 'progress', done: i, count, shapes: engine.shapes(), score, elapsed: now - start, svg: engine.svg() });
    }
  }
  postMessage({ type: 'done', score, shapes: engine.shapes(), elapsed: performance.now() - start, threads });
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
    case 'prepare':
      // start the search workers early so the first run doesn't wait for them
      await ready;
      try {
        while (pool.length < Math.min(MAX_THREADS, data.threads)) pool.push(spawn());
      } catch {
        // reported when a run starts
      }
      break;
    case 'stop':
      runId++;
      break;
    case 'png': {
      const bytes = await engine.png(data.count);
      postMessage({ type: 'png', bytes }, bytes ? [bytes.buffer] : []);
      break;
    }
    case 'svg':
      postMessage({ type: 'svg', data: engine.svg(data.count) });
      break;
    case 'export':
      try {
        await exportAnimation(data);
      } catch (err) {
        postMessage({ type: 'export-error', message: String(err && err.message || err) });
      }
      break;
  }
};

// ---------------------------------------------------------------- animations

const GIF_FPS = 15;
const VIDEO_FPS = 30;
const HOLD_SECONDS = 2.5; // how long the finished image stays at the end

// count, if set, ends the animation at that many shapes.
async function exportAnimation({ format, duration, size, purpose, count }) {
  if (format === 'svg') {
    postMessage({ type: 'export', format, purpose, data: engine.animatedSVG(duration, count) });
  } else if (format === 'gif') {
    const delay = Math.round(100 / GIF_FPS);
    const bytes = engine.gif(Math.min(size, 512), Math.round(duration * GIF_FPS), delay, HOLD_SECONDS * 100, count);
    postMessage({ type: 'export', format, purpose, data: bytes }, [bytes.buffer]);
  } else if (format === 'mp4') {
    const bytes = await encodeVideo(duration, size, count);
    postMessage({ type: 'export', format, purpose, data: bytes }, [bytes.buffer]);
  }
}

async function encodeVideo(duration, size, count) {
  const mb = await import('./vendor/mediabunny.min.mjs');
  const frames = Math.round(duration * VIDEO_FPS);
  const { width, height } = engine.animStart(size, frames, count);
  const codec = await mb.getFirstEncodableVideoCodec(['avc', 'hevc', 'vp9', 'av1'], { width, height });
  if (!codec) throw new Error('This browser cannot encode video. Try a recent Chrome, Edge or Safari.');

  const output = new mb.Output({ format: new mb.Mp4OutputFormat({ fastStart: 'in-memory' }), target: new mb.BufferTarget() });
  const source = new mb.VideoSampleSource({ codec, bitrate: mb.QUALITY_HIGH });
  output.addVideoTrack(source, { frameRate: VIDEO_FPS });
  await output.start();
  for (let k = 0; k < frames; k++) {
    const last = k === frames - 1;
    const sample = new mb.VideoSample(engine.animFrame(k), {
      format: 'RGBA',
      codedWidth: width,
      codedHeight: height,
      timestamp: k / VIDEO_FPS,
      duration: last ? HOLD_SECONDS : 1 / VIDEO_FPS,
    });
    await source.add(sample);
    sample.close();
    if (k % 10 === 0) postMessage({ type: 'export-progress', done: k, total: frames });
  }
  await output.finalize();
  return new Uint8Array(output.target.buffer);
}
