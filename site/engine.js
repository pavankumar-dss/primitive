// Loads the engine that runs the primitive algorithm, inside a worker: the
// Rust port in engine/, built to engine.wasm, wrapped in this interface:
//
//   init(pixels, width, height, outputSize, background, constraints, seed)
//                    → starting score; background is hex or '' for average
//   search(mode, alpha, searches, fresh) → energy of the best shape so far
//   commit(repeat)   → the shapes added (opaque, with a length), for apply
//   apply(shapes)    → new score
//   shapes(), svg(), animatedSVG(duration)
//   png()            → Promise of PNG bytes at the output size
//   gif(maxSide, frames, delay, lastDelay) → GIF bytes
//   animStart(maxSide, frames) → {width, height}; animFrame(k) → RGBA bytes

const ENGINE_FILE = 'engine.wasm';

async function loadEngine(module) {
  const x = (await WebAssembly.instantiate(module, {})).exports;
  const bytes = (ptr, len) => new Uint8Array(x.memory.buffer, ptr, len);
  const put = (data) => bytes(x.input_buffer(data.byteLength), data.byteLength).set(data);
  const text = (len) => new TextDecoder().decode(bytes(x.output_ptr(), len));
  let frameSize = 0;
  return {
    init(pixels, width, height, outputSize, background, constraints, seed) {
      put(pixels);
      const score = x.init(width, height, outputSize, background ? parseInt(background, 16) : -1, seed);
      if (constraints) {
        const c = constraints;
        if (c.region) put(c.region);
        const flags = (c.focus ? 1 : 0) | (typeof c.angle === 'number' ? 2 : 0) | (c.region ? 4 : 0);
        x.set_constraints(flags, c.focus ? c.focus[0] : 0, c.focus ? c.focus[1] : 0, c.angle || 0, c.minSize || 0, c.maxSize || 0);
      }
      return score;
    },
    search: (mode, alpha, searches, fresh) => x.search(mode, alpha, searches, fresh ? 1 : 0),
    commit(repeat) {
      const n = x.commit(repeat);
      return new Float64Array(x.memory.buffer, x.shapes_ptr(), n).slice();
    },
    apply(shapes) {
      put(new Uint8Array(shapes.buffer, shapes.byteOffset, shapes.byteLength));
      return x.apply(shapes.length);
    },
    shapes: () => x.shape_count(),
    svg: () => text(x.svg()),
    animatedSVG: (duration) => text(x.animated_svg(duration)),
    gif(maxSide, frames, delay, lastDelay) {
      const n = x.gif(maxSide, frames, delay, lastDelay); // replaces the output buffer
      return bytes(x.output_ptr(), n).slice();
    },
    animStart(maxSide, frames) {
      const size = x.anim_start(maxSide, frames, 1);
      const width = size >>> 16, height = size & 0xffff;
      frameSize = width * height * 4;
      return { width, height };
    },
    animFrame: (k) => bytes(x.anim_frame(k), frameSize).slice(),
    async png() {
      const size = x.anim_start(0, 1, 0);
      const width = size >>> 16, height = size & 0xffff;
      const pixels = new Uint8ClampedArray(bytes(x.anim_frame(0), width * height * 4));
      const canvas = new OffscreenCanvas(width, height);
      canvas.getContext('2d').putImageData(new ImageData(pixels, width, height), 0, 0);
      const blob = await canvas.convertToBlob({ type: 'image/png' });
      return new Uint8Array(await blob.arrayBuffer());
    },
  };
}
