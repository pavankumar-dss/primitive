'use strict';

// A dropdown for the shape <select> that shows a small preview of each
// shape. The native <select id="mode"> stays in the page, visually hidden,
// as the source of truth: choosing here updates it and fires its "change"
// event, and changes made to it elsewhere are reflected here.
(() => {
  const select = document.getElementById('mode');
  const button = document.getElementById('mode-button');
  const list = document.getElementById('mode-list');

  const A = '#f2b134', R = '#ed553b', B = '#2b78bd', T = '#3caea3';
  const ICONS = {
    1: `<path d="M3 20 12 4l9 16z" fill="${A}"/>`,
    2: `<rect x="3.5" y="6.5" width="17" height="11" fill="${B}"/>`,
    5: `<rect x="3.5" y="8" width="17" height="8" fill="${B}" transform="rotate(-30 12 12)"/>`,
    3: `<ellipse cx="12" cy="12" rx="9.5" ry="5.5" fill="${T}"/>`,
    7: `<ellipse cx="12" cy="12" rx="9.5" ry="4.5" fill="${T}" transform="rotate(-35 12 12)"/>`,
    4: `<circle cx="12" cy="12" r="8.5" fill="${R}"/>`,
    8: `<path d="M4 8.5 14 3l7 11-11.5 7z" fill="${A}"/>`,
    6: `<path d="M3 19C7 3 15 3 21 13" fill="none" stroke="${R}" stroke-width="2.5" stroke-linecap="round"/>`,
    0: `<path d="M2 21 9 7l7 14z" fill="${A}" fill-opacity=".85"/><circle cx="16" cy="9" r="6" fill="${R}" fill-opacity=".7"/>`
      + `<rect x="10" y="14" width="11" height="7" fill="${B}" fill-opacity=".75"/>`,
  };
  const icon = (value) => `<svg class="shape-icon" viewBox="0 0 24 24" aria-hidden="true">${ICONS[value] || ''}</svg>`;

  const options = [...select.options].map((o, i) => {
    const li = document.createElement('li');
    li.id = 'mode-option-' + i;
    li.setAttribute('role', 'option');
    li.dataset.value = o.value;
    li.innerHTML = icon(o.value) + '<span></span>';
    li.querySelector('span').textContent = o.textContent;
    list.append(li);
    return li;
  });
  let active = 0;

  function render() {
    const i = select.selectedIndex;
    button.querySelector('.picker-value').innerHTML = icon(select.value) + '<span></span>';
    button.querySelector('.picker-value span').textContent = select.options[i].textContent;
    options.forEach((li, j) => li.setAttribute('aria-selected', String(j === i)));
  }

  function setActive(i) {
    active = (i + options.length) % options.length;
    options.forEach((li, j) => li.classList.toggle('active', j === active));
    list.setAttribute('aria-activedescendant', options[active].id);
    options[active].scrollIntoView({ block: 'nearest' });
  }

  function open() {
    list.hidden = false;
    button.setAttribute('aria-expanded', 'true');
    setActive(select.selectedIndex);
    list.focus();
  }

  function close(refocus = true) {
    if (list.hidden) return;
    list.hidden = true;
    button.setAttribute('aria-expanded', 'false');
    if (refocus) button.focus();
  }

  function choose(i) {
    if (select.selectedIndex !== i) {
      select.selectedIndex = i;
      select.dispatchEvent(new Event('change', { bubbles: true }));
    }
    close();
  }

  button.addEventListener('click', () => (list.hidden ? open() : close()));
  button.addEventListener('keydown', (e) => {
    if (['ArrowDown', 'ArrowUp', 'Enter', ' '].includes(e.key)) {
      e.preventDefault();
      open();
    }
  });
  list.addEventListener('keydown', (e) => {
    switch (e.key) {
      case 'ArrowDown': setActive(active + 1); break;
      case 'ArrowUp': setActive(active - 1); break;
      case 'Home': setActive(0); break;
      case 'End': setActive(options.length - 1); break;
      case 'Enter': case ' ': choose(active); break;
      case 'Escape': close(); break;
      case 'Tab': close(false); return;
      default: {
        // type-ahead: jump to the next option starting with that letter
        if (e.key.length !== 1) return;
        const k = e.key.toLowerCase();
        for (let n = 1; n <= options.length; n++) {
          const j = (active + n) % options.length;
          if (options[j].textContent.trim().toLowerCase().startsWith(k)) { setActive(j); break; }
        }
      }
    }
    e.preventDefault();
  });
  list.addEventListener('click', (e) => {
    const li = e.target.closest('[role=option]');
    if (li) choose(options.indexOf(li));
  });
  list.addEventListener('mousemove', (e) => {
    const li = e.target.closest('[role=option]');
    if (li && options.indexOf(li) !== active) setActive(options.indexOf(li));
  });
  document.addEventListener('pointerdown', (e) => {
    if (!list.hidden && !e.target.closest('.shape-picker')) close(false);
  });
  select.addEventListener('change', render);
  render();
})();
