// Finds the object at the points someone clicks, for selecting a subject
// (see constraints.js). It runs SlimSAM, a small version of Meta's Segment
// Anything model, with Transformers.js, all from files served with the site
// (scripts/build-site.sh). Looking at an image takes a few seconds; after
// that each set of clicks takes a fraction of one.
//
// Messages in: {type: 'image', pixels, width, height} (RGBA), then
// {type: 'points', seq, points: [{x, y, include}]} with x and y in fractions
// of the image. Messages out: {type: 'progress', loaded} (a fraction, while
// the model downloads), {type: 'looking'} when it starts on the image,
// {type: 'ready'} once it has looked at it,
// {type: 'mask', seq, width, height, data} (a byte per pixel, 255 inside),
// and {type: 'error', message}.
import { SamModel, AutoProcessor, RawImage, Tensor, env } from './vendor/transformers/transformers.min.js';

const MODEL = 'Xenova/slimsam-77-uniform';
env.allowLocalModels = true; // off by default in browsers
env.allowRemoteModels = false;
env.localModelPath = new URL('vendor/models/', location.href).href;
// ONNX Runtime's CPU build: half the size of its WebGPU-capable build, and
// as fast for this quantized model
env.backends.onnx.wasm.wasmPaths = {
  mjs: new URL('vendor/ort/ort-wasm-simd-threaded.mjs', location.href).href,
  wasm: new URL('vendor/ort/ort-wasm-simd-threaded.wasm', location.href).href,
};

let loading = null;
let model, processor;
let image = null; // {inputs, embeddings, width, height} of the current image

function load() {
  if (!loading) {
    // download progress over all the model's files
    const files = new Map();
    const progress_callback = (p) => {
      if (p.status !== 'progress' || !p.total) return;
      files.set(p.file, [p.loaded, p.total]);
      let loaded = 0, total = 0;
      for (const [l, t] of files.values()) { loaded += l; total += t; }
      postMessage({ type: 'progress', loaded: loaded / total });
    };
    loading = Promise.all([
      SamModel.from_pretrained(MODEL, { dtype: 'q8', device: 'wasm', progress_callback }),
      AutoProcessor.from_pretrained(MODEL),
    ]).then(([m, p]) => { model = m; processor = p; }, (err) => {
      loading = null; // let the next image try again
      throw err;
    });
  }
  return loading;
}

async function look(pixels, width, height) {
  image = null;
  await load();
  postMessage({ type: 'looking' });
  const raw = new RawImage(new Uint8ClampedArray(pixels), width, height, 4).rgb();
  const inputs = await processor(raw);
  image = { inputs, embeddings: await model.get_image_embeddings(inputs), width, height };
  postMessage({ type: 'ready' });
}

async function segment(seq, points) {
  if (!image || !points.length) return;
  const { inputs, embeddings, width, height } = image;
  // the model takes points in its resized image's pixels
  const [h, w] = inputs.reshaped_input_sizes[0];
  const input_points = new Tensor('float32', points.flatMap((p) => [p.x * w, p.y * h]), [1, 1, points.length, 2]);
  const input_labels = new Tensor('int64', points.map((p) => BigInt(p.include ? 1 : 0)), [1, 1, points.length]);
  const outputs = await model({ ...embeddings, input_points, input_labels });
  const [masks] = await processor.post_process_masks(outputs.pred_masks, inputs.original_sizes, inputs.reshaped_input_sizes);
  // it proposes three masks; keep the one it rates best
  const scores = outputs.iou_scores.data;
  let best = 0;
  for (let i = 1; i < scores.length; i++) if (scores[i] > scores[best]) best = i;
  const n = width * height;
  const data = new Uint8Array(n);
  for (let i = 0; i < n; i++) data[i] = masks.data[best * n + i] ? 255 : 0;
  postMessage({ type: 'mask', seq, width, height, data }, [data.buffer]);
}

// one message at a time, in order: points wait for the image before them
let queue = Promise.resolve();
onmessage = ({ data }) => {
  queue = queue.then(() => (data.type === 'image'
    ? look(data.pixels, data.width, data.height)
    : segment(data.seq, data.points)))
    .catch((err) => postMessage({ type: 'error', message: String(err && err.message || err) }));
};
