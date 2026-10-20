'use strict';

const $ = (id) => document.getElementById(id);
const ui = {
  form: $('controls'), drop: $('drop'), file: $('file'), url: $('url'), loadUrl: $('load-url'),
  mode: $('mode'), count: $('count'), countCustom: $('count-custom'), countWarn: $('count-warn'),
  alpha: $('alpha'), alphaAuto: $('alpha-auto'),
  inputSize: $('input-size'), outputSize: $('output-size'), repeat: $('repeat'), threads: $('threads'),
  bgAuto: $('bg-auto'), bg: $('bg'), start: $('start'), stop: $('stop'), status: $('status'),
  palette: $('palette'), paletteCustom: $('palette-custom'), paletteColors: $('palette-colors'),
  paletteSwatches: $('palette-swatches'), paletteHint: $('palette-hint'),
  progress: $('progress'), progressBar: $('progress-bar'),
  original: $('original'), result: $('result'), resultCaption: $('result-caption'),
  dlPng: $('dl-png'), dlSvg: $('dl-svg'), replay: $('replay'), duration: $('duration'), scrub: $('scrub'),
  exportButtons: [...document.querySelectorAll('[data-export]')], exportStatus: $('export-status'),
  share: $('share'), shareBox: $('share-box'), shareNote: $('share-note'), shareLink: $('share-link'),
};

const EXPORTS = {
  gif: { label: 'GIF', type: 'image/gif', suffix: '.gif' },
  mp4: { label: 'MP4 video', type: 'video/mp4', suffix: '.mp4' },
  svg: { label: 'animated SVG', type: 'image/svg+xml', suffix: '-animated.svg' },
};

const MODE_NAMES = {
  0: 'shapes', 1: 'triangles', 2: 'rectangles', 3: 'ellipses', 4: 'circles',
  5: 'rotated rectangles', 6: 'Bézier curves', 7: 'rotated ellipses', 8: 'polygons',
};

let source = null;       // ImageBitmap of the loaded image
let sourceName = 'image';
let sourceOrigin = null; // {url} or {sample} when the image can go in a share link
let engineReady = false;
let running = false;
let lastSvg = null;
let kept = null;         // the original image and region mask around a kept region (see keepLayers)
let lastJob = null;
let lastShapes = 0;      // shapes in the current result
let resultLabel = 'Primitive'; // the result's caption when it shows every shape
let visible = 0;         // shapes the preview shows, of those in its SVG
let exporting = null;    // format of the animation being made, if any
let lastSpeed = null;    // {mode, inputSize, threads, perSecond} measured on the last full run

// ---------------------------------------------------------------- worker

const worker = new Worker('worker.js');
worker.onmessage = ({ data }) => {
  switch (data.type) {
    case 'ready':
      engineReady = true;
      setupThreads(data.maxThreads);
      setStatus(source ? 'Ready.' : 'Ready. Choose an image to start.');
      updateButtons();
      break;
    case 'progress':
      if (!running) break;
      lastShapes = data.shapes;
      lastSvg = data.svg;
      showResult(data.svg);
      ui.progressBar.style.width = (100 * data.done / data.count) + '%';
      setStatus(`Shape ${data.done} of ${data.count} · ${fmtTime(data.elapsed)}`
        + ` · ${(data.done / data.elapsed * 1000).toFixed(1)} shapes/s`);
      break;
    case 'done': {
      lastShapes = data.shapes;
      const short = lastJob.constraints && data.shapes < lastJob.count
        ? ' Some steps found no shape that fits the constraints.' : '';
      finish(`Done: ${shapeLabel(data.shapes)} in ${fmtTime(data.elapsed)}.${short}`);
      if (data.elapsed > 2000) {
        lastSpeed = { mode: lastJob.mode, inputSize: lastJob.inputSize, threads: data.threads, perSecond: lastJob.count / data.elapsed * 1000 };
      }
      setResultLabel(shapeLabel(data.shapes));
      break;
    }
    case 'png':
      if (data.bytes) download(new Blob([data.bytes], { type: 'image/png' }), '.png');
      break;
    case 'svg':
      downloadSvg(data.data, '.svg');
      break;
    case 'export':
      exporting = null;
      if (data.purpose === 'replay') {
        showResult(data.data);
        setExportStatus('');
      } else if (data.format === 'svg') {
        downloadSvg(data.data, EXPORTS.svg.suffix).then((blob) => {
          setExportStatus(`Saved ${EXPORTS.svg.label} (${fmtBytes(blob.size)}).`);
        });
      } else {
        const blob = new Blob([data.data], { type: EXPORTS[data.format].type });
        download(blob, EXPORTS[data.format].suffix);
        setExportStatus(`Saved ${EXPORTS[data.format].label} (${fmtBytes(blob.size)}).`);
      }
      updateButtons();
      break;
    case 'export-progress':
      setExportStatus(`Encoding video… ${Math.round(100 * data.done / data.total)}%`);
      break;
    case 'export-error':
      exporting = null;
      setExportStatus(data.message, true);
      updateButtons();
      break;
    case 'error':
      finish('Something went wrong: ' + data.message + ' Reload the page to try again.', true);
      break;
  }
};
worker.onerror = (e) => finish('The engine crashed (' + e.message + '). Reload the page to try again.', true);

// ---------------------------------------------------------------- loading images

async function useBlob(blob, name, origin = null) {
  if (blob.type && !blob.type.startsWith('image/')) throw new Error('That file is not an image.');
  let bitmap;
  try {
    bitmap = await createImageBitmap(blob);
  } catch {
    throw new Error('Your browser could not decode that image.');
  }
  if (running) {
    worker.postMessage({ type: 'stop' });
    finish('Stopped.');
  }
  source = bitmap;
  sourceOrigin = origin;
  // both frames take the image's shape (the result keeps its aspect ratio)
  document.querySelector('.frames').style.setProperty('--ratio', bitmap.width / bitmap.height);
  sourceName = (name || 'image').replace(/\.[a-z0-9]+$/i, '').replace(/[^\w.-]+/g, '-').slice(0, 60) || 'image';
  const canvas = document.createElement('canvas');
  canvas.width = bitmap.width;
  canvas.height = bitmap.height;
  canvas.getContext('2d').drawImage(bitmap, 0, 0);
  constraintsUI.attach(ui.original, canvas);
  compareUI.setImage(bitmap);
  ui.result.innerHTML = '<p class="placeholder">Press Generate</p>';
  setResultLabel('Primitive');
  lastSvg = null;
  setExportStatus('');
  setStatus(engineReady ? `Loaded ${bitmap.width}×${bitmap.height} image.` : 'Image loaded. Waiting for the engine…');
  updateButtons();
  settingsChanged();
}

const PROXY = (u) => 'https://wsrv.nl/?url=' + encodeURIComponent(u) + '&w=2048&h=2048&fit=inside&we';

async function fetchImage(u) {
  const res = await fetch(u, { referrerPolicy: 'no-referrer' });
  if (!res.ok) throw Object.assign(new Error(`That link returned an error (HTTP ${res.status}).`), { http: true });
  const type = res.headers.get('content-type') || '';
  if (!type.startsWith('image/')) {
    throw Object.assign(new Error('That link points to a web page, not an image. Right-click the picture and choose “Copy image address”.'), { http: true });
  }
  return res.blob();
}

// Turns a Pinterest pin page link into the pin's image URL.
async function pinterestImage(pinId) {
  const res = await fetch('https://widgets.pinterest.com/v3/pidgets/pins/info/?pin_ids=' + pinId);
  const json = await res.json();
  const images = json && json.data && json.data[0] && json.data[0].images;
  if (!images) throw new Error('Could not find that Pinterest pin. Is it public?');
  const best = Object.values(images).sort((a, b) => b.width - a.width)[0];
  return best.url;
}

async function resolveLink(raw) {
  let url;
  try {
    url = new URL(raw.trim());
  } catch {
    throw new Error('That does not look like a link.');
  }
  if (!/^https?:$/.test(url.protocol)) throw new Error('Only http and https links are supported.');
  const host = url.hostname.replace(/^www\./, '');

  if (host === 'pin.it') {
    throw new Error('Short pin.it links cannot be opened from here. Open the pin and copy the full pinterest.com link.');
  }
  const pin = /(^|\.)pinterest\.[a-z.]+$/.test(host) && url.pathname.match(/^\/pin\/(?:[^/]*--)?(\d+)/);
  if (pin) return { url: await pinterestImage(pin[1]), proxy: true };
  if (host === 'unsplash.com' && url.pathname.startsWith('/photos/')) {
    throw new Error('Unsplash photo pages cannot be read directly. Right-click the photo and choose “Copy image address”, then paste that.');
  }
  // i.pinimg.com sends no CORS headers, so skip straight to the proxy.
  return { url: url.href, proxy: host.endsWith('pinimg.com') };
}

async function loadLink(raw) {
  setStatus('Fetching image…');
  const { url, proxy } = await resolveLink(raw);
  const name = decodeURIComponent(new URL(url).pathname.split('/').pop() || 'image');
  let blob;
  if (!proxy) {
    try {
      blob = await fetchImage(url);
    } catch (err) {
      if (err.http) throw err;
      // A network/CORS failure: the site blocks cross-origin requests.
    }
  }
  if (!blob) {
    try {
      blob = await fetchImage(PROXY(url));
    } catch {
      throw new Error('Could not get an image from that link. If it is a web page, right-click the picture and choose “Copy image address”; otherwise save the image and upload the file.');
    }
  }
  // the link as given, so a Pinterest pin is looked up again when shared
  await useBlob(blob, name, { url: raw.trim() });
}

function guard(promise) {
  return promise.catch((err) => setStatus(err.message, true));
}

ui.file.addEventListener('change', () => {
  const f = ui.file.files[0];
  if (f) guard(useBlob(f, f.name));
  ui.file.value = '';
});
ui.drop.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); ui.file.click(); }
});
ui.loadUrl.addEventListener('click', () => ui.url.value && guard(loadLink(ui.url.value)));
ui.url.addEventListener('keydown', (e) => {
  if (e.key === 'Enter') { e.preventDefault(); ui.loadUrl.click(); }
});
const samples = [...document.querySelectorAll('[data-sample]')];
const sampleName = (b) => b.dataset.sample.split('/').pop().replace(/\.\w+$/, '');
function loadSample(b) {
  return guard(fetchImage(b.dataset.sample).then((blob) => useBlob(blob, b.dataset.sample.split('/').pop(), { sample: sampleName(b) })));
}
samples.forEach((b) => b.addEventListener('click', () => loadSample(b)));

// Drag and drop: files, or images dragged from another web page (which arrive as links).
['dragenter', 'dragover'].forEach((t) => document.addEventListener(t, (e) => {
  e.preventDefault();
  ui.drop.classList.add('over');
}));
['dragleave', 'drop'].forEach((t) => document.addEventListener(t, (e) => {
  e.preventDefault();
  if (t === 'drop' || !e.relatedTarget) ui.drop.classList.remove('over');
}));
document.addEventListener('drop', (e) => {
  const dt = e.dataTransfer;
  const f = [...dt.files].find((x) => x.type.startsWith('image/'));
  if (f) return guard(useBlob(f, f.name));
  const link = (dt.getData('text/uri-list') || dt.getData('text/plain')).split('\n').find((l) => /^https?:/.test(l));
  if (link) {
    ui.url.value = link.trim();
    guard(loadLink(link));
  }
});

document.addEventListener('paste', (e) => {
  if (e.target === ui.url) return;
  const item = [...e.clipboardData.items].find((i) => i.type.startsWith('image/'));
  if (item) return guard(useBlob(item.getAsFile(), 'pasted'));
  const text = e.clipboardData.getData('text').trim();
  if (/^https?:\/\//.test(text)) {
    ui.url.value = text;
    guard(loadLink(text));
  }
});

// ---------------------------------------------------------------- running

function readJob() {
  return {
    mode: +ui.mode.value,
    count: readCount(),
    alpha: ui.alphaAuto.checked ? 0 : +ui.alpha.value,
    repeat: +ui.repeat.value,
    inputSize: +ui.inputSize.value,
    outputSize: +ui.outputSize.value,
    threads: readThreads(),
    background: ui.bgAuto.checked ? '' : ui.bg.value.slice(1),
  };
}

ui.form.addEventListener('submit', (e) => {
  e.preventDefault();
  if (!source || !engineReady) return;
  const job = readJob();
  const scale = Math.min(1, job.inputSize / Math.max(source.width, source.height));
  const width = Math.max(1, Math.round(source.width * scale));
  const height = Math.max(1, Math.round(source.height * scale));
  try {
    job.constraints = constraintsUI.read(width, height);
    job.palette = readPalette();
  } catch (err) {
    setStatus(err.message, true);
    return;
  }
  const canvas = new OffscreenCanvas(width, height);
  const ctx = canvas.getContext('2d');
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(source, 0, 0, width, height);
  const pixels = ctx.getImageData(0, 0, width, height).data.buffer;
  const transfer = [pixels];
  if (job.constraints && job.constraints.region) transfer.push(job.constraints.region);
  if (job.constraints && job.constraints.detail) transfer.push(job.constraints.detail);
  keepLayers(job.constraints && job.constraints.keep ? job : null, width, height, transfer);
  lastJob = job;
  lastShapes = 0;
  running = true;
  ui.progress.hidden = false;
  ui.progressBar.style.width = '0';
  setResultLabel('Primitive');
  setStatus('Starting…');
  setExportStatus('');
  updateButtons();
  worker.postMessage({ type: 'start', pixels, width, height, ...job }, transfer);
  if (matchMedia('(max-width: 860px)').matches) ui.result.scrollIntoView({ behavior: 'smooth', block: 'center' });
});

ui.stop.addEventListener('click', () => {
  worker.postMessage({ type: 'stop' });
  finish('Stopped.');
  if (lastShapes) setResultLabel(shapeLabel(lastShapes) + ' (stopped)');
});

function finish(message, isError) {
  running = false;
  ui.progress.hidden = true;
  setStatus(message, isError);
  updateButtons();
}

// ---------------------------------------------------------------- downloads

function download(blob, suffix) {
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = `${sourceName}-primitive${suffix}`;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 10000);
}
ui.dlSvg.addEventListener('click', () => {
  if (!lastSvg) return;
  const count = exportCount();
  if (count === null) downloadSvg(lastSvg, '.svg');
  else worker.postMessage({ type: 'svg', count });
});

// ---------------------------------------------------------------- a kept region

// With "Around the region: the original image", renders and SVGs show the
// original image around the region. The job carries the original and the
// region's mask at the output size for the engine's renders, and SVGs get
// both as images, the shapes masked to the region: in the preview as blob
// URLs (cheap to repeat every update), in downloads as data URLs, so the
// file stands alone.
function keepLayers(job, width, height, transfer) {
  if (kept) kept.urls.then((u) => Object.values(u).forEach(URL.revokeObjectURL));
  kept = null;
  if (!job) return;
  // the output size, as the engine works it out from the working size
  const size = job.outputSize, aspect = width / height;
  const w = aspect >= 1 ? size : Math.floor(size * aspect);
  const h = aspect >= 1 ? Math.floor(size / aspect) : size;
  const canvas = new OffscreenCanvas(w, h);
  const ctx = canvas.getContext('2d');
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(source, 0, 0, w, h);
  const pixels = ctx.getImageData(0, 0, w, h).data.buffer;
  const mask = constraintsUI.regionMask(w, h);
  job.base = { width: w, height: h, pixels, mask };
  transfer.push(pixels, mask.buffer);
  const blobs = Promise.all([canvas.convertToBlob({ type: 'image/jpeg', quality: 0.92 }), constraintsUI.regionPNG()]);
  const urls = blobs.then(([image, region]) => ({ image: URL.createObjectURL(image), region: URL.createObjectURL(region) }));
  kept = { blobs, urls, ready: null };
  const current = kept;
  urls.then((u) => {
    current.ready = u;
    // redraw a preview that arrived before the images were ready
    if (kept === current && lastSvg) showResult(lastSvg);
  });
}

// The SVG with the original image under it and its shapes masked to the
// region, given URLs for both images.
function withKept(svg, { image, region }) {
  const w = /width="(\d+)"/.exec(svg)[1], h = /height="(\d+)"/.exec(svg)[1];
  const open = svg.indexOf('>') + 1, close = svg.lastIndexOf('</svg>');
  const size = `x="0" y="0" width="${w}" height="${h}"`;
  return svg.slice(0, open).replace('<svg ', '<svg xmlns:xlink="http://www.w3.org/1999/xlink" ')
    + `\n<defs><mask id="region" maskUnits="userSpaceOnUse" ${size}>`
    + `<image ${size} preserveAspectRatio="none" xlink:href="${region}" /></mask></defs>`
    + `\n<image ${size} preserveAspectRatio="none" xlink:href="${image}" />`
    + '\n<g mask="url(#region)">' + svg.slice(open, close) + '</g>\n' + svg.slice(close);
}

const dataURL = (blob) => new Promise((resolve, reject) => {
  const r = new FileReader();
  r.onload = () => resolve(r.result);
  r.onerror = () => reject(r.error);
  r.readAsDataURL(blob);
});

// Downloads an SVG, with the original image in it around a kept region.
// Returns the blob saved.
async function downloadSvg(svg, suffix) {
  if (kept) {
    const [image, region] = await Promise.all((await kept.blobs).map(dataURL));
    svg = withKept(svg, { image, region });
  }
  const blob = new Blob([svg], { type: 'image/svg+xml' });
  download(blob, suffix);
  return blob;
}

function requestExport(format, purpose) {
  if (!lastSvg || running || exporting) return;
  exporting = format;
  if (purpose === 'download') setExportStatus(`Making ${EXPORTS[format].label}…`);
  updateButtons();
  worker.postMessage({ type: 'export', format, purpose, duration: +ui.duration.value, size: lastJob.outputSize, count: exportCount() });
}
ui.exportButtons.forEach((b) => b.addEventListener('click', () => requestExport(b.dataset.export, 'download')));
ui.replay.addEventListener('click', () => {
  // replays the whole result, so the slider goes back to the end
  resetScrub();
  requestExport('svg', 'replay');
});
ui.dlPng.addEventListener('click', () => worker.postMessage({ type: 'png', count: exportCount() }));

// ---------------------------------------------------------------- shape-count slider

// Downloads show the shapes the slider shows. While it is hidden (during a
// run) they show everything so far; once shown it also pins a stopped run's
// downloads to the shapes in its preview, as the worker may have added more
// since the last preview.
function exportCount() {
  return ui.scrub.hidden ? null : +ui.scrub.value;
}

function setResultLabel(text) {
  resultLabel = text;
  resetScrub();
}

function resetScrub() {
  ui.scrub.max = Math.max(1, lastShapes);
  ui.scrub.value = ui.scrub.max;
  showShapes(Infinity);
  ui.resultCaption.textContent = resultLabel;
}

function showScrubbed() {
  const k = +ui.scrub.value;
  showShapes(k);
  ui.resultCaption.textContent = k < +ui.scrub.max ? `${k} of ${resultLabel}` : resultLabel;
}

function showResult(svgText) {
  ui.result.innerHTML = kept && kept.ready ? withKept(svgText, kept.ready) : svgText;
  fitSvg(ui.result.querySelector('svg'));
  const group = shapeGroup();
  visible = group ? group.children.length : 0;
  if (!ui.scrub.hidden) showShapes(+ui.scrub.value);
}

// The result SVG has one element per shape, in order, in its group, so
// hiding the ones past k shows the picture as it was after k shapes. Only
// those between the old and new count change, which keeps dragging smooth
// with thousands of shapes. (visibility, unlike display, doesn't restart a
// replay's fade-in when a shape comes back.)
function showShapes(k) {
  const group = shapeGroup();
  if (!group) return;
  const shapes = group.children;
  const end = Math.min(shapes.length, Math.max(k, visible));
  for (let i = Math.min(k, visible); i < end; i++) shapes[i].style.visibility = i < k ? '' : 'hidden';
  visible = k;
}

// The engine's group of shapes in the result SVG: the one scaled to the
// output size (a kept region wraps it in another group).
function shapeGroup() {
  return ui.result.querySelector('svg g[transform]');
}

// one update per frame, however fast the input events come
let scrubQueued = false;
ui.scrub.addEventListener('input', () => {
  if (scrubQueued) return;
  scrubQueued = true;
  requestAnimationFrame(() => {
    scrubQueued = false;
    showScrubbed();
  });
});

// ---------------------------------------------------------------- ui state

function setStatus(text, isError) {
  ui.status.textContent = text;
  ui.status.classList.toggle('error', !!isError);
}

function updateButtons() {
  ui.start.disabled = !source || !engineReady || running;
  ui.start.hidden = running;
  ui.stop.hidden = !running;
  ui.dlPng.disabled = ui.dlSvg.disabled = !lastSvg;
  const noAnim = !lastSvg || running || !!exporting;
  ui.exportButtons.forEach((b) => { b.disabled = noAnim; });
  ui.replay.disabled = noAnim;
  ui.scrub.hidden = running || !lastSvg || lastShapes < 2;
  compareUI.setAvailable(!!lastSvg);
}

// The engine's SVG has a fixed width/height and no viewBox; add one so CSS can scale it.
function fitSvg(svg) {
  if (svg) svg.setAttribute('viewBox', `0 0 ${svg.getAttribute('width')} ${svg.getAttribute('height')}`);
}

function shapeLabel(shapes) {
  return `${shapes} ${MODE_NAMES[lastJob.mode]}`;
}

function setExportStatus(text, isError) {
  ui.exportStatus.textContent = text;
  ui.exportStatus.classList.toggle('error', !!isError);
}

function fmtBytes(n) {
  return n < 1e6 ? `${Math.max(1, Math.round(n / 1e3))} KB` : `${(n / 1e6).toFixed(1)} MB`;
}

function fmtTime(ms) {
  const s = Math.round(ms / 1000);
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, '0')}s`;
}

// ---------------------------------------------------------------- palette

const PALETTE_HINT = 'Hex codes, separated by commas or spaces.';

// Hex codes in text, as 6 lowercase digits, and the words that aren't.
function parseColors(text) {
  const colors = [], bad = [];
  for (const word of text.split(/[\s,;-]+/).filter(Boolean)) {
    const hex = word.replace(/^#/, '').toLowerCase();
    if (/^[0-9a-f]{6}$/.test(hex)) colors.push(hex);
    else if (/^[0-9a-f]{3}$/.test(hex)) colors.push([...hex].map((d) => d + d).join(''));
    else bad.push(word);
  }
  return { colors, bad };
}

// The colors shapes may take, as engine.js init wants them: null for any,
// a number to choose from the image, or a list of hex codes.
function readPalette() {
  if (ui.palette.value !== 'custom') return ui.palette.value ? +ui.palette.value : null;
  const { colors, bad } = parseColors(ui.paletteColors.value);
  if (bad.length || !colors.length) throw new Error('Check your colors under Advanced: ' + ui.paletteHint.textContent);
  return colors;
}

function syncPalette() {
  ui.paletteCustom.hidden = ui.palette.value !== 'custom';
  const { colors, bad } = parseColors(ui.paletteColors.value);
  ui.paletteSwatches.replaceChildren(...colors.map((c) => {
    const swatch = document.createElement('span');
    swatch.style.background = swatch.title = '#' + c;
    return swatch;
  }));
  ui.paletteHint.textContent = bad.length ? `Not a hex code: ${bad.join(', ')}.`
    : colors.length ? PALETTE_HINT : 'Add at least one color.';
  ui.paletteHint.classList.toggle('warn', bad.length > 0 || !colors.length);
}

ui.palette.addEventListener('change', syncPalette);
ui.paletteColors.addEventListener('input', syncPalette);
syncPalette();

// ---------------------------------------------------------------- threads

const THREAD_CHOICES = [1, 2, 4, 6, 8, 12, 16];
let autoThreads = 1;

function setupThreads(maxThreads) {
  const cores = navigator.hardwareConcurrency || 4;
  // leave a core for the page and everything else
  autoThreads = Math.max(1, Math.min(maxThreads, cores - 1));
  ui.threads.options[0].textContent = `Auto (${autoThreads})`;
  for (const n of THREAD_CHOICES) {
    if (n > Math.min(maxThreads, cores)) break;
    ui.threads.add(new Option(n === 1 ? '1 (no parallel search)' : String(n), String(n)));
  }
  worker.postMessage({ type: 'prepare', threads: autoThreads });
}

function readThreads() {
  return ui.threads.value === 'auto' ? autoThreads : +ui.threads.value;
}

// ---------------------------------------------------------------- shape count

const MAX_SHAPES = 10000;
const SLOW_SHAPES = 1000;
// rough shapes per second at a 128 px working size, by shape mode
const SPEED = { 0: 3, 1: 5, 2: 5, 3: 3, 4: 3, 5: 4, 6: 2, 7: 1.8, 8: 3 };
// rough speedup over one thread, as measured on a 12-thread laptop CPU
// (4 performance + 4 efficiency cores): 4 threads ~2.3x, 11 ~4x
const speedup = (threads) => threads ** 0.6;

function readCount() {
  const n = Math.round(+ui.countCustom.value);
  return Number.isFinite(n) ? Math.min(MAX_SHAPES, Math.max(1, n)) : 100;
}

function updateCountWarning() {
  const n = readCount();
  ui.countWarn.hidden = n <= SLOW_SHAPES;
  if (n <= SLOW_SHAPES) return;
  const mode = +ui.mode.value, size = +ui.inputSize.value, threads = readThreads();
  const perSecond = lastSpeed && lastSpeed.mode === mode && lastSpeed.inputSize === size && lastSpeed.threads === threads
    ? lastSpeed.perSecond : SPEED[mode] * 128 / size * speedup(threads);
  ui.countWarn.textContent = `More than ${SLOW_SHAPES} shapes takes a while: roughly ${fmtDuration(n / perSecond)}`
    + ' with these settings. You can stop at any point and keep the result.';
}

function fmtDuration(seconds) {
  const m = Math.round(seconds / 60);
  if (m < 2) return 'a minute';
  if (m < 90) return `${m} minutes`;
  return `${(m / 60).toFixed(1).replace(/\.0$/, '')} hours`;
}

ui.count.addEventListener('input', () => {
  ui.countCustom.value = ui.count.value;
  updateCountWarning();
});
ui.countCustom.addEventListener('input', () => {
  ui.count.value = Math.min(+ui.count.max, readCount());
  updateCountWarning();
});
ui.countCustom.addEventListener('change', () => {
  ui.countCustom.value = readCount();
  updateCountWarning();
});
ui.mode.addEventListener('change', updateCountWarning);
ui.inputSize.addEventListener('change', updateCountWarning);
ui.threads.addEventListener('change', updateCountWarning);

for (const [input, out, unit = ''] of [['alpha', 'alpha-out'], ['repeat', 'repeat-out'], ['duration', 'duration-out', 's']]) {
  const sync = () => { $(out).textContent = $(input).value + unit; };
  $(input).addEventListener('input', sync);
  sync();
}
ui.alphaAuto.addEventListener('change', () => {
  ui.alpha.disabled = ui.alphaAuto.checked;
  $('alpha-out').textContent = ui.alphaAuto.checked ? 'auto' : ui.alpha.value;
});
ui.bgAuto.addEventListener('change', () => { ui.bg.disabled = ui.bgAuto.checked; });

// ---------------------------------------------------------------- share links

// A share link carries the settings in its query, named after the
// command-line tool's flags (?m=5&n=200&a=128), and the image when it came
// from a link or is a sample; uploaded images never leave the device.
// Settings at their defaults are left out to keep links short. CPU threads
// are left out too: the right number depends on the device opening the link.

// Encodes a value but leaves the characters that are safe in a query, like
// the slashes of an image link, so links stay readable.
const linkValue = (v) => encodeURIComponent(v)
  .replace(/%(2C|2F|3A|3D|3F|40)/g, (_, hex) => String.fromCharCode(parseInt(hex, 16)));
const isDefault = (input) => (input.tagName === 'SELECT'
  ? input.selectedOptions[0].defaultSelected : input.value === input.defaultValue);
const fraction = (v) => String(+v.toFixed(2));

function shareLink() {
  const job = readJob();
  const c = constraintsUI.settings();
  const params = [];
  if (!isDefault(ui.mode)) params.push(['m', job.mode]);
  if (!isDefault(ui.countCustom)) params.push(['n', job.count]);
  if (ui.alphaAuto.checked || !isDefault(ui.alpha)) params.push(['a', job.alpha]);
  if (!isDefault(ui.repeat)) params.push(['rep', job.repeat]);
  if (!isDefault(ui.inputSize)) params.push(['r', job.inputSize]);
  if (!isDefault(ui.outputSize)) params.push(['s', job.outputSize]);
  if (job.background) params.push(['bg', job.background]);
  if (c.focus) params.push(['focus', c.focus.map(fraction).join(',')]);
  if (c.angle !== undefined) params.push(['angle', c.angle]);
  if (c.minSize) params.push(['minsize', fraction(c.minSize)]);
  if (c.maxSize) params.push(['maxsize', fraction(c.maxSize)]);
  if (sourceOrigin && sourceOrigin.sample) params.push(['sample', sourceOrigin.sample]);
  if (sourceOrigin && sourceOrigin.url) params.push(['url', sourceOrigin.url]); // last, as the longest
  const url = new URL(location.href);
  url.search = params.map(([k, v]) => k + '=' + linkValue(v)).join('&');
  url.hash = '';
  return url.href;
}

// Keeps the address bar in step with the settings, so a reload keeps them
// and the address works as a share link too.
function settingsChanged() {
  const link = shareLink();
  if (link !== location.href) history.replaceState(null, '', link);
  ui.shareBox.hidden = true; // what it says may be out of date now
}
ui.form.addEventListener('change', settingsChanged);

let shareTimer = 0;
ui.share.addEventListener('click', async () => {
  const link = shareLink();
  const notes = [];
  if (source && !sourceOrigin) notes.push('Your image is not included: only samples and image links can be.');
  if (constraintsUI.settings().region) notes.push('The painted region is not included.');
  let copied = true;
  try {
    await navigator.clipboard.writeText(link);
  } catch {
    copied = false; // no clipboard access, e.g. permission denied or not https
  }
  clearTimeout(shareTimer);
  ui.shareNote.textContent = [copied ? 'Link copied.' : 'Could not copy the link. Copy it from here:', ...notes].join(' ');
  ui.shareLink.value = link;
  ui.shareLink.hidden = copied;
  ui.shareBox.hidden = false;
  if (!copied) ui.shareLink.select();
  // a plain confirmation goes away; notes stay until the settings change
  else if (!notes.length) shareTimer = setTimeout(() => { ui.shareBox.hidden = true; }, 2500);
});
ui.shareLink.addEventListener('focus', () => ui.shareLink.select());

// Applies a share link's settings, skipping any value the controls would
// not accept, then loads its image. Older links with just ?url= work too.
function applyLink(params) {
  const get = (k) => (params.get(k) || '').trim();
  const num = (v) => (v.trim() ? Number(v) : NaN);
  // Sets a <select> or number input when the value is one it accepts. The
  // events don't bubble, so the form leaves the address alone until the
  // image has loaded.
  const set = (input, v) => {
    const ok = input.tagName === 'SELECT' ? [...input.options].some((o) => o.value === v)
      : /^\d+$/.test(v) && +v >= +input.min && +v <= +input.max;
    if (ok) {
      input.value = v;
      input.dispatchEvent(new Event(input.tagName === 'SELECT' ? 'change' : 'input'));
    }
    return ok;
  };
  set(ui.mode, get('m'));
  set(ui.countCustom, get('n'));
  if (set(ui.alpha, get('a')) || get('a') === '0') {
    ui.alphaAuto.checked = get('a') === '0';
    ui.alphaAuto.dispatchEvent(new Event('change'));
  }
  set(ui.repeat, get('rep'));
  set(ui.inputSize, get('r'));
  set(ui.outputSize, get('s'));
  const bg = get('bg').replace(/^#/, '');
  if (/^[0-9a-f]{6}$/i.test(bg)) {
    ui.bg.value = '#' + bg.toLowerCase();
    ui.bgAuto.checked = false;
    ui.bgAuto.dispatchEvent(new Event('change'));
  }
  constraintsUI.apply({
    focus: get('focus') ? get('focus').split(',').map(num) : null,
    angle: num(get('angle')), minSize: num(get('minsize')), maxSize: num(get('maxsize')),
  });

  const sample = samples.find((b) => sampleName(b) === get('sample'));
  if (sample) {
    loadSample(sample);
  } else if (get('url')) {
    ui.url.value = get('url');
    guard(loadLink(get('url')));
  }
}

if (location.search) applyLink(new URLSearchParams(location.search));
