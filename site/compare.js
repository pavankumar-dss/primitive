'use strict';

// Before and after: the original image laid over the result, cut off at a
// divider that can be dragged, or moved with the arrow keys once focused.
// app.js calls compareUI.setImage() when an image loads and
// compareUI.setAvailable() as results come and go.
const compareUI = (() => {
  const $ = (id) => document.getElementById(id);
  const el = { button: $('compare'), view: $('compare-view'), original: $('compare-original'), handle: $('compare-handle') };
  const MAX_SIDE = 2048; // a big photo needn't take a big canvas: the frame is smaller
  const STEP = 2;        // percent per arrow key press
  const BIG_STEP = 10;   // per Page Up or Page Down

  let image = null;    // ImageBitmap of the original
  let drawn = false;   // whether the overlay shows it yet
  let split = 50;      // the divider's position, in percent from the left
  let dragging = false;

  function setImage(bitmap) {
    image = bitmap;
    drawn = false;
    show(false);
  }

  function setAvailable(available) {
    el.button.disabled = !available || !image;
    if (el.button.disabled) show(false);
  }

  function show(on) {
    el.button.setAttribute('aria-pressed', String(on));
    el.view.hidden = !on;
    if (on && !drawn) draw();
  }

  // The canvas fills the result's frame, which has the image's shape, so
  // the two line up. It goes under the "Original" label, which the divider
  // cuts off along with the image.
  function draw() {
    const scale = Math.min(1, MAX_SIDE / Math.max(image.width, image.height));
    const canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round(image.width * scale));
    canvas.height = Math.max(1, Math.round(image.height * scale));
    const ctx = canvas.getContext('2d');
    ctx.imageSmoothingQuality = 'high';
    ctx.drawImage(image, 0, 0, canvas.width, canvas.height);
    el.original.querySelector('canvas')?.remove();
    el.original.prepend(canvas);
    drawn = true;
  }

  function move(to) {
    split = Math.min(100, Math.max(0, to));
    el.view.style.setProperty('--split', split + '%');
    el.handle.setAttribute('aria-valuenow', Math.round(split));
    el.handle.setAttribute('aria-valuetext', `${Math.round(split)}% original`);
  }

  function moveTo(e) {
    const r = el.view.getBoundingClientRect();
    move(100 * (e.clientX - r.left) / r.width);
  }

  el.button.addEventListener('click', () => show(el.view.hidden));

  // Press anywhere on the picture to bring the divider there, and drag.
  // Vertical swipes still scroll the page on touch screens (touch-action
  // in style.css), which cancels the drag.
  el.view.addEventListener('pointerdown', (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    el.view.setPointerCapture(e.pointerId);
    dragging = true;
    moveTo(e);
    el.handle.focus({ preventScroll: true });
  });
  el.view.addEventListener('pointermove', (e) => {
    if (dragging) moveTo(e);
  });
  for (const type of ['pointerup', 'pointercancel']) {
    el.view.addEventListener(type, () => { dragging = false; });
  }

  el.handle.addEventListener('keydown', (e) => {
    const steps = { ArrowLeft: -STEP, ArrowDown: -STEP, ArrowRight: STEP, ArrowUp: STEP, PageDown: -BIG_STEP, PageUp: BIG_STEP };
    if (e.key in steps) move(split + steps[e.key]);
    else if (e.key === 'Home') move(0);
    else if (e.key === 'End') move(100);
    else return;
    e.preventDefault();
  });

  move(split);
  return { setImage, setAvailable };
})();
