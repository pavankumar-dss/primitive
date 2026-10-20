'use strict';

// Constraint controls: direction (toward a spot or along an angle), shape
// size limits, and a painted region, set by clicking and painting on the
// original image. app.js calls constraintsUI.attach() when an image loads
// and constraintsUI.read() when a run starts; settings() and apply() carry
// the controls in and out of share links.
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
    clear: $('c-clear'), invert: $('c-invert'),
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
    draw();
  });
  el.invert.addEventListener('click', () => {
    if (!mask) return;
    const inverted = document.createElement('canvas');
    inverted.width = mask.width;
    inverted.height = mask.height;
    const ctx = inverted.getContext('2d');
    ctx.fillStyle = '#fff';
    ctx.fillRect(0, 0, mask.width, mask.height);
    ctx.globalCompositeOperation = 'destination-out';
    ctx.drawImage(mask, 0, 0);
    const m = mask.getContext('2d');
    m.clearRect(0, 0, mask.width, mask.height);
    m.drawImage(inverted, 0, 0);
    draw();
  });

  // ------------------------------------------------------------ the original image

  // Wraps the original image's canvas with a paintable overlay and a marker.
  function attach(frame, imageCanvas) {
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
  return { attach, read, settings, apply };
})();
