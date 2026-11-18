// The text view in the browser: the document as rows of segments, the
// original beside the translation (as in a CAT tool), with Claude's
// translation of the row being worked on in a panel. The desktop app's
// SegmentsView.qml is the same view. It edits the same translation as the
// pages, through the same editor (crates/babel-editor, rows.rs): typing,
// formatting, undo, done marks and the right-click menu are the page view's.
//
// A row's translation is an editable cell; what is typed is handed to the
// editor as "these characters became those" (positions in UTF-16, a chip
// counting one), and the cell is drawn again from the editor whenever it
// would differ from what was typed (formatting, undo, an edit refused).

import { QT, SHIFT, CTRL } from './wasm-view.js';

const esc = t => t.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

// Spans as HTML: bold, italic, underline, links; chips can't be typed into.
function html(spans) {
  let h = '';
  for (const s of spans ?? []) {
    if (s.chip !== undefined) {
      h += s.chip
        ? `<span class="chip" contenteditable="false" data-n="${s.chip}" title="${esc(s.title || '')}">${esc(s.t)}</span>`
        : `<span class="chip0" contenteditable="false">${s.t}</span>`;
      continue;
    }
    let t = esc(s.t).replace(/\n/g, '<br>');
    if (s.b) t = `<b>${t}</b>`;
    if (s.i) t = `<i>${t}</i>`;
    if (s.u) t = `<u>${t}</u>`;
    if (s.l) t = `<span class="lnk">${t}</span>`;
    h += t;
  }
  // A line break at the very end shows only with another after it.
  const all = (spans ?? []).map(s => s.t).join('');
  if (all.endsWith('\n')) h += '<br data-end>';
  return h;
}

const plainOf = spans => (spans ?? []).map(s => s.t).join('');
const textOf = spans => (spans ?? []).filter(s => s.chip === undefined).map(s => s.t).join('');

// What a cell holds, in order: text nodes, line breaks and chips with their length.
function units(cell) {
  const out = [];
  const visit = n => {
    for (const c of n.childNodes) {
      if (c.nodeType === Node.TEXT_NODE) out.push([c, c.data.length]);
      else if (c.nodeName === 'BR') { if (!c.hasAttribute('data-end')) out.push([c, 1]); }
      else if (c.classList?.contains('chip') || c.classList?.contains('chip0')) out.push([c, c.textContent.length]);
      else if (c.nodeType === Node.ELEMENT_NODE) visit(c);
    }
  };
  visit(cell);
  return out;
}

function domText(cell) {
  return units(cell).map(([n]) => (n.nodeType === Node.TEXT_NODE ? n.data : n.nodeName === 'BR' ? '\n' : n.textContent)).join('').replace(/\u00a0/g, ' ');
}

// A DOM position (as a selection gives it) as a position in the cell's text.
function offsetOf(cell, node, off) {
  if (!cell.contains(node)) return 0;
  const r = document.createRange();
  r.setStart(cell, 0);
  r.setEnd(node, off);
  const d = document.createElement('div');
  d.appendChild(r.cloneContents());
  return domText(d).length;
}

// A position in the cell's text as a DOM position.
function placeOf(cell, off) {
  let acc = 0;
  for (const [n, len] of units(cell)) {
    if (off <= acc + len) {
      if (n.nodeType === Node.TEXT_NODE) return [n, off - acc];
      const i = [...n.parentNode.childNodes].indexOf(n);
      return [n.parentNode, off === acc ? i : i + 1];
    }
    acc += len;
  }
  return [cell, cell.childNodes.length];
}

export class SegmentsView {
  constructor(view, host) {
    Object.assign(this, { view, ed: view.editor, host });
    this.all = [];
    this.at = new Map();
    this.els = new Map();
    this.active = null;
    this.typing = false;
    this.search = '';
    this.onlyOpen = false;
    this.build();
  }

  build() {
    this.host.innerHTML = `
      <div class="seg-bar">
        <input type="search" class="seg-search" placeholder="Search the original and the translation" aria-label="Search">
        <label class="toggle"><input type="checkbox" class="seg-open"> Only rows not done</label>
        <span class="seg-count"></span>
      </div>
      <div class="seg-body">
        <div class="seg-list" role="grid" aria-label="Segments"></div>
        <aside class="seg-side" aria-label="The segment"></aside>
      </div>`;
    this.list = this.host.querySelector('.seg-list');
    this.side = this.host.querySelector('.seg-side');
    this.count = this.host.querySelector('.seg-count');
    let t = null;
    this.host.querySelector('.seg-search').addEventListener('input', e => {
      this.search = e.target.value;
      clearTimeout(t);
      t = setTimeout(() => this.render(), 200);
    });
    this.host.querySelector('.seg-open').addEventListener('change', e => {
      this.onlyOpen = e.target.checked;
      this.render();
    });
  }

  show() {
    this.host.hidden = false;
    this.host.closest('.work')?.classList.add('text-mode');
    this.load();
  }

  hide() {
    this.host.hidden = true;
    this.host.closest('.work')?.classList.remove('text-mode');
  }

  load() {
    this.all = JSON.parse(this.ed.rows());
    this.reindex();
    this.render();
    this.active = null;
    const first = this.all[0];
    if (first) this.place(first.pid, 0, 0);
  }

  reindex() {
    this.at = new Map(this.all.map((r, i) => [r.pid, i]));
  }

  matches(r) {
    if (this.onlyOpen && r.done) return false;
    if (!this.search) return true;
    const q = this.search.toLowerCase();
    return textOf(r.src).toLowerCase().includes(q) || textOf(r.tgt).toLowerCase().includes(q);
  }

  render() {
    this.list.textContent = '';
    this.els.clear();
    const frag = document.createDocumentFragment();
    let shown = 0;
    for (const r of this.all) {
      if (!this.matches(r)) continue;
      frag.append(this.rowEl(r));
      shown++;
    }
    this.list.append(frag);
    this.count.textContent = shown === this.all.length ? `${this.all.length} segments` : `${shown} of ${this.all.length} segments`;
    if (this.active) this.els.get(this.active)?.row.classList.add('active');
  }

  rowEl(r) {
    const row = document.createElement('div');
    row.className = 'seg-row';
    row.dataset.pid = r.pid;
    row.setAttribute('role', 'row');
    const n = document.createElement('div');
    n.className = 'seg-n';
    const src = document.createElement('div');
    src.className = 'seg-src';
    src.innerHTML = html(r.src);
    const tgt = document.createElement('div');
    tgt.className = 'seg-tgt';
    tgt.contentEditable = r.editable ? 'true' : 'false';
    tgt.spellcheck = false;
    tgt.setAttribute('role', 'textbox');
    tgt.setAttribute('aria-label', `Segment ${r.n}`);
    row.append(n, src, tgt);
    const e = { row, n, src, tgt, known: '' };
    this.els.set(r.pid, e);
    this.status(r);
    this.fill(r.pid, r.tgt);
    this.wireCell(r.pid, e);
    src.addEventListener('contextmenu', ev => {
      ev.preventDefault();
      const sel = String(window.getSelection() ?? '').trim();
      const lookup = sel && sel.length < 60 && sel.split(/\s+/).length <= 5 ? sel : null;
      const row = this.all[this.at.get(r.pid)];
      this.view.menu.open({ panel: 'src', text: sel, lookup, paragraph: textOf(row?.src) }, ev.clientX, ev.clientY);
    });
    return row;
  }

  status(r) {
    const e = this.els.get(r.pid);
    if (!e) return;
    e.n.innerHTML = `<span>${r.n}</span>${r.part ? `<small>${esc(r.part)}</small>` : ''}${r.done ? '<i class="done" title="Done">✓</i>' : r.edited ? '<i class="edited" title="Edited">●</i>' : ''}`;
    e.row.classList.toggle('done', !!r.done);
  }

  // The cell's text from the editor (keeping the cursor where it was, if it has it).
  fill(pid, spans) {
    const e = this.els.get(pid);
    if (!e) return;
    const focused = document.activeElement === e.tgt;
    const sel = focused ? this.selection(e.tgt) : null;
    e.tgt.innerHTML = html(spans);
    e.known = domText(e.tgt);
    if (sel) this.select(e.tgt, sel[0], sel[1]);
  }

  selection(cell) {
    const s = window.getSelection();
    if (!s || !s.anchorNode || !cell.contains(s.anchorNode)) return [0, 0];
    return [offsetOf(cell, s.anchorNode, s.anchorOffset), offsetOf(cell, s.focusNode, s.focusOffset)];
  }

  select(cell, anchor, focus) {
    const s = window.getSelection();
    const [an, ao] = placeOf(cell, anchor);
    const [fn, fo] = placeOf(cell, focus);
    s.setBaseAndExtent(an, ao, fn, fo);
  }

  // The editor's selection is the cell's: keys and menu actions then act there.
  sync(pid) {
    const e = this.els.get(pid);
    if (!e) return;
    const [a, f] = this.selection(e.tgt);
    this.view.answer(this.ed.textSelect(pid, a, f));
  }

  // The cursor (and selection: anchor to focus) in a row's translation.
  place(pid, anchor, focus) {
    const e = this.els.get(pid);
    if (!e) return;
    if (document.activeElement !== e.tgt) {
      this.placing = true;
      e.tgt.focus({ preventScroll: true });
      this.placing = false;
    }
    this.select(e.tgt, anchor, focus);
    const r = e.row.getBoundingClientRect(), l = this.list.getBoundingClientRect();
    if (r.top < l.top || r.bottom > l.bottom) e.row.scrollIntoView({ block: 'nearest' });
    this.setActive(pid);
  }

  move(pid, dir) {
    const e = this.els.get(pid);
    let next = dir > 0 ? e?.row.nextElementSibling : e?.row.previousElementSibling;
    if (!next) return;
    const p = next.dataset.pid;
    const end = dir < 0 ? domText(this.els.get(p).tgt).length : 0;
    this.place(p, end, end);
    this.sync(p);
  }

  setActive(pid) {
    if (pid === this.active) return;
    if (this.active) this.els.get(this.active)?.row.classList.remove('active');
    this.active = pid;
    this.els.get(pid)?.row.classList.add('active');
    this.panel();
  }

  // After any answer of the editor: rows changed are drawn again, the cursor goes where it is.
  afterAnswer(r) {
    const ch = JSON.parse(this.ed.rowChanges());
    if (ch.all) {
      this.all = ch.rows;
      this.reindex();
      this.render();
    } else {
      for (const row of ch.rows) {
        const i = this.at.get(row.pid);
        if (i !== undefined) this.all[i] = row;
        const e = this.els.get(row.pid);
        if (!e) continue;
        this.status(row);
        // What was just typed stays as it is (the cursor, an input method); anything else is drawn.
        const keep = this.typing && document.activeElement === e.tgt && e.known === plainOf(row.tgt);
        if (!keep) this.fill(row.pid, row.tgt);
      }
    }
    const tc = r.textCaret;
    if (tc && !this.typing) this.place(tc.pid, tc.anchor, tc.at);
    else if (tc) this.setActive(tc.pid);
    this.typing = false;
    this.panel();
  }

  edit(pid, e) {
    const now = domText(e.tgt);
    const was = e.known;
    if (now === was) return;
    let a = 0;
    while (a < was.length && a < now.length && was[a] === now[a]) a++;
    let b = 0;
    while (b < was.length - a && b < now.length - a && was[was.length - 1 - b] === now[now.length - 1 - b]) b++;
    e.known = now;
    this.typing = true;
    this.view.answer(this.ed.textEdit(pid, a, was.length - b, now.slice(a, now.length - b)));
  }

  // Replaces the selection with text (a line break, a tab, what is pasted).
  insert(pid, text) {
    const e = this.els.get(pid);
    const [a, f] = this.selection(e.tgt);
    this.view.answer(this.ed.textEdit(pid, Math.min(a, f), Math.max(a, f), text));
  }

  key(pid, code, mods) {
    this.sync(pid);
    this.view.key(code, '', mods);
  }

  // Marks a row done (or not); marked, on to the next row of the grid (the
  // pages' Ctrl+Enter goes to what is below on the page, here that's the next row).
  done(pid) {
    if (this.els.get(pid) && document.activeElement === this.els.get(pid).tgt) this.sync(pid);
    else this.view.answer(this.ed.textSelect(pid, 0, 0));
    this.view.key(QT.Enter, '', CTRL);
    if (this.all[this.at.get(pid)]?.done) this.move(pid, 1);
  }

  insertClaude(pid) {
    if (!pid) return;
    if (this.els.get(pid) && document.activeElement === this.els.get(pid).tgt) this.sync(pid);
    else this.view.answer(this.ed.textSelect(pid, 0, 0));
    this.view.answer(this.ed.insertClaude());
  }

  wireCell(pid, e) {
    const t = e.tgt;
    let composing = false;
    t.addEventListener('focus', () => {
      this.setActive(pid);
      if (!this.placing) this.sync(pid);
    });
    t.addEventListener('blur', () => {
      // Leaving the row: drawn as the editor has it (the look of what was typed).
      const row = this.all[this.at.get(pid)];
      if (row) this.fill(pid, row.tgt);
    });
    t.addEventListener('compositionstart', () => { composing = true; });
    t.addEventListener('compositionend', () => { composing = false; this.edit(pid, e); });
    t.addEventListener('input', ev => {
      if (composing || ev.isComposing) return;
      this.edit(pid, e);
    });
    t.addEventListener('beforeinput', ev => {
      // The editor's undo, not the browser's.
      if (ev.inputType === 'historyUndo' || ev.inputType === 'historyRedo') {
        ev.preventDefault();
        this.key(pid, ev.inputType === 'historyUndo' ? 0x5a : 0x59, CTRL);
      } else if (ev.inputType === 'insertParagraph') {
        ev.preventDefault();
      } else if (ev.inputType === 'insertLineBreak') {
        ev.preventDefault();
        this.insert(pid, '\n');
      } else if (ev.inputType.startsWith('format')) {
        ev.preventDefault();
      }
    });
    t.addEventListener('keydown', ev => {
      if (ev.isComposing || composing) return;
      const ctrl = ev.ctrlKey || ev.metaKey;
      const k = ev.key.toLowerCase();
      const stop = () => { ev.preventDefault(); ev.stopPropagation(); };
      if (ev.key === 'Enter' && ctrl) { stop(); this.done(pid); return; }
      if (ev.key === 'Enter' && ev.shiftKey) { stop(); this.insert(pid, '\n'); return; }
      if (ev.key === 'Enter') { stop(); this.move(pid, 1); return; }
      if (ev.key === 'Tab' && !ctrl && !ev.shiftKey) { stop(); this.insert(pid, '\t'); return; }
      if (ctrl && ['b', 'i', 'u', 'z', 'y'].includes(k)) { stop(); this.key(pid, k.toUpperCase().charCodeAt(0), (ev.shiftKey ? SHIFT : 0) | CTRL); return; }
      if (ctrl && k === 'k') { stop(); this.sync(pid); this.view.pasteLink(); return; }
      if (ctrl && k === '1') { stop(); this.insertClaude(pid); return; }
      const [a, f] = this.selection(t);
      if (ev.key === 'ArrowUp' && !ev.shiftKey && a === f && a === 0) { stop(); this.move(pid, -1); return; }
      if (ev.key === 'ArrowDown' && !ev.shiftKey && a === f && f === domText(t).length) { stop(); this.move(pid, 1); return; }
    });
    t.addEventListener('paste', ev => {
      ev.preventDefault();
      this.insert(pid, ev.clipboardData.getData('text/plain'));
    });
    t.addEventListener('copy', ev => {
      this.sync(pid);
      const text = this.ed.selectedText();
      if (!text) return;
      ev.clipboardData.setData('text/plain', text);
      ev.preventDefault();
    });
    t.addEventListener('cut', ev => {
      this.sync(pid);
      const text = this.ed.selectedText();
      ev.preventDefault();
      if (!text) return;
      ev.clipboardData.setData('text/plain', text);
      this.view.key(QT.Delete, '', 0);
    });
    t.addEventListener('contextmenu', ev => {
      ev.preventDefault();
      this.sync(pid);
      this.view.menu.open({ panel: 'tgt' }, ev.clientX, ev.clientY);
    });
  }

  // The row being worked on: Claude's translation of it, and what to do with it.
  panel() {
    const r = this.all[this.at.get(this.active)];
    if (!r) { this.side.textContent = ''; return; }
    const claude = JSON.parse(this.ed.claudeSpans(r.pid));
    const attached = !!this.view.state.claude;
    this.side.innerHTML = `
      <h3>Segment ${r.n}${r.part ? ` · ${esc(r.part)}` : ''}</h3>
      <h4>Claude</h4>
      ${claude ? `<div class="seg-claude">${html(claude)}</div><button type="button" class="primary" data-act="claude" title="Ctrl+1">Insert Claude’s translation</button>`
        : `<p class="fine">${attached ? 'Claude’s translation has no segment for this row.' : 'No translation by Claude is attached to this document.'}</p>
           ${attached ? '' : '<button type="button" class="ghost" data-act="attach">Attach Claude’s translation…</button>'}`}
      <hr>
      <button type="button" class="ghost" data-act="done" title="Ctrl+Enter: and go to the next segment">${r.done ? 'Unmark as done' : 'Mark as done'}</button>
      <button type="button" class="ghost" data-act="revert" ${r.edited ? '' : 'disabled'}>Revert to the original</button>`;
    this.side.querySelectorAll('button').forEach(b => b.addEventListener('click', () => {
      const act = b.dataset.act;
      if (act === 'claude') this.insertClaude(r.pid);
      else if (act === 'attach') this.view.attachClaude();
      else if (act === 'done') this.done(r.pid);
      else if (act === 'revert') { this.view.answer(this.ed.textSelect(r.pid, 0, 0)); this.view.answer(this.ed.revert()); }
    }));
  }
}
