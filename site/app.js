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
  dlPng: $('dl-png'), dlSvg: $('dl-svg'), replay: $('replay'), duration: $('duration'),
  exportButtons: [...document.querySelectorAll('[data-export]')], exportStatus: $('export-status'),
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
let engineReady = false;
let running = false;
let lastSvg = null;
let lastJob = null;
let lastShapes = 0;      // shapes in the current result
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
      ui.result.innerHTML = data.svg;
      fitSvg(ui.result.querySelector('svg'));
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
      ui.resultCaption.textContent = shapeLabel(data.shapes);
      break;
    }
    case 'png':
      if (data.bytes) download(new Blob([data.bytes], { type: 'image/png' }), '.png');
      break;
    case 'export':
      exporting = null;
      if (data.purpose === 'replay') {
        ui.result.innerHTML = data.data;
        fitSvg(ui.result.querySelector('svg'));
        setExportStatus('');
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

async function useBlob(blob, name) {
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
  // both frames take the image's shape (the result keeps its aspect ratio)
  document.querySelector('.frames').style.setProperty('--ratio', bitmap.width / bitmap.height);
  sourceName = (name || 'image').replace(/\.[a-z0-9]+$/i, '').replace(/[^\w.-]+/g, '-').slice(0, 60) || 'image';
  const canvas = document.createElement('canvas');
  canvas.width = bitmap.width;
  canvas.height = bitmap.height;
  canvas.getContext('2d').drawImage(bitmap, 0, 0);
  constraintsUI.attach(ui.original, canvas);
  ui.result.innerHTML = '<p class="placeholder">Press Generate</p>';
  ui.resultCaption.textContent = 'Primitive';
  lastSvg = null;
  setExportStatus('');
  setStatus(engineReady ? `Loaded ${bitmap.width}×${bitmap.height} image.` : 'Image loaded. Waiting for the engine…');
  updateButtons();
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
  await useBlob(blob, name);
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
document.querySelectorAll('[data-sample]').forEach((b) => b.addEventListener('click', () => {
  guard(fetchImage(b.dataset.sample).then((blob) => useBlob(blob, b.dataset.sample.split('/').pop())));
}));

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
  lastJob = job;
  lastShapes = 0;
  running = true;
  ui.progress.hidden = false;
  ui.progressBar.style.width = '0';
  ui.resultCaption.textContent = 'Primitive';
  setStatus('Starting…');
  setExportStatus('');
  updateButtons();
  const transfer = [pixels];
  if (job.constraints && job.constraints.region) transfer.push(job.constraints.region);
  worker.postMessage({ type: 'start', pixels, width, height, ...job }, transfer);
  if (matchMedia('(max-width: 860px)').matches) ui.result.scrollIntoView({ behavior: 'smooth', block: 'center' });
});

ui.stop.addEventListener('click', () => {
  worker.postMessage({ type: 'stop' });
  finish('Stopped.');
  if (lastShapes) ui.resultCaption.textContent = shapeLabel(lastShapes) + ' (stopped)';
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
ui.dlSvg.addEventListener('click', () => lastSvg && download(new Blob([lastSvg], { type: 'image/svg+xml' }), '.svg'));

function requestExport(format, purpose) {
  if (!lastSvg || running || exporting) return;
  exporting = format;
  if (purpose === 'download') setExportStatus(`Making ${EXPORTS[format].label}…`);
  updateButtons();
  worker.postMessage({ type: 'export', format, purpose, duration: +ui.duration.value, size: lastJob.outputSize });
}
ui.exportButtons.forEach((b) => b.addEventListener('click', () => requestExport(b.dataset.export, 'download')));
ui.replay.addEventListener('click', () => requestExport('svg', 'replay'));
ui.dlPng.addEventListener('click', () => worker.postMessage({ type: 'png' }));

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

// Support sharing links like ?url=https://images.unsplash.com/...
const shared = new URLSearchParams(location.search).get('url');
if (shared) {
  ui.url.value = shared;
  guard(loadLink(shared));
}
