'use strict';

// Constraint controls: direction (toward a spot or along an angle), shape
// size limits, and a painted region, set by clicking and painting on the
// original image (or by clicking a subject for segment-worker.js to find).
// app.js calls constraintsUI.attach() when an image loads and
// constraintsUI.read() when a run starts; settings() and apply() carry the
// controls in and out of share links.
const constraintsUI = (() => {
  const $ = (id) => document.getElementById(id);
  const el = {
    panel: $('constraints'), badge: $('c-badge'),
    direction: $('c-direction'), focusHint: $('c-focus-hint'),
    focusPos: $('c-focus-pos'), focusReset: $('c-focus-reset'),
    angleField: $('c-angle-field'), angle: $('c-angle'), angleOut: $('c-angle-out'),
    warn: $('c-direction-warn'), switchMode: $('c-switch'), mode: $('mode'),
    min: $('c-min'), minOut: $('c-min-out'), max: $('c-max'), maxOut: $('c-max-out'),
    region: $('c-region'), regionTools: $('c-region-tools'), brush: $('c-brush'),
    clear: $('c-clear'), invert: $('c-invert'), outside: $('c-outside'),
    subject: $('c-subject'), subjectStatus: $('c-subject-status'),
    tools: $('tools'), toolButtons: [...document.querySelectorAll('[data-tool]')],
  };
  const ORIENTED_MODES = ['0', '5', '6', '7']; // combo, rotated rect, Bézier, rotated ellipse
  const MASK_SIZE = 512;

  let focus = { x: 0.5, y: 0.5 };
  let tool = null;     // 'spot', 'paint' or 'erase' while one is usable
  let mask = null;     // canvas: opaque where shapes may go
  let overlay = null;  // canvas shown over the original image
  let marker = null;
  let painting = false;
  let lastPoint = null;
  let angleGuides = false; // parallels show briefly while the angle changes
  let angleTimer = 0;
  let source = null;   // the original image's canvas

  // subject selection (see segment-worker.js)
  let segmenter = null;  // the worker, once someone asks for a subject
  let points = [];       // clicks so far: {x, y, include}, in fractions
  let seq = 0;           // numbers requests, so late masks are dropped
  let inverted = false;  // Invert was pressed: found subjects become holes

  // ------------------------------------------------------------ controls

  function sync() {
    const dir = el.direction.value;
    el.focusHint.hidden = dir !== 'focus';
    el.angleField.hidden = dir !== 'angle';
    el.warn.hidden = !dir || ORIENTED_MODES.includes(el.mode.value);
    el.angleOut.textContent = el.angle.value + '°';
    el.minOut.textContent = +el.min.value ? el.min.value + '%' : 'any';
    el.maxOut.textContent = +el.max.value < 100 ? el.max.value + '%' : 'any';
    el.regionTools.hidden = !el.region.checked;

    // offer only the tools that apply, and keep a valid one selected
    const available = [];
    if (dir === 'focus') available.push('spot');
    if (el.region.checked && segmenter) available.push('pick', 'unpick');
    if (el.region.checked) available.push('paint', 'erase');
    el.toolButtons.forEach((b) => {
      b.hidden = !available.includes(b.dataset.tool);
      b.classList.toggle('on', b.dataset.tool === tool);
    });
    if (!available.includes(tool)) setTool(available[0] || null);
    el.tools.hidden = available.length === 0;

    const active = [dir, +el.min.value > 0 || +el.max.value < 100, el.region.checked].filter(Boolean).length;
    el.badge.textContent = active ? `${active} on` : '';
    el.badge.hidden = !active;
    draw();
  }

  function setTool(t) {
    tool = t;
    el.toolButtons.forEach((b) => b.classList.toggle('on', b.dataset.tool === t));
    if (overlay) overlay.classList.toggle('active', !!t);
  }

  el.direction.addEventListener('change', () => {
    if (el.direction.value === 'focus') setTool('spot');
    sync();
  });
  el.region.addEventListener('change', () => {
    if (el.region.checked) setTool('paint');
    sync();
  });
  el.min.addEventListener('input', () => {
    if (+el.min.value >= +el.max.value) el.max.value = Math.min(100, +el.min.value + 2);
    sync();
  });
  el.max.addEventListener('input', () => {
    if (+el.max.value <= +el.min.value) el.min.value = Math.max(0, +el.max.value - 2);
    sync();
  });
  el.angle.addEventListener('input', () => {
    angleGuides = true;
    clearTimeout(angleTimer);
    angleTimer = setTimeout(() => { angleGuides = false; draw(); }, 900);
    sync();
  });
  el.mode.addEventListener('change', sync);
  el.switchMode.addEventListener('click', () => {
    el.mode.value = '5';
    el.mode.dispatchEvent(new Event('change', { bubbles: true })); // updates the shape picker and sync()
  });
  el.toolButtons.forEach((b) => b.addEventListener('click', () => setTool(b.dataset.tool)));
  el.focusReset.addEventListener('click', () => {
    moveFocus({ x: 0.5, y: 0.5 });
    changed();
  });
  el.clear.addEventListener('click', () => {
    if (!mask) return;
    mask.getContext('2d').clearRect(0, 0, mask.width, mask.height);
    points = [];
    seq++;
    if (segmenter) subjectStatus('ready');
    draw();
  });
  el.invert.addEventListener('click', () => {
    if (!mask) return;
    inverted = !inverted;
    const flipped = document.createElement('canvas');
    flipped.width = mask.width;
    flipped.height = mask.height;
    const ctx = flipped.getContext('2d');
    ctx.fillStyle = '#fff';
    ctx.fillRect(0, 0, mask.width, mask.height);
    ctx.globalCompositeOperation = 'destination-out';
    ctx.drawImage(mask, 0, 0);
    const m = mask.getContext('2d');
    m.clearRect(0, 0, mask.width, mask.height);
    m.drawImage(flipped, 0, 0);
    draw();
  });

  // ------------------------------------------------------------ subject selection

  // The first press loads the model and looks at the image; then clicks
  // with Select (or Exclude) add points the subject includes (or not).
  el.subject.addEventListener('click', () => {
    if (!segmenter) {
      segmenter = new Worker('segment-worker.js', { type: 'module' });
      segmenter.onmessage = onSegment;
      segmenter.onerror = (e) => {
        e.preventDefault();
        subjectStatus('error', e.message || 'the selection model could not start');
      };
      sendImage();
    }
    setTool('pick');
    sync();
  });

  function sendImage() {
    if (!segmenter || !source) return;
    // the model looks at 1024 pixels at most anyway
    const scale = Math.min(1, 1024 / Math.max(source.width, source.height));
    const w = Math.max(1, Math.round(source.width * scale));
    const h = Math.max(1, Math.round(source.height * scale));
    const canvas = new OffscreenCanvas(w, h);
    const ctx = canvas.getContext('2d');
    ctx.imageSmoothingQuality = 'high';
    ctx.drawImage(source, 0, 0, w, h);
    const pixels = ctx.getImageData(0, 0, w, h).data.buffer;
    segmenter.postMessage({ type: 'image', pixels, width: w, height: h }, [pixels]);
    subjectStatus('looking');
  }

  function onSegment({ data }) {
    if (data.type === 'progress') subjectStatus('loading', data.loaded);
    else if (data.type === 'looking') subjectStatus('looking');
    else if (data.type === 'ready') subjectStatus(points.length ? 'selected' : 'ready');
    else if (data.type === 'error') subjectStatus('error', data.message);
    else if (data.type === 'mask' && data.seq === seq) {
      // the found subject becomes the region (or a hole in it, inverted)
      const canvas = new OffscreenCanvas(data.width, data.height);
      const ctx = canvas.getContext('2d');
      const img = ctx.createImageData(data.width, data.height);
      for (let i = 0; i < data.data.length; i++) {
        img.data[i * 4] = img.data[i * 4 + 1] = img.data[i * 4 + 2] = 255;
        img.data[i * 4 + 3] = inverted ? 255 - data.data[i] : data.data[i];
      }
      ctx.putImageData(img, 0, 0);
      const m = mask.getContext('2d');
      m.globalCompositeOperation = 'source-over';
      m.clearRect(0, 0, mask.width, mask.height);
      m.imageSmoothingQuality = 'high';
      m.drawImage(canvas, 0, 0, mask.width, mask.height);
      subjectStatus('selected');
      draw();
    }
  }

  function addPoint(p, include) {
    points.push({ x: p.x, y: p.y, include });
    segmenter.postMessage({ type: 'points', seq: ++seq, points });
    draw();
  }

  function subjectStatus(state, detail) {
    el.subjectStatus.hidden = false;
    el.subjectStatus.classList.toggle('warn', state === 'error');
    el.subjectStatus.textContent = {
      loading: `Loading the selection model (28 MB, only the first time)… ${Math.round((detail || 0) * 100)}%`,
      looking: 'Looking at the image… (a few seconds)',
      ready: 'Click the subject on the original image.',
      selected: 'Click to add more with Select, click away parts with Exclude, or touch up with Paint and Erase.',
      error: `Subject selection failed (${String(detail).replace(/\.$/, '')}). Paint the region instead.`,
    }[state];
  }

  // ------------------------------------------------------------ the original image

  // Wraps the original image's canvas with a paintable overlay and a marker.
  function attach(frame, imageCanvas) {
    source = imageCanvas;
    points = [];
    seq++;
    inverted = false;
    sendImage();
    const scale = Math.min(1, MASK_SIZE / Math.max(imageCanvas.width, imageCanvas.height));
    mask = document.createElement('canvas');
    mask.width = Math.max(1, Math.round(imageCanvas.width * scale));
    mask.height = Math.max(1, Math.round(imageCanvas.height * scale));
    overlay = document.createElement('canvas');
    overlay.className = 'overlay';
    overlay.width = mask.width;
    overlay.height = mask.height;
    marker = document.createElement('div');
    marker.className = 'marker';

    const wrap = document.createElement('div');
    wrap.className = 'canvas-wrap';
    wrap.style.aspectRatio = `${imageCanvas.width} / ${imageCanvas.height}`;
    wrap.classList.toggle('tall', imageCanvas.height > imageCanvas.width);
    wrap.append(imageCanvas, overlay, marker);
    frame.replaceChildren(wrap);

    // guide line widths depend on the displayed size
    new ResizeObserver(() => draw()).observe(overlay);
    overlay.addEventListener('pointerdown', onDown);
    overlay.addEventListener('pointermove', onMove);
    overlay.addEventListener('pointerup', onUp);
    overlay.addEventListener('pointercancel', onUp);
    setTool(tool);
    sync();
  }

  function pointFrom(e) {
    const r = overlay.getBoundingClientRect();
    return {
      x: Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)),
      y: Math.min(1, Math.max(0, (e.clientY - r.top) / r.height)),
    };
  }

  function onDown(e) {
    if (!tool) return;
    e.preventDefault();
    overlay.setPointerCapture(e.pointerId);
    const p = pointFrom(e);
    if (tool === 'pick' || tool === 'unpick') {
      addPoint(p, tool === 'pick');
      return;
    }
    if (tool === 'spot') {
      moveFocus(p);
      painting = true; // dragging moves the spot too
    } else {
      painting = true;
      lastPoint = p;
      paint(p, p);
    }
    draw();
  }

  function onMove(e) {
    if (!painting) return;
    const p = pointFrom(e);
    if (tool === 'spot') {
      moveFocus(p);
    } else {
      paint(lastPoint, p);
      lastPoint = p;
    }
    draw();
  }

  function onUp() {
    if (painting && tool === 'spot') changed();
    painting = false;
    lastPoint = null;
    draw(); // hides the guide rays
  }

  // Moving the spot is not a form control change, so announce it like one
  // for app.js, which keeps the address bar's link up to date.
  function changed() {
    el.panel.dispatchEvent(new Event('change', { bubbles: true }));
  }

  function moveFocus(p) {
    focus = p;
    const pct = (v) => Math.round(v * 100) + '%';
    const centered = Math.abs(p.x - 0.5) < 0.02 && Math.abs(p.y - 0.5) < 0.02;
    el.focusPos.textContent = centered ? 'in the center' : `${pct(p.x)} across, ${pct(p.y)} down`;
    if (marker) {
      // restart the pulse animation
      marker.classList.remove('pulse');
      void marker.offsetWidth;
      marker.classList.add('pulse');
    }
    draw();
  }

  function paint(a, b) {
    const ctx = mask.getContext('2d');
    ctx.globalCompositeOperation = tool === 'erase' ? 'destination-out' : 'source-over';
    ctx.strokeStyle = '#fff';
    ctx.lineCap = 'round';
    ctx.lineWidth = (+el.brush.value / 100) * Math.max(mask.width, mask.height);
    ctx.beginPath();
    ctx.moveTo(a.x * mask.width, a.y * mask.height);
    ctx.lineTo(b.x * mask.width + 0.01, b.y * mask.height);
    ctx.stroke();
  }

  function draw() {
    if (!overlay) return;
    const ctx = overlay.getContext('2d');
    ctx.clearRect(0, 0, overlay.width, overlay.height);
    if (el.region.checked) {
      // dim everything outside the painted region
      ctx.globalCompositeOperation = 'source-over';
      ctx.fillStyle = 'rgba(10, 12, 16, 0.6)';
      ctx.fillRect(0, 0, overlay.width, overlay.height);
      ctx.globalCompositeOperation = 'destination-out';
      ctx.drawImage(mask, 0, 0);
      ctx.globalCompositeOperation = 'source-over';
    }
    // the subject's clicks: light for Select, dark for Exclude
    if (el.region.checked) {
      const px = overlay.width / Math.max(1, overlay.clientWidth);
      for (const pt of points) {
        ctx.beginPath();
        ctx.arc(pt.x * overlay.width, pt.y * overlay.height, 5 * px, 0, 2 * Math.PI);
        ctx.fillStyle = pt.include ? '#fff' : '#111';
        ctx.strokeStyle = pt.include ? '#111' : '#fff';
        ctx.lineWidth = 2 * px;
        ctx.fill();
        ctx.stroke();
      }
    }
    // guides only while the direction is being changed
    if (el.direction.value === 'focus' && painting && tool === 'spot') drawRays(ctx);
    if (el.direction.value === 'angle' && angleGuides) drawParallels(ctx);
    marker.hidden = el.direction.value !== 'focus';
    marker.style.left = focus.x * 100 + '%';
    marker.style.top = focus.y * 100 + '%';
  }

  // Guide lines are drawn twice, dark then light, to show on any image.
  function guide(ctx, lines) {
    const px = overlay.width / Math.max(1, overlay.clientWidth); // canvas pixels per CSS pixel
    for (const [color, width] of [['rgba(0, 0, 0, 0.45)', 3], ['rgba(255, 255, 255, 0.75)', 1.25]]) {
      ctx.strokeStyle = color;
      ctx.lineWidth = width * px;
      ctx.setLineDash([6 * px, 5 * px]);
      ctx.beginPath();
      for (const [x0, y0, x1, y1] of lines) {
        ctx.moveTo(x0, y0);
        ctx.lineTo(x1, y1);
      }
      ctx.stroke();
    }
    ctx.setLineDash([]);
  }

  // Rays from the spot: the directions shapes will point in.
  function drawRays(ctx) {
    const w = overlay.width, h = overlay.height;
    const fx = focus.x * w, fy = focus.y * h;
    const reach = Math.hypot(w, h);
    const inner = 22 * overlay.width / Math.max(1, overlay.clientWidth); // start outside the marker
    const lines = [];
    for (let i = 0; i < 16; i++) {
      const a = (i / 16) * 2 * Math.PI;
      const dx = Math.cos(a), dy = Math.sin(a);
      lines.push([fx + dx * inner, fy + dy * inner, fx + dx * reach, fy + dy * reach]);
    }
    guide(ctx, lines);
  }

  // Parallel lines along the fixed angle.
  function drawParallels(ctx) {
    const w = overlay.width, h = overlay.height;
    const a = (+el.angle.value * Math.PI) / 180;
    const dx = Math.cos(a), dy = -Math.sin(a); // counterclockwise; canvas y points down
    const nx = -dy, ny = dx;
    const reach = Math.hypot(w, h);
    const lines = [];
    for (let k = -4; k <= 4; k++) {
      const cx = w / 2 + nx * k * reach / 9, cy = h / 2 + ny * k * reach / 9;
      lines.push([cx - dx * reach, cy - dy * reach, cx + dx * reach, cy + dy * reach]);
    }
    guide(ctx, lines);
  }

  // ------------------------------------------------------------ reading

  // Returns the constraints for a run on a width x height working image, or
  // null when none are set. Throws if a region is on but empty.
  function read(width, height) {
    const c = {};
    if (el.direction.value === 'focus') c.focus = [focus.x, focus.y];
    if (el.direction.value === 'angle') c.angle = +el.angle.value;
    if (+el.min.value > 0) c.minSize = +el.min.value / 100;
    if (+el.max.value < 100) c.maxSize = +el.max.value / 100;
    if (el.region.checked && mask) {
      const canvas = new OffscreenCanvas(width, height);
      const ctx = canvas.getContext('2d');
      ctx.drawImage(mask, 0, 0, width, height);
      const rgba = ctx.getImageData(0, 0, width, height).data;
      const region = new Uint8Array(width * height);
      let painted = 0;
      for (let i = 0; i < region.length; i++) {
        if (rgba[i * 4 + 3] >= 128) {
          region[i] = 255;
          painted++;
        }
      }
      if (!painted) throw new Error('Paint the area where shapes may go on the original image, or turn off “Only paint inside a region”.');
      c.region = region.buffer;
      // shapes are clipped to the region, and the image around it stays
      if (el.outside.value === 'original') c.keep = true;
    }
    return Object.keys(c).length ? c : null;
  }

  // ------------------------------------------------------------ share links

  // The constraints a share link can carry, in read()'s units. The painted
  // region is too big for a link, so this only says whether one is on.
  function settings() {
    const c = { region: el.region.checked };
    if (el.direction.value === 'focus') c.focus = [focus.x, focus.y];
    if (el.direction.value === 'angle') c.angle = +el.angle.value;
    if (+el.min.value > 0) c.minSize = +el.min.value / 100;
    if (+el.max.value < 100) c.maxSize = +el.max.value / 100;
    return c;
  }

  // Sets the controls from a link's values, skipping any that are missing
  // or out of range.
  function apply(c) {
    const fits = (input, v) => Number.isInteger(v) && v >= +input.min && v <= +input.max;
    if (c.focus && c.focus.length === 2 && c.focus.every((v) => v >= 0 && v <= 1)) {
      el.direction.value = 'focus';
      moveFocus({ x: c.focus[0], y: c.focus[1] });
    } else if (Number.isFinite(c.angle)) {
      el.direction.value = 'angle';
      el.angle.value = ((Math.round(c.angle) % 180) + 180) % 180; // the same line either way round
    }
    const min = Math.round(c.minSize * 100), max = Math.round(c.maxSize * 100);
    if (fits(el.max, max)) el.max.value = max;
    if (fits(el.min, min) && min < +el.max.value) el.min.value = min;
    sync();
  }

  sync();
  // The painted region at width x height, smoothly scaled: a byte per
  // pixel, 255 inside. For drawing the original image around it.
  function regionMask(width, height) {
    const canvas = new OffscreenCanvas(width, height);
    const ctx = canvas.getContext('2d');
    ctx.imageSmoothingQuality = 'high';
    ctx.drawImage(mask, 0, 0, width, height);
    const rgba = ctx.getImageData(0, 0, width, height).data;
    const out = new Uint8Array(width * height);
    for (let i = 0; i < out.length; i++) out[i] = rgba[i * 4 + 3];
    return out;
  }

  // The painted region as a PNG, white inside and black outside, for an SVG
  // mask.
  function regionPNG() {
    const canvas = new OffscreenCanvas(mask.width, mask.height);
    const ctx = canvas.getContext('2d');
    ctx.fillStyle = '#000';
    ctx.fillRect(0, 0, mask.width, mask.height);
    ctx.drawImage(mask, 0, 0);
    return canvas.convertToBlob({ type: 'image/png' });
  }

  return { attach, read, settings, apply, regionMask, regionPNG };
})();
