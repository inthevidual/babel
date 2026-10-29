// A small context menu. Items are registered once with a condition; opening
// the menu shows the items whose condition holds for the click. New actions
// are just new items.
//
//   menu.add({ id, group, label, hint, when(ctx), enabled(ctx), run(ctx) })
//
// label may be a function of ctx. Items are shown in registration order,
// with a separator between groups.

export class ContextMenu {
  constructor(el) {
    this.el = el;
    this.items = [];
    this.ctx = null;
    el.setAttribute('role', 'menu');
    el.addEventListener('mousedown', e => e.preventDefault()); // keep focus and selection
    el.addEventListener('click', e => {
      const b = e.target.closest('.menu-item');
      if (b && !b.disabled) this.choose(+b.dataset.i);
    });
    el.addEventListener('keydown', e => this.key(e));
    addEventListener('mousedown', e => { if (!el.hidden && !el.contains(e.target)) this.close(); }, true);
    addEventListener('blur', () => this.close());
    addEventListener('resize', () => this.close());
    // Close when the user scrolls, not on programmatic scrolls (the panels
    // keep each other in sync a frame later).
    addEventListener('wheel', e => { if (!el.contains(e.target)) this.close(); }, { capture: true, passive: true });
    addEventListener('touchmove', () => this.close(), { capture: true, passive: true });
  }

  add(item) { this.items.push(item); return this; }

  open(ctx, x, y) {
    const shown = this.items.filter(it => !it.when || it.when(ctx));
    if (!shown.length) return false;
    this.ctx = ctx;
    this.shown = shown;
    this.restore = document.activeElement;
    let html = '';
    let group = shown[0].group;
    shown.forEach((it, i) => {
      if (it.group !== group) { html += '<div class="menu-sep" role="separator"></div>'; group = it.group; }
      const label = typeof it.label === 'function' ? it.label(ctx) : it.label;
      const sub = typeof it.sub === 'function' ? it.sub(ctx) : it.sub;
      const on = !it.enabled || it.enabled(ctx);
      html += `<button type="button" class="menu-item" role="menuitem" data-i="${i}" ${on ? '' : 'disabled'}>
        <span class="mi-label">${esc(label)}${sub ? `<small>${esc(sub)}</small>` : ''}</span>
        ${it.hint ? `<kbd class="mi-hint">${esc(it.hint)}</kbd>` : ''}</button>`;
    });
    this.el.innerHTML = html;
    this.el.hidden = false;
    const w = this.el.offsetWidth, h = this.el.offsetHeight;
    this.el.style.left = `${Math.max(4, Math.min(innerWidth - w - 4, x))}px`;
    this.el.style.top = `${Math.max(4, y + h > innerHeight - 4 ? y - h : y)}px`;
    this.el.querySelector('.menu-item:not(:disabled)')?.focus({ preventScroll: true });
    return true;
  }

  close() {
    if (this.el.hidden) return;
    this.el.hidden = true;
    this.ctx = null;
  }

  async choose(i) {
    const it = this.shown?.[i];
    const ctx = this.ctx;
    this.close();
    if (!it || !ctx) return;
    // Give focus back so editing commands act where the menu was opened.
    if (this.restore?.isConnected) this.restore.focus({ preventScroll: true });
    if (ctx.range) { const s = getSelection(); s.removeAllRanges(); s.addRange(ctx.range); }
    try { await it.run(ctx); } catch (e) { console.error(e); ctx.fail?.(e); }
  }

  key(e) {
    const items = [...this.el.querySelectorAll('.menu-item:not(:disabled)')];
    const i = items.indexOf(document.activeElement);
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const n = items.length;
      items[(i + (e.key === 'ArrowDown' ? 1 : n - 1)) % n]?.focus();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      this.close();
      if (this.restore?.isConnected) this.restore.focus({ preventScroll: true });
    } else if (e.key === 'Tab') {
      e.preventDefault();
    }
  }
}

const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
