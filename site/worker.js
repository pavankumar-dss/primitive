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
  const { pixels, width, height, mode, count, alpha, repeat, outputSize, background, constraints } = job;
  if (constraints && constraints.region) constraints.region = new Uint8Array(constraints.region);
  primitiveInit(new Uint8Array(pixels), width, height, outputSize, background, constraints || null);
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
      postMessage({ type: 'progress', done: i, count, shapes: primitiveShapes(), score, elapsed: now - start, svg: primitiveSVG() });
    }
  }
  postMessage({ type: 'done', score, shapes: primitiveShapes(), elapsed: performance.now() - start });
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

async function exportAnimation({ format, duration, size, purpose }) {
  if (format === 'svg') {
    postMessage({ type: 'export', format, purpose, data: primitiveAnimatedSVG(duration) });
  } else if (format === 'gif') {
    const delay = Math.round(100 / GIF_FPS);
    const bytes = primitiveGIF(Math.min(size, 512), Math.round(duration * GIF_FPS), delay, HOLD_SECONDS * 100);
    postMessage({ type: 'export', format, purpose, data: bytes }, [bytes.buffer]);
  } else if (format === 'mp4') {
    const bytes = await encodeVideo(duration, size);
    postMessage({ type: 'export', format, purpose, data: bytes }, [bytes.buffer]);
  }
}

async function encodeVideo(duration, size) {
  const mb = await import('./vendor/mediabunny.min.mjs');
  const frames = Math.round(duration * VIDEO_FPS);
  const { width, height } = primitiveAnimStart(size, frames);
  const codec = await mb.getFirstEncodableVideoCodec(['avc', 'hevc', 'vp9', 'av1'], { width, height });
  if (!codec) throw new Error('This browser cannot encode video. Try a recent Chrome, Edge or Safari.');

  const output = new mb.Output({ format: new mb.Mp4OutputFormat({ fastStart: 'in-memory' }), target: new mb.BufferTarget() });
  const source = new mb.VideoSampleSource({ codec, bitrate: mb.QUALITY_HIGH });
  output.addVideoTrack(source, { frameRate: VIDEO_FPS });
  await output.start();
  for (let k = 0; k < frames; k++) {
    const last = k === frames - 1;
    const sample = new mb.VideoSample(primitiveAnimFrame(k), {
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
