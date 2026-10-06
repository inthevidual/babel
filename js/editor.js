// The target panel as an editor. Every rendered paragraph is its own editing
// host, so the browser can never merge or split paragraphs: the document's
// structure is fixed and only text changes. Runs that are not text (images,
// footnote references, fields, tabs, breaks) are non-editable islands.

const HOST = 'p[data-p]';
const ALLOWED_INPUT = new Set([
  'insertText', 'insertReplacementText', 'insertCompositionText', 'insertFromComposition',
  'deleteCompositionText', 'deleteByComposition',
  'deleteContentBackward', 'deleteContentForward', 'deleteContent', 'deleteWordBackward', 'deleteWordForward',
  'deleteSoftLineBackward', 'deleteSoftLineForward', 'deleteHardLineBackward', 'deleteHardLineForward',
  'deleteEntireSoftLine', 'deleteByCut', 'historyUndo', 'historyRedo',
  'formatBold', 'formatItalic', 'formatUnderline',
]);

export const hostOf = node => (node?.nodeType === 1 ? node : node?.parentElement)?.closest(HOST) ?? null;

export function makeEditable(root, { lang, spellcheck }) {
  root.lang = lang;
  for (const p of root.querySelectorAll(HOST)) {
    p.contentEditable = 'true';
    p.spellcheck = !!spellcheck;
  }
  for (const s of root.querySelectorAll('span[data-r]:not([data-k="t"])')) s.contentEditable = 'false';
  for (const a of root.querySelectorAll('a[href]')) a.removeAttribute('href');
}

// Fragments that make up one paragraph: every page-split piece of a body
// paragraph; only the instance being edited for headers and footers, which
// repeat on every page.
export function fragments(root, pid, host) {
  if (host?.closest('header, footer')) return [host];
  return [...root.querySelectorAll(`p[data-p="${CSS.escape(pid)}"]`)];
}

const effective = (el, stop) => {
  const cs = getComputedStyle(el);
  let u = false;
  for (let a = el; a && a !== stop; a = a.parentElement) {
    if (getComputedStyle(a).textDecorationLine.includes('underline')) { u = true; break; }
  }
  return { b: parseInt(cs.fontWeight, 10) >= 600, i: cs.fontStyle !== 'normal', u };
};

// Reads a paragraph back from the DOM: tokens for the XML writer, plus the
// plain text and a node map for proofing.
export function readParagraph(frags) {
  const tokens = [];
  const segs = [];
  let text = '';
  let lastSpan = null;
  for (const frag of frags) {
    const runOf = node => {
      for (let a = node.parentElement; a && a !== frag; a = a.parentElement) if (a.dataset.r) return a;
      return null;
    };
    const walk = el => {
      for (const n of el.childNodes) {
        if (n.nodeType === 3) {
          if (!n.data.length) continue;
          const span = runOf(n);
          const rid = span?.dataset.k === 't' ? span.dataset.r : null;
          const base = rid ? span : lastSpan;
          let fmt = null;
          if (n.parentElement !== span && !/^(SUP|SUB)$/.test(n.parentElement.tagName)) {
            const a = effective(n.parentElement, frag);
            const b = effective(base ?? frag, frag);
            fmt = {};
            for (const k of ['b', 'i', 'u']) if (a[k] !== b[k]) fmt[k] = a[k];
          }
          tokens.push({ t: 'text', rid, base: base?.dataset.r ?? null, text: n.data, fmt });
          segs.push({ node: n, start: text.length });
          text += n.data;
          if (rid) lastSpan = span;
        } else if (n.nodeType === 1) {
          if (n.matches(HOST)) continue; // nested (text box) paragraph
          if (n.classList.contains('b-br')) {
            tokens.push({ t: 'br', base: lastSpan?.dataset.r ?? null });
            text += '\n';
            continue;
          }
          if (n.dataset.r && n.dataset.k !== 't') {
            tokens.push({ t: 'atom', rid: n.dataset.r });
            if (n.dataset.k === 'tab') text += '\t';
            else if (n.dataset.k === 'br') text += '\n';
            continue;
          }
          if (n.tagName === 'BR') continue; // placeholder the browser adds to empty hosts
          walk(n);
        }
      }
    };
    walk(frag);
  }
  return { tokens, text, segs };
}

// Run ids rendered at load: text runs may be rewritten, breaks and tabs may
// be deleted; nothing else is ever removed from the XML.
export function runInfo(frags) {
  const textRuns = new Set();
  const atoms = new Set();
  for (const f of frags) {
    for (const s of f.querySelectorAll('span[data-r]')) {
      if (hostOf(s) !== f) continue;
      if (s.dataset.k === 't') textRuns.add(s.dataset.r);
      else if (s.dataset.k === 'br' || s.dataset.k === 'tab') atoms.add(s.dataset.r);
    }
  }
  return { textRuns, atoms };
}

// ── Caret helpers ──────────────────────────────────────────────────────────

export function placeCaret(host, where = 'start') {
  host.focus({ preventScroll: true });
  const sel = getSelection();
  const r = document.createRange();
  if (where.x != null) {
    const pos = document.caretPositionFromPoint?.(where.x, where.y);
    const cr = pos ? null : document.caretRangeFromPoint?.(where.x, where.y);
    const node = pos?.offsetNode ?? cr?.startContainer;
    if (node && hostOf(node) === host) {
      r.setStart(node, pos?.offset ?? cr.startOffset);
      r.collapse(true);
      sel.removeAllRanges();
      sel.addRange(r);
      return;
    }
    where = 'start';
  }
  const walker = document.createTreeWalker(host, NodeFilter.SHOW_TEXT, {
    acceptNode: n => (hostOf(n) === host && !n.parentElement.closest('[contenteditable="false"]')
      ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT),
  });
  const texts = [];
  for (let n; (n = walker.nextNode());) texts.push(n);
  if (!texts.length) { r.selectNodeContents(host); r.collapse(where === 'start'); }
  else if (where === 'start') r.setStart(texts[0], 0);
  else r.setStart(texts.at(-1), texts.at(-1).length);
  r.collapse(true);
  sel.removeAllRanges();
  sel.addRange(r);
}

export function selectWhole(host) {
  host.focus({ preventScroll: true });
  const r = document.createRange();
  r.selectNodeContents(host);
  getSelection().removeAllRanges();
  getSelection().addRange(r);
}

const caretRect = () => {
  const sel = getSelection();
  if (!sel.rangeCount) return null;
  const r = sel.getRangeAt(0).cloneRange();
  let rect = r.getClientRects()[0];
  if (!rect || (!rect.height && !rect.width)) {
    const c = r.startContainer;
    if (c.nodeType === 3 && c.length) {
      if (r.startOffset < c.length) r.setEnd(c, r.startOffset + 1);
      else r.setStart(c, r.startOffset - 1);
      const rs = r.getClientRects();
      rect = rs[0] ?? rect;
    }
  }
  if (!rect || !rect.height) {
    const h = hostOf(sel.anchorNode);
    return h ? h.getBoundingClientRect() : null;
  }
  return rect;
};

const lineRects = host => {
  const r = document.createRange();
  r.selectNodeContents(host);
  return [...r.getClientRects()].filter(x => x.height > 0);
};

const atEdge = (host, dir) => {
  const sel = getSelection();
  if (!sel.rangeCount || !sel.isCollapsed) return false;
  const r = document.createRange();
  r.selectNodeContents(host);
  const caret = sel.getRangeAt(0);
  if (dir < 0) r.setEnd(caret.startContainer, caret.startOffset);
  else r.setStart(caret.endContainer, caret.endOffset);
  return r.toString().length === 0;
};

const onFirstLine = host => {
  const c = caretRect(); const lines = lineRects(host);
  return !c || !lines.length || c.top < lines[0].bottom - 2;
};
const onLastLine = host => {
  const c = caretRect(); const lines = lineRects(host);
  return !c || !lines.length || c.bottom > lines.at(-1).top + 2;
};

const isProtected = a => {
  const box = a.getBoundingClientRect();
  return box.width > 0 || box.height > 0;
};

// -1, 0, 1: where point a lies relative to point b.
const cmpPoints = (an, ao, bn, bo) => {
  const r = document.createRange();
  r.setStart(bn, bo);
  return r.comparePoint(an, ao);
};

// True when the range actually covers part of a visible non-text run, not
// merely touches its edge.
export function rangeTouchesProtected(host, r) {
  for (const a of host.querySelectorAll('span[data-k="a"], span[data-k="page"], span[data-k="numpages"]')) {
    if (!isProtected(a)) continue;
    const parent = a.parentNode;
    const i = [...parent.childNodes].indexOf(a);
    const startsBeforeEnd = cmpPoints(parent, i, r.endContainer, r.endOffset) < 0;
    const endsAfterStart = cmpPoints(parent, i + 1, r.startContainer, r.startOffset) > 0;
    if (startsBeforeEnd && endsAfterStart) return true;
  }
  return false;
}

// What Backspace/Delete would remove, found by extending a copy of the
// selection the way the browser would. Chrome reports no target range when
// the thing deleted is a non-editable island, so beforeinput can't see it.
function wouldDeleteProtected(host, dir, unit) {
  const sel = getSelection();
  if (!sel.rangeCount) return false;
  const saved = sel.getRangeAt(0).cloneRange();
  let r = saved;
  if (saved.collapsed) {
    sel.modify('extend', dir, unit);
    r = sel.rangeCount ? sel.getRangeAt(0).cloneRange() : saved;
    sel.removeAllRanges();
    sel.addRange(saved);
  }
  return rangeTouchesProtected(host, r);
}

const caretAt = (x, y) => {
  const pos = document.caretPositionFromPoint?.(x, y);
  if (pos) return { node: pos.offsetNode, offset: pos.offset };
  const r = document.caretRangeFromPoint?.(x, y);
  return r ? { node: r.startContainer, offset: r.startOffset } : null;
};

// A plain click inside an existing selection makes Chrome hold the selection
// and wait to see whether a drag of the selected text begins, which hands
// over to the platform's drag-and-drop machinery. Babel never drags text, so
// that path is skipped: the click places the caret at once, as anywhere else.
// Dragging to select, double/triple click and Shift+click are untouched.
export function guardSelectionClicks(root, { editable }) {
  root.addEventListener('mousedown', e => {
    if (e.button !== 0 || e.detail !== 1 || e.shiftKey || e.altKey || e.ctrlKey || e.metaKey) return;
    const sel = getSelection();
    if (!sel.rangeCount || sel.isCollapsed) return;
    const range = sel.getRangeAt(0);
    if (!root.contains(range.commonAncestorContainer)) return;
    const inside = [...range.getClientRects()].some(r =>
      e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom);
    if (!inside) return;
    e.preventDefault();
    if (!editable) { sel.removeAllRanges(); return; }
    const host = hostOf(e.target);
    const pos = caretAt(e.clientX, e.clientY);
    if (!host || !pos || hostOf(pos.node) !== host) { sel.removeAllRanges(); return; }
    if (document.activeElement !== host) host.focus({ preventScroll: true });
    sel.collapse(pos.node, pos.offset);
  }, true);
}

// ── Behaviour ──────────────────────────────────────────────────────────────

export function attach(root, cb) {
  // cb: { input(host), focus(host), next(host, dir, mode), confirm(host),
  //       blocked(message), lineBreak(host) }
  root.addEventListener('beforeinput', e => {
    const host = hostOf(e.target);
    if (!host) return;
    if (e.inputType === 'insertParagraph') { e.preventDefault(); return; }
    if (e.inputType === 'insertLineBreak') { e.preventDefault(); insertBreak(host); return; }
    if (e.inputType === 'insertFromPaste' || e.inputType === 'insertFromDrop') { e.preventDefault(); return; }
    if (!ALLOWED_INPUT.has(e.inputType)) { e.preventDefault(); return; }
    if (/^delete|^insert(Text|ReplacementText)$/.test(e.inputType)) {
      for (const sr of e.getTargetRanges()) {
        const r = document.createRange();
        try { r.setStart(sr.startContainer, sr.startOffset); r.setEnd(sr.endContainer, sr.endOffset); }
        catch { continue; }
        if (r.collapsed) continue;
        if (rangeTouchesProtected(host, r)) {
          e.preventDefault();
          cb.blocked('Images, footnote references and fields are part of the layout and can’t be deleted.');
          return;
        }
      }
    }
  });

  root.addEventListener('input', e => {
    const host = hostOf(e.target);
    if (host) cb.input(host);
  });

  root.addEventListener('paste', e => {
    const host = hostOf(e.target);
    if (!host) return;
    e.preventDefault();
    const text = (e.clipboardData?.getData('text/plain') ?? '').replace(/\r?\n+/g, ' ').replace(/\t/g, ' ');
    if (text) document.execCommand('insertText', false, text);
  });
  root.addEventListener('drop', e => e.preventDefault());
  root.addEventListener('dragstart', e => e.preventDefault());

  root.addEventListener('focusin', e => {
    const host = hostOf(e.target);
    if (host) cb.focus(host);
  });

  root.addEventListener('keydown', e => {
    const host = hostOf(e.target);
    if (!host) return;
    const plain = !e.ctrlKey && !e.metaKey && !e.altKey;
    if (e.key === 'Enter') {
      e.preventDefault();
      if (e.shiftKey && !e.ctrlKey && !e.metaKey) insertBreak(host);
      else if (e.ctrlKey || e.metaKey) cb.confirm(host);
      else cb.next(host, 1, 'paragraph');
      return;
    }
    if (e.key === 'Tab' && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      cb.next(host, e.shiftKey ? -1 : 1, 'paragraph');
      return;
    }
    if (e.altKey && !e.ctrlKey && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
      e.preventDefault();
      cb.next(host, e.key === 'ArrowDown' ? 1 : -1, 'todo');
      return;
    }
    if ((e.key === 'Backspace' || e.key === 'Delete') && !e.altKey && wouldDeleteProtected(host, e.key === 'Delete' ? 'forward' : 'backward', e.ctrlKey || e.metaKey ? 'word' : 'character')) {
      e.preventDefault();
      cb.blocked('Images, footnote references and fields are part of the layout and can’t be deleted.');
      return;
    }
    if (!plain || e.shiftKey) return;
    if (e.key === 'ArrowDown' && onLastLine(host)) {
      const c = caretRect();
      if (cb.next(host, 1, 'spatial', c ? c.left : null)) e.preventDefault();
    } else if (e.key === 'ArrowUp' && onFirstLine(host)) {
      const c = caretRect();
      if (cb.next(host, -1, 'spatial', c ? c.left : null)) e.preventDefault();
    } else if (e.key === 'ArrowRight' && atEdge(host, 1)) {
      if (cb.next(host, 1, 'adjacent')) e.preventDefault();
    } else if (e.key === 'ArrowLeft' && atEdge(host, -1)) {
      if (cb.next(host, -1, 'adjacent')) e.preventDefault();
    }
  });

  function insertBreak(host) {
    const sel = getSelection();
    if (!sel.rangeCount || hostOf(sel.anchorNode) !== host) return;
    const r = sel.getRangeAt(0);
    r.deleteContents();
    const span = document.createElement('span');
    span.className = 'b-br';
    span.contentEditable = 'false';
    span.appendChild(document.createElement('br'));
    r.insertNode(span);
    // Text after the break needs a node to land in.
    const after = document.createTextNode('');
    span.after(after);
    r.setStart(after, 0);
    r.collapse(true);
    sel.removeAllRanges();
    sel.addRange(r);
    cb.input(host);
  }
}

// Ranges in the DOM for a [start, end) span of a paragraph's text.
export function rangeFor(segs, start, end) {
  const locate = (off, preferEnd) => {
    for (let i = 0; i < segs.length; i++) {
      const s = segs[i];
      const len = s.node.length;
      if (off < s.start + len || (preferEnd && off === s.start + len)) {
        if (off >= s.start) return [s.node, off - s.start];
      }
    }
    return null;
  };
  const a = locate(start, false);
  const b = locate(end, true);
  if (!a || !b || !a[0].isConnected || !b[0].isConnected) return null;
  const r = new Range();
  try { r.setStart(a[0], a[1]); r.setEnd(b[0], b[1]); } catch { return null; }
  return r;
}
