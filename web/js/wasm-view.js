// The editor in WebAssembly (crates/babel-wasm over babel-editor, the same
// editor as the desktop app's): both documents laid out by Babel's own
// engine, drawn on canvases, with the caret, selections, proofing
// underlines and done/edited marks as overlays. A hidden textarea takes the
// keyboard, input methods and the clipboard.
//
// main.js keeps everything around it (projects, storage, the journal,
// versions, download): this view writes each edited paragraph to the
// journal as it changes and hands changed parts back through onParts.

import init, { WasmEditor, addFont } from '../wasm/babel_wasm.js';
import { ContextMenu } from './menu.js';

const QT = {
  ArrowLeft: 0x01000012, ArrowRight: 0x01000014, ArrowUp: 0x01000013, ArrowDown: 0x01000015,
  Home: 0x01000010, End: 0x01000011, Backspace: 0x01000003, Delete: 0x01000007, Enter: 0x01000004,
};
const SHIFT = 0x02000000, CTRL = 0x04000000;
const PX = 96 / 72; // CSS pixels per point at 100 %

let engine = null;

// Loads the WebAssembly module and gives it the fonts Babel serves
// (metric-compatible stand-ins for Word's: the browser has no system fonts
// to offer it).
export function loadEngine() {
  engine ??= (async () => {
    await init();
    const css = await (await fetch('fonts/fonts.css')).text();
    const urls = [...new Set([...css.matchAll(/url\(["']?([^"')]+\.woff2)["']?\)/g)].map(m => new URL(m[1], new URL('fonts/', location.href)).href))];
    await Promise.all(urls.map(async u => {
      const res = await fetch(u);
      if (!res.ok) return console.warn('font missing', u);
      try { addFont(new Uint8Array(await res.arrayBuffer())); } catch (e) { console.warn('font not loaded', u, e); }
    }));
  })();
  return engine;
}

const el = (tag, cls, parent) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  parent?.appendChild(e);
  return e;
};

export class WasmView {
  // original: ArrayBuffer; sourceParts, translated: {path: xml}; project: metadata.
  // hooks: { journal(entries), parts(parts, meta), toast(msg, kind), readOnly(), settings, langCode }
  static async open({ original, sourceParts, translated, project, srcRoot, tgtRoot, srcScroll, tgtScroll, hooks }) {
    await loadEngine();
    const editor = new WasmEditor(new Uint8Array(original), JSON.stringify(sourceParts), JSON.stringify(translated), JSON.stringify(project));
    const v = new WasmView(editor, { srcRoot, tgtRoot, srcScroll, tgtScroll, project, hooks });
    v.build();
    return v;
  }

  constructor(editor, { srcRoot, tgtRoot, srcScroll, tgtScroll, project, hooks }) {
    Object.assign(this, { editor, srcRoot, tgtRoot, srcScroll, tgtScroll, project, hooks });
    this.zoom = 1;
    this.state = {};
    this.issues = { underlines: [], count: 0, status: '' };
    this.marks = [];
    this.stale = new Set();
    this.proofBusy = false;
    this.backoff = 0;
    this.closed = false;
  }

  // ── Building and drawing ───────────────────────────────────────────────

  build() {
    const s = this.hooks.settings;
    this.editor.setProofing(!!s.picky, JSON.stringify([...(this.hooks.dictionary ?? [])]));
    this.srcRoot.innerHTML = this.tgtRoot.innerHTML = '';
    this.srcRoot.classList.add('w-doc');
    this.tgtRoot.classList.add('w-doc');
    const sizes = JSON.parse(this.editor.pageSizes());
    this.src = sizes.source.map((sz, i) => this.makePage(this.srcRoot, sz, i, 'src'));
    this.tgt = sizes.target.map((sz, i) => this.makePage(this.tgtRoot, sz, i, 'tgt'));
    this.editor.takeRedraw();
    this.input = el('textarea', 'w-input', document.body);
    this.input.setAttribute('aria-label', 'Translation');
    this.input.spellcheck = false;
    this.preedit = el('span', 'w-preedit', document.body);
    this.preedit.hidden = true;
    this.observe();
    this.wireInput();
    this.wireMouse();
    this.menu = new ContextMenu(el('div', 'menu ctx', document.body));
    this.menu.el.hidden = true;
    this.registerMenu();
    this.applyState(this.editor.state());
    this.proofTimer = setInterval(() => this.proof(), 500);
  }

  makePage(root, sz, i, side) {
    const wrap = el('section', 'w-page', root);
    wrap.dataset.i = i;
    const canvas = el('canvas', 'w-canvas', wrap);
    const over = el('div', 'w-over', wrap);
    const page = { wrap, canvas, over, w: sz.w, h: sz.h, i, side, drawn: 0 };
    this.size(page);
    return page;
  }

  size(p) {
    p.wrap.style.width = `${p.w * PX * this.zoom}px`;
    p.wrap.style.height = `${p.h * PX * this.zoom}px`;
  }

  // Pages are drawn when they come into view, and again when they change.
  observe() {
    this.io = new IntersectionObserver(entries => {
      for (const e of entries) {
        const p = e.target.__page;
        p.visible = e.isIntersecting;
        if (p.visible && (p.drawn !== this.zoom || this.stale.has(p))) this.draw(p);
      }
    }, { rootMargin: '600px 0px' });
    for (const p of [...this.src, ...this.tgt]) {
      p.wrap.__page = p;
      this.io.observe(p.wrap);
    }
  }

  draw(p) {
    const scale = PX * this.zoom * (devicePixelRatio || 1);
    const buf = p.side === 'src' ? this.editor.renderSource(p.i, scale) : this.editor.render(p.i, scale);
    const dv = new DataView(buf.buffer, buf.byteOffset, 8);
    const w = dv.getUint32(0, true), h = dv.getUint32(4, true);
    p.canvas.width = w;
    p.canvas.height = h;
    p.canvas.style.width = `${p.w * PX * this.zoom}px`;
    p.canvas.style.height = `${p.h * PX * this.zoom}px`;
    p.canvas.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(buf.buffer, buf.byteOffset + 8, w * h * 4), w, h), 0, 0);
    p.drawn = this.zoom;
    this.stale.delete(p);
  }

  // Draws (or marks for drawing) the translation's pages the editor says changed.
  redraw() {
    const r = JSON.parse(this.editor.takeRedraw());
    if (r.all) {
      const sizes = JSON.parse(this.editor.pageSizes()).target;
      for (const p of this.tgt) { this.io.unobserve(p.wrap); p.wrap.remove(); }
      this.tgt = sizes.map((sz, i) => this.makePage(this.tgtRoot, sz, i, 'tgt'));
      for (const p of this.tgt) { p.wrap.__page = p; this.io.observe(p.wrap); }
      return true;
    }
    for (const i of r.pages) {
      const p = this.tgt[i];
      if (!p) continue;
      if (p.visible) this.draw(p); else this.stale.add(p);
    }
    return r.pages.length > 0;
  }

  setZoom(z) {
    this.zoom = z;
    for (const p of [...this.src, ...this.tgt]) {
      this.size(p);
      p.canvas.style.width = p.wrap.style.width;
      p.canvas.style.height = p.wrap.style.height;
      if (p.visible) this.draw(p);
    }
    this.paint();
  }

  pageWidthPx() {
    return (this.tgt[0]?.w ?? 595) * PX;
  }

  // ── State and overlays ─────────────────────────────────────────────────

  // Takes an answer from the editor: caret, selections, pages, marks, saving.
  answer(json, { fromClick = false } = {}) {
    if (!json || json === 'null') return;
    const r = JSON.parse(json);
    const changed = this.redraw();
    if (r.error) this.hooks.toast(r.error, 'warn');
    if (r.message) this.hooks.toast(r.message);
    this.applyState(r);
    if (changed) {
      this.issues = JSON.parse(this.editor.issues());
      this.marks = JSON.parse(this.editor.marks());
      this.paint();
    } else if (this.editor.metaDirty()) {
      this.marks = JSON.parse(this.editor.marks());
      this.paint();
    }
    this.flushJournal();
    if (fromClick && r.issue) this.showIssue(r.issue); else this.hideIssue();
  }

  applyState(json) {
    const r = typeof json === 'string' ? JSON.parse(json) : json;
    this.state = r;
    if (!this.marks.length) this.marks = JSON.parse(this.editor.marks());
    this.paint();
    this.hooks.status?.(r.stats, r.paragraph, this.issues, this);
  }

  // Each edited paragraph to the journal at once; the parts to main.js shortly.
  flushJournal() {
    const j = JSON.parse(this.editor.takeJournal());
    const keys = Object.keys(j);
    if (keys.length) this.hooks.journal(j);
    if (keys.length || this.editor.metaDirty()) {
      clearTimeout(this.partsTimer);
      this.partsTimer = setTimeout(() => this.handParts(), 400);
    }
  }

  // Changed parts and metadata go back to main.js, which saves them.
  handParts() {
    const parts = JSON.parse(this.editor.takeDirty());
    const meta = JSON.parse(this.editor.projectMeta(JSON.stringify(this.project)));
    Object.assign(this.project, { confirmed: meta.confirmed, ignored: meta.ignored, stats: meta.stats });
    this.hooks.parts(parts, meta);
  }

  // Parts still to hand over (before a save, closing or downloading).
  flush() {
    clearTimeout(this.partsTimer);
    this.flushJournal();
    clearTimeout(this.partsTimer);
    this.handParts();
  }

  paint() {
    const st = this.state;
    const z = PX * this.zoom;
    const by = (list, side) => {
      const pages = side === 'src' ? this.src : this.tgt;
      const out = new Map(pages.map(p => [p, []]));
      for (const r of list ?? []) { const p = pages[r.page]; if (p) out.get(p).push(r); }
      return out;
    };
    const rect = (r, cls, extra = '') => `<i class="${cls}" style="left:${r.x * z}px;top:${r.y * z}px;width:${Math.max(2, r.w * z)}px;height:${(r.h ?? 0) * z}px;${extra}"></i>`;
    const sel = by(st.selection, 'tgt'), srcSel = by(st.sourceSelection, 'src');
    const und = by(this.issues.underlines, 'tgt'), marks = by(this.marks, 'tgt');
    for (const p of this.tgt) {
      let html = '';
      for (const m of marks.get(p)) html += `<i class="w-mark ${m.kind}" style="left:${(m.x - 9) * z}px;top:${m.y * z}px;height:${m.h * z}px"></i>`;
      for (const r of sel.get(p)) html += rect(r, 'w-sel');
      for (const u of und.get(p)) html += `<i class="w-und ${u.kind}" style="left:${u.x * z}px;top:${u.y * z}px;width:${u.w * z}px"></i>`;
      const c = st.caret;
      if (c && c.page === p.i && !(st.selection ?? []).length) {
        html += `<i class="w-caret${c.editable ? '' : ' fixed'}" style="left:${c.x * z - 1}px;top:${c.top * z}px;height:${c.height * z}px"></i>`;
      }
      p.over.innerHTML = html;
    }
    for (const p of this.src) {
      let html = '';
      for (const r of srcSel.get(p)) html += rect(r, 'w-sel');
      p.over.innerHTML = html;
    }
    this.placeInput();
  }

  // The textarea follows the caret, so input methods open their windows there.
  placeInput() {
    const c = this.state.caret;
    const p = c && this.tgt[c.page];
    if (!p) return;
    const box = p.wrap.getBoundingClientRect();
    const z = PX * this.zoom;
    const x = box.left + c.x * z, y = box.top + c.top * z;
    Object.assign(this.input.style, { left: `${x}px`, top: `${y}px`, height: `${c.height * z}px`, fontSize: `${Math.max(8, c.height * z * 0.8)}px` });
    Object.assign(this.preedit.style, { left: `${x}px`, top: `${y}px`, fontSize: this.input.style.fontSize });
  }

  // Scrolls the translation so the caret is in view.
  showCaret() {
    const c = this.state.caret;
    const p = c && this.tgt[c.page];
    if (!p) return;
    const z = PX * this.zoom;
    const sc = this.tgtScroll;
    const top = p.wrap.offsetTop + c.top * z, bottom = top + c.height * z;
    if (top < sc.scrollTop + 20) sc.scrollTop = top - 40;
    else if (bottom > sc.scrollTop + sc.clientHeight - 20) sc.scrollTop = bottom - sc.clientHeight + 40;
  }

  // ── Input ──────────────────────────────────────────────────────────────

  focus() {
    this.input.focus({ preventScroll: true });
  }

  key(code, text, mods) {
    if (this.hooks.readOnly()) return false;
    const r = this.editor.key(code, text, mods);
    if (r === 'null') return false;
    this.answer(r);
    this.showCaret();
    return true;
  }

  type(text) {
    if (this.hooks.readOnly() || !text) return;
    this.answer(this.editor.text(text));
    this.showCaret();
  }

  wireInput() {
    const t = this.input;
    let composing = false;
    t.addEventListener('keydown', e => {
      if (e.isComposing || composing) return;
      const ctrl = e.ctrlKey || e.metaKey;
      const mods = (e.shiftKey ? SHIFT : 0) | (ctrl ? CTRL : 0);
      if (ctrl && e.key.toLowerCase() === 'k') { e.preventDefault(); this.pasteLink(); return; }
      if (ctrl && ['c', 'x', 'v'].includes(e.key.toLowerCase())) return; // the clipboard events below
      if (e.key === 'Tab' && !ctrl) { e.preventDefault(); this.type('\t'); return; }
      if (e.key === 'Escape') { this.hideIssue(); return; }
      let code = QT[e.key];
      if (!code && ctrl && e.key.length === 1) code = e.key.toUpperCase().charCodeAt(0);
      if (!code) return; // text: the input event
      if (this.key(code, '', mods)) e.preventDefault();
    });
    t.addEventListener('compositionstart', () => { composing = true; });
    t.addEventListener('compositionupdate', e => {
      this.preedit.textContent = e.data;
      this.preedit.hidden = !e.data;
    });
    t.addEventListener('compositionend', e => {
      composing = false;
      this.preedit.hidden = true;
      t.value = '';
      this.type(e.data);
    });
    t.addEventListener('input', e => {
      if (composing || e.isComposing) return;
      const v = t.value;
      t.value = '';
      this.type(v);
    });
    t.addEventListener('copy', e => {
      const text = this.editor.selectedText() || this.editor.sourceSelectedText();
      if (!text) return;
      e.clipboardData.setData('text/plain', text);
      e.preventDefault();
    });
    t.addEventListener('cut', e => {
      const text = this.editor.selectedText();
      if (!text) return;
      e.clipboardData.setData('text/plain', text);
      e.preventDefault();
      this.key(QT.Delete, '', 0);
    });
    t.addEventListener('paste', e => {
      e.preventDefault();
      this.type(e.clipboardData.getData('text/plain'));
    });
    t.addEventListener('blur', () => { this.tgtRoot.classList.add('w-blur'); });
    t.addEventListener('focus', () => { this.tgtRoot.classList.remove('w-blur'); });
  }

  async pasteLink() {
    let clip = '';
    try { clip = await navigator.clipboard.readText(); } catch { /* the copied link, if any */ }
    this.answer(this.editor.pasteLink(clip));
  }

  // Page coordinates (points) of a mouse event on a page.
  at(e, p) {
    const box = p.wrap.getBoundingClientRect();
    const z = PX * this.zoom;
    return [p.i, (e.clientX - box.left) / z, (e.clientY - box.top) / z];
  }

  wireMouse() {
    const pageOf = (e, list) => list.find(p => p.wrap.contains(e.target));
    let drag = null;
    for (const [root, side] of [[this.tgtRoot, 'tgt'], [this.srcRoot, 'src']]) {
      root.addEventListener('pointerdown', e => {
        if (e.button !== 0) return;
        const p = pageOf(e, side === 'tgt' ? this.tgt : this.src);
        if (!p) return;
        e.preventDefault();
        this.focus();
        this.menu.close();
        const [i, x, y] = this.at(e, p);
        if (side === 'tgt') this.answer(this.editor.click(i, x, y, e.shiftKey), { fromClick: !e.shiftKey });
        else this.answer(this.editor.sourceClick(i, x, y, e.shiftKey));
        drag = { side };
        root.setPointerCapture?.(e.pointerId);
      });
      root.addEventListener('pointermove', e => {
        if (!drag || drag.side !== side || !(e.buttons & 1)) return;
        const list = side === 'tgt' ? this.tgt : this.src;
        const p = list.find(q => { const b = q.wrap.getBoundingClientRect(); return e.clientY >= b.top && e.clientY <= b.bottom; }) ?? pageOf(e, list);
        if (!p) return;
        const [i, x, y] = this.at(e, p);
        this.answer(side === 'tgt' ? this.editor.click(i, x, y, true) : this.editor.sourceClick(i, x, y, true));
      });
      root.addEventListener('pointerup', () => { drag = null; });
      root.addEventListener('dblclick', e => {
        const p = pageOf(e, side === 'tgt' ? this.tgt : this.src);
        if (!p) return;
        const [i, x, y] = this.at(e, p);
        this.answer(side === 'tgt' ? this.editor.word(i, x, y) : this.editor.sourceWord(i, x, y));
      });
      root.addEventListener('contextmenu', e => {
        if (e.shiftKey) return; // the browser's own menu
        const p = pageOf(e, side === 'tgt' ? this.tgt : this.src);
        if (!p) return;
        e.preventDefault();
        const [i, x, y] = this.at(e, p);
        let ctx;
        if (side === 'tgt') {
          if (!(this.state.selection ?? []).length) this.answer(this.editor.click(i, x, y, false));
          ctx = { panel: 'tgt' };
        } else {
          ctx = { panel: 'src', ...JSON.parse(this.editor.sourceMenu(i, x, y)) };
        }
        this.focus();
        this.menu.open(ctx, e.clientX, e.clientY);
      });
    }
    // The two panels scroll together when Sync is on.
    let syncing = false;
    const follow = (a, b) => a.addEventListener('scroll', () => {
      this.placeInput();
      if (syncing || !this.hooks.settings.sync) return;
      syncing = true;
      b.scrollTop = a.scrollTop;
      requestAnimationFrame(() => { syncing = false; });
    });
    follow(this.tgtScroll, this.srcScroll);
    follow(this.srcScroll, this.tgtScroll);
  }

  registerMenu() {
    const st = () => this.state;
    const sel = () => (st().selection ?? []).length > 0;
    const para = () => st().paragraph;
    const write = t => navigator.clipboard.writeText(t).catch(() => {});
    const m = this.menu;
    const tgt = ctx => ctx.panel === 'tgt', src = ctx => ctx.panel === 'src';
    m.add({ group: 'clip', label: 'Cut', hint: 'Ctrl+X', when: tgt, enabled: sel, run: () => { write(this.editor.selectedText()); this.key(QT.Delete, '', 0); } })
      .add({ group: 'clip', label: 'Copy', hint: 'Ctrl+C', when: tgt, enabled: sel, run: () => write(this.editor.selectedText()) })
      .add({ group: 'clip', label: 'Paste as plain text', hint: 'Ctrl+V', when: tgt, run: async () => { try { this.type(await navigator.clipboard.readText()); } catch { this.hooks.toast('Use Ctrl+V to paste.', 'warn'); } } })
      .add({ group: 'clip', label: 'Paste link', hint: 'Ctrl+K', when: tgt, enabled: sel, run: () => this.pasteLink() })
      .add({ group: 'para', label: () => (para()?.done ? 'Unmark as done' : 'Mark as done'), hint: 'Ctrl+Enter', when: tgt, enabled: () => !!para(), run: () => this.key(QT.Enter, '', CTRL) })
      .add({ group: 'para', label: 'Revert paragraph to the original', when: tgt, enabled: () => !!para()?.edited, run: () => this.answer(this.editor.revert()) })
      .add({ group: 'para', label: 'Copy the original text', when: tgt, enabled: () => !!para(), run: () => write(this.editor.originalText() ?? '') })
      .add({ group: 'src', label: 'Copy', hint: 'Ctrl+C', when: ctx => src(ctx) && !!ctx.text, run: ctx => write(ctx.text) })
      .add({ group: 'src', label: ctx => `Look up “${ctx.lookup}” in Merriam-Webster`, when: ctx => src(ctx) && !!ctx.lookup, run: ctx => open(`https://www.merriam-webster.com/dictionary/${encodeURIComponent(ctx.lookup)}`, '_blank', 'noopener') })
      .add({ group: 'src', label: 'Copy link', sub: ctx => ctx.link, when: ctx => src(ctx) && !!ctx.link, run: () => { const c = this.editor.copySourceLink(); if (c) { write(c); this.hooks.toast(`Link copied: ${c}`); } } })
      .add({ group: 'src', label: 'Copy paragraph text', when: ctx => src(ctx) && !!ctx.paragraph, run: ctx => write(ctx.paragraph) });
  }

  // ── Proofing ───────────────────────────────────────────────────────────

  async proof() {
    const s = this.hooks.settings;
    if (this.closed || this.proofBusy || !s.proof || Date.now() < (this.nextProof ?? 0)) return;
    const req = this.editor.proofRequest();
    if (req === 'null') return;
    this.proofBusy = true;
    const form = new URLSearchParams(JSON.parse(req));
    if (s.user && s.key) { form.set('username', s.user); form.set('apiKey', s.key); }
    try {
      const res = await fetch((s.endpoint || 'https://api.languagetool.org').replace(/\/+$/, '') + '/v2/check', {
        method: 'POST', body: form, headers: { Accept: 'application/json' },
      });
      if (res.status === 429 || res.status === 503) throw Object.assign(new Error('LanguageTool rate limit — retrying shortly'), { wait: 4000 });
      if (!res.ok) throw Object.assign(new Error(`LanguageTool answered ${res.status}`), { wait: 10000 });
      this.editor.proofResult(await res.text());
      this.backoff = 0;
    } catch (e) {
      this.editor.proofFailed(e.message || 'LanguageTool is unreachable');
      this.backoff = Math.min(120000, Math.max(e.wait ?? 10000, this.backoff * 2));
    } finally {
      this.proofBusy = false;
      this.nextProof = Date.now() + 3200 + this.backoff;
    }
    if (this.closed) return;
    this.issues = JSON.parse(this.editor.issues());
    this.paint();
    this.hooks.status?.(this.state.stats, this.state.paragraph, this.issues, this);
  }

  showIssue(i) {
    const pop = document.getElementById('issuePop');
    if (!pop) return;
    const c = this.state.caret;
    const p = c && this.tgt[c.page];
    if (!p) return;
    const box = p.wrap.getBoundingClientRect();
    const z = PX * this.zoom;
    const esc = s => String(s).replace(/[&<>"]/g, ch => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[ch]));
    pop.innerHTML = `<b>${esc(i.short || (i.kind === 'spelling' ? 'Spelling' : 'Grammar'))}</b><p>${esc(i.message)}</p>
      <div class="pop-sugg">${(i.replacements ?? []).map((r, n) => `<button type="button" class="ghost small" data-r="${n}">${esc(r || '(remove)')}</button>`).join('')}</div>
      <div class="pop-acts"><button type="button" class="ghost small" data-a="ignore">Ignore</button>${i.kind === 'spelling' ? '<button type="button" class="ghost small" data-a="dict">Add to dictionary</button>' : ''}</div>`;
    pop.hidden = false;
    pop.style.left = `${Math.max(8, Math.min(innerWidth - pop.offsetWidth - 8, box.left + c.x * z - 20))}px`;
    pop.style.top = `${box.top + (c.top + c.height) * z + 6}px`;
    pop.onmousedown = e => e.preventDefault();
    pop.onclick = e => {
      const b = e.target.closest('button');
      if (!b) return;
      this.hideIssue();
      if (b.dataset.r != null) this.answer(this.editor.applySuggestion(i.pid, i.n, i.replacements[+b.dataset.r]));
      else if (b.dataset.a === 'ignore') { this.editor.ignore(i.pid, i.n); this.flushJournal(); }
      else if (b.dataset.a === 'dict') { const w = this.editor.addToDictionary(i.pid, i.n); if (w) this.hooks.addWord?.(w); }
      this.issues = JSON.parse(this.editor.issues());
      this.paint();
      this.focus();
    };
  }

  hideIssue() {
    const pop = document.getElementById('issuePop');
    if (pop) pop.hidden = true;
  }

  setProofing(settings, dictionary) {
    this.editor.setProofing(!!settings.picky, JSON.stringify([...dictionary]));
  }

  setLanguage(code) {
    this.editor.setLanguage(code);
  }

  close() {
    if (this.closed) return;
    this.flush();
    this.closed = true;
    clearInterval(this.proofTimer);
    this.io?.disconnect();
    this.menu?.close();
    this.menu?.el.remove();
    this.input?.remove();
    this.preedit?.remove();
    this.hideIssue();
    this.srcRoot.classList.remove('w-doc');
    this.tgtRoot.classList.remove('w-doc');
    this.editor.free();
  }
}
