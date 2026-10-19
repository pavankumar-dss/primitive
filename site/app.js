'use strict';

const $ = (id) => document.getElementById(id);
const ui = {
  form: $('controls'), drop: $('drop'), file: $('file'), url: $('url'), loadUrl: $('load-url'),
  mode: $('mode'), count: $('count'), alpha: $('alpha'), alphaAuto: $('alpha-auto'),
  inputSize: $('input-size'), outputSize: $('output-size'), repeat: $('repeat'),
  bgAuto: $('bg-auto'), bg: $('bg'), start: $('start'), stop: $('stop'), status: $('status'),
  progress: $('progress'), progressBar: $('progress-bar'),
  original: $('original'), result: $('result'), resultCaption: $('result-caption'),
  dlPng: $('dl-png'), dlSvg: $('dl-svg'),
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
let lastDone = 0;        // steps completed in the current run

// ---------------------------------------------------------------- worker

const worker = new Worker('worker.js');
worker.onmessage = ({ data }) => {
  switch (data.type) {
    case 'ready':
      engineReady = true;
      setStatus(source ? 'Ready.' : 'Ready. Choose an image to start.');
      updateButtons();
      break;
    case 'progress':
      if (!running) break;
      lastDone = data.done;
      lastSvg = data.svg;
      ui.result.innerHTML = data.svg;
      fitSvg(ui.result.querySelector('svg'));
      ui.progressBar.style.width = (100 * data.done / data.count) + '%';
      setStatus(`Shape ${data.done} of ${data.count} · ${fmtTime(data.elapsed)}`
        + ` · ${(data.done / data.elapsed * 1000).toFixed(1)} shapes/s`);
      break;
    case 'done':
      finish(`Done: ${shapeLabel(lastJob.count)} in ${fmtTime(data.elapsed)}.`);
      ui.resultCaption.textContent = shapeLabel(lastJob.count);
      break;
    case 'png':
      if (data.bytes) download(new Blob([data.bytes], { type: 'image/png' }), 'png');
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
  sourceName = (name || 'image').replace(/\.[a-z0-9]+$/i, '').replace(/[^\w.-]+/g, '-').slice(0, 60) || 'image';
  const canvas = document.createElement('canvas');
  canvas.width = bitmap.width;
  canvas.height = bitmap.height;
  canvas.getContext('2d').drawImage(bitmap, 0, 0);
  ui.original.replaceChildren(canvas);
  ui.result.innerHTML = '<p class="placeholder">Press Generate</p>';
  ui.resultCaption.textContent = 'Primitive';
  lastSvg = null;
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
    count: +ui.count.value,
    alpha: ui.alphaAuto.checked ? 0 : +ui.alpha.value,
    repeat: +ui.repeat.value,
    inputSize: +ui.inputSize.value,
    outputSize: +ui.outputSize.value,
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
  const canvas = new OffscreenCanvas(width, height);
  const ctx = canvas.getContext('2d');
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(source, 0, 0, width, height);
  const pixels = ctx.getImageData(0, 0, width, height).data.buffer;
  lastJob = job;
  lastDone = 0;
  running = true;
  ui.progress.hidden = false;
  ui.progressBar.style.width = '0';
  ui.resultCaption.textContent = 'Primitive';
  setStatus('Starting…');
  updateButtons();
  worker.postMessage({ type: 'start', pixels, width, height, ...job }, [pixels]);
  if (matchMedia('(max-width: 860px)').matches) ui.result.scrollIntoView({ behavior: 'smooth', block: 'center' });
});

ui.stop.addEventListener('click', () => {
  worker.postMessage({ type: 'stop' });
  finish('Stopped.');
  if (lastDone) ui.resultCaption.textContent = shapeLabel(lastDone) + ' (stopped)';
});

function finish(message, isError) {
  running = false;
  ui.progress.hidden = true;
  setStatus(message, isError);
  updateButtons();
}

// ---------------------------------------------------------------- downloads

function download(blob, ext) {
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = `${sourceName}-primitive.${ext}`;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 10000);
}
ui.dlSvg.addEventListener('click', () => lastSvg && download(new Blob([lastSvg], { type: 'image/svg+xml' }), 'svg'));
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
}

// The Go SVG has a fixed width/height and no viewBox; add one so CSS can scale it.
function fitSvg(svg) {
  if (svg) svg.setAttribute('viewBox', `0 0 ${svg.getAttribute('width')} ${svg.getAttribute('height')}`);
}

// Each step adds one shape plus `repeat` extra ones.
function shapeLabel(steps) {
  return `${steps * (1 + lastJob.repeat)} ${MODE_NAMES[lastJob.mode]}`;
}

function fmtTime(ms) {
  const s = Math.round(ms / 1000);
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, '0')}s`;
}

for (const [input, out] of [['count', 'count-out'], ['alpha', 'alpha-out'], ['repeat', 'repeat-out']]) {
  const sync = () => { $(out).textContent = $(input).value; };
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
