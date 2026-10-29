// OOXML layer. The original package is the source of truth: Babel never
// regenerates a document, it only rewrites the text of runs inside the XML it
// was given. Everything this file does not touch survives byte-for-byte.

export const W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
export const R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const MC = 'http://schemas.openxmlformats.org/markup-compatibility/2006';
const XMLNS_XML = 'http://www.w3.org/XML/1998/namespace';
const PKG_REL = 'http://schemas.openxmlformats.org/package/2006/relationships';
const OFFICE_DOC = /\/officeDocument$/;
const TEXT_REL = /\/(header|footer|footnotes|endnotes)$/;

const JSZip = globalThis.JSZip;

// ── Package ────────────────────────────────────────────────────────────────

export const relsPathFor = path => {
  const i = path.lastIndexOf('/');
  return `${path.slice(0, i + 1)}_rels/${path.slice(i + 1)}.rels`;
};

const resolve = (base, target) => {
  if (target.startsWith('/')) return target.slice(1);
  const parts = base.split('/').slice(0, -1);
  for (const seg of target.split('/')) {
    if (seg === '..') parts.pop();
    else if (seg !== '.') parts.push(seg);
  }
  return parts.join('/');
};

export const parseXml = str => {
  const doc = new DOMParser().parseFromString(str, 'application/xml');
  const err = doc.getElementsByTagName('parsererror')[0];
  if (err) throw new Error('XML parse error: ' + err.textContent.slice(0, 200));
  return doc;
};

const XML_DECL = /^﻿?\s*<\?xml[^>]*\?>\s*/;

export const serializeXml = (doc, original) => {
  const body = new XMLSerializer().serializeToString(doc).replace(XML_DECL, '');
  const decl = (original ?? '').match(XML_DECL)?.[0].trim()
    ?? '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>';
  return decl + '\r\n' + body;
};

// Opens a .docx and finds the parts that carry translatable paragraphs.
export async function openPackage(bytes) {
  let zip;
  try {
    zip = await JSZip.loadAsync(bytes);
  } catch {
    throw new Error('This is not a Word document (.docx). Older .doc files must be saved as .docx in Word first.');
  }
  const rootRels = zip.file('_rels/.rels');
  if (!rootRels) throw new Error('The file is a zip archive but not a Word document.');
  const rels = parseXml(await rootRels.async('string'));
  const main = [...rels.getElementsByTagNameNS(PKG_REL, 'Relationship')]
    .find(r => OFFICE_DOC.test(r.getAttribute('Type')));
  if (!main) throw new Error('No main document part was found in this file.');
  const mainPath = resolve('', main.getAttribute('Target'));
  if (!zip.file(mainPath)) throw new Error('The main document part is missing.');

  const textParts = [mainPath];
  const mainRels = zip.file(relsPathFor(mainPath));
  if (mainRels) {
    const r = parseXml(await mainRels.async('string'));
    for (const rel of r.getElementsByTagNameNS(PKG_REL, 'Relationship')) {
      if (rel.getAttribute('TargetMode') === 'External') continue;
      if (!TEXT_REL.test(rel.getAttribute('Type'))) continue;
      const p = resolve(mainPath, rel.getAttribute('Target'));
      if (zip.file(p) && !textParts.includes(p)) textParts.push(p);
    }
  }
  const stylesPath = textParts.length && zip.file(resolve(mainPath, 'styles.xml')) ? resolve(mainPath, 'styles.xml') : null;
  const settingsPath = zip.file(resolve(mainPath, 'settings.xml')) ? resolve(mainPath, 'settings.xml') : null;
  return { zip, mainPath, textParts, stylesPath, settingsPath };
}

// A short, stable key per part, used as the prefix of paragraph ids.
export const partKey = path => path.split('/').pop().replace(/\.xml$/, '');

export const partKind = path => {
  const k = partKey(path);
  if (/^header\d*$/.test(k)) return 'header';
  if (/^footer\d*$/.test(k)) return 'footer';
  if (k === 'footnotes') return 'footnotes';
  if (k === 'endnotes') return 'endnotes';
  return 'main';
};

// ── Normalisation ──────────────────────────────────────────────────────────
// Every run is split so it holds exactly one content element, and every
// paragraph and run gets an id. Splitting a run is invisible to Word (runs
// with identical properties are equivalent to one run) and makes the mapping
// between HTML spans and XML runs one-to-one.

const isW = (el, name) => el.namespaceURI === W && el.localName === name;
const kids = el => [...el.children];

export function normalize(doc, key) {
  for (const r of [...doc.getElementsByTagNameNS(W, 'r')]) {
    const rPr = kids(r).find(k => isW(k, 'rPr'));
    const content = kids(r).filter(k => k !== rPr);
    let ref = r;
    for (const c of content.slice(1)) {
      const nr = r.cloneNode(false);
      nr.removeAttribute('data-br');
      if (rPr) nr.appendChild(rPr.cloneNode(true));
      nr.appendChild(c);
      ref.after(nr);
      ref = nr;
    }
  }
  ensureIds(doc, key);
  return doc;
}

// Gives ids to anything that lacks one (runs Babel generated in an earlier
// session become first-class runs) without ever renumbering existing ids.
export function ensureIds(doc, key) {
  const used = new Set();
  const all = (name, attr) => [...doc.getElementsByTagNameNS(W, name)].map(el => {
    const v = el.getAttribute(attr);
    if (v) used.add(v);
    return el;
  });
  const ps = all('p', 'data-bp');
  const rs = all('r', 'data-br');
  let n = 0;
  for (const p of ps) {
    if (p.getAttribute('data-bp')) continue;
    while (used.has(`${key}:${n}`)) n++;
    p.setAttribute('data-bp', `${key}:${n}`);
    used.add(`${key}:${n}`);
  }
  n = 0;
  for (const r of rs) {
    r.removeAttribute('data-bgen');
    if (r.getAttribute('data-br')) continue;
    while (used.has(`${key}:r${n}`)) n++;
    r.setAttribute('data-br', `${key}:r${n}`);
    used.add(`${key}:r${n}`);
  }
  markFields(doc);
}

// Field results are not translatable text. PAGE and NUMPAGES are marked so
// the view can show real page numbers.
function markFields(doc) {
  const kindOf = instr => (/^\s*PAGE\b/i.test(instr) ? 'page' : /^\s*(NUMPAGES|SECTIONPAGES)\b/i.test(instr) ? 'numpages' : null);
  for (const f of doc.getElementsByTagNameNS(W, 'fldSimple')) {
    const k = kindOf(f.getAttribute('w:instr') ?? '') ?? 'a';
    for (const r of f.getElementsByTagNameNS(W, 'r')) r.setAttribute('data-bk', k);
  }
  // Complex fields: begin … instrText … separate … result … end, possibly nested.
  for (const p of doc.getElementsByTagNameNS(W, 'p')) {
    const stack = [];
    for (const r of p.getElementsByTagNameNS(W, 'r')) {
      if (ownerPara(r) !== p) continue;
      const fc = kids(r).find(k => isW(k, 'fldChar'));
      const type = fc?.getAttribute('w:fldCharType');
      if (type === 'begin') { stack.push({ instr: '', result: false }); continue; }
      if (type === 'separate') { if (stack.length) stack.at(-1).result = true; continue; }
      if (type === 'end') { stack.pop(); continue; }
      const top = stack.at(-1);
      if (!top) continue;
      const it = kids(r).find(k => isW(k, 'instrText'));
      if (it && !top.result) top.instr += it.textContent;
      else if (top.result) {
        const k = kindOf(top.instr);
        if (k) r.setAttribute('data-bk', k);
      }
    }
  }
}

const ancestor = (el, pred) => {
  for (let a = el.parentElement; a; a = a.parentElement) if (pred(a)) return a;
  return null;
};

const ownerPara = el => ancestor(el, a => isW(a, 'p'));

// ── Text ───────────────────────────────────────────────────────────────────

// The paragraph's own text (not nested text-box paragraphs, not deleted text,
// not field codes). Tabs and breaks are kept as \t and \n.
export function paraText(p) {
  let out = '';
  const walk = el => {
    for (const c of el.children) {
      if (c.namespaceURI === MC && c.localName === 'Fallback') continue;
      if (c.namespaceURI !== W) { walk(c); continue; }
      switch (c.localName) {
        case 'p': break;
        case 't': out += c.textContent; break;
        case 'tab': case 'ptab': out += '\t'; break;
        case 'br': case 'cr':
          if ((c.getAttribute('w:type') ?? 'textWrapping') === 'textWrapping') out += '\n';
          break;
        case 'noBreakHyphen': out += '‑'; break;
        case 'sym': out += String.fromCharCode(parseInt(c.getAttribute('w:char') ?? '0', 16) || 0x25a1); break;
        case 'del': case 'delText': case 'instrText': case 'moveFrom': break;
        default: walk(c);
      }
    }
  };
  walk(p);
  return out;
}

// Word's "Characters (with spaces)": text only, no tabs or paragraph marks.
export const countChars = text => {
  let n = 0;
  for (const ch of text) if (ch !== '\t' && ch !== '\n') n++;
  return n;
};
export const countWords = text => (text.match(/[^\s‑]+/g) ?? []).length;

// Paragraphs a translator works on: excludes Fallback copies of text boxes.
export function paragraphs(doc) {
  return [...doc.getElementsByTagNameNS(W, 'p')]
    .filter(p => !ancestor(p, a => a.namespaceURI === MC && a.localName === 'Fallback'));
}

// ── Write-back ─────────────────────────────────────────────────────────────
// tokens come from the editor, in visual order:
//   {t:'text', rid, base, text, fmt}   rid = run the text sits in (or null),
//                                       base = run to borrow formatting from
//   {t:'atom', rid}                     a non-text run that is still present
//   {t:'br', base}                      a line break the translator inserted
// info.textRuns / info.atoms: run ids that were rendered when the session
// started; only those are ever rewritten or removed.

const RPR_ORDER = ['rStyle', 'rFonts', 'b', 'bCs', 'i', 'iCs', 'caps', 'smallCaps', 'strike', 'dstrike',
  'outline', 'shadow', 'emboss', 'imprint', 'noProof', 'snapToGrid', 'vanish', 'webHidden', 'color',
  'spacing', 'w', 'kern', 'position', 'sz', 'szCs', 'highlight', 'u', 'effect', 'bdr', 'shd', 'fitText',
  'vertAlign', 'rtl', 'cs', 'em', 'lang', 'eastAsianLayout', 'specVanish', 'oMath'];

// Inserts or replaces a w:rPr child in schema order (Word is strict about it).
export function setRPrChild(rPr, name, attrs) {
  const doc = rPr.ownerDocument;
  for (const c of kids(rPr)) if (isW(c, name)) c.remove();
  if (attrs === null) return;
  const el = doc.createElementNS(W, 'w:' + name);
  for (const [k, v] of Object.entries(attrs)) el.setAttributeNS(W, 'w:' + k, v);
  const rank = RPR_ORDER.indexOf(name);
  const before = kids(rPr).find(c => c.namespaceURI === W && RPR_ORDER.indexOf(c.localName) > rank);
  rPr.insertBefore(el, before ?? null);
  return el;
}

const FMT = {
  b: on => ({ b: on ? {} : { val: '0' }, bCs: on ? {} : { val: '0' } }),
  i: on => ({ i: on ? {} : { val: '0' }, iCs: on ? {} : { val: '0' } }),
  u: on => ({ u: { val: on ? 'single' : 'none' } }),
};

function buildRun(doc, templateRPr, seg) {
  const r = doc.createElementNS(W, 'w:r');
  let rPr = templateRPr ? templateRPr.cloneNode(true) : null;
  if (seg.fmt && Object.keys(seg.fmt).length) {
    rPr ??= doc.createElementNS(W, 'w:rPr');
    for (const [k, on] of Object.entries(seg.fmt))
      for (const [name, attrs] of Object.entries(FMT[k](on))) setRPrChild(rPr, name, attrs);
  }
  if (rPr) r.appendChild(rPr);
  if (seg.br) {
    r.appendChild(doc.createElementNS(W, 'w:br'));
  } else {
    const t = doc.createElementNS(W, 'w:t');
    t.setAttributeNS(XMLNS_XML, 'xml:space', 'preserve');
    t.textContent = seg.text;
    r.appendChild(t);
  }
  return r;
}

// Paragraph-mark formatting minus revision markers: what Word gives new text
// typed into an empty paragraph.
function paragraphMarkRPr(p) {
  const pPr = kids(p).find(k => isW(k, 'pPr'));
  const rPr = pPr && kids(pPr).find(k => isW(k, 'rPr'));
  if (!rPr) return null;
  const c = rPr.cloneNode(true);
  for (const k of kids(c)) if (['ins', 'del', 'moveFrom', 'moveTo', 'rPrChange'].includes(k.localName)) k.remove();
  return c;
}

export function applyParagraph(p, tokens, info, templates) {
  const doc = p.ownerDocument;
  const own = new Map();
  for (const r of p.getElementsByTagNameNS(W, 'r')) {
    if (ownerPara(r) !== p) continue;
    if (r.hasAttribute('data-bgen')) continue;
    own.set(r.getAttribute('data-br'), r);
  }
  for (const r of [...p.querySelectorAll('[data-bgen]')]) if (ownerPara(r) === p) r.remove();

  // The rPr each run had when this session rendered it. Formatting deltas are
  // always relative to that, never cumulative.
  const tmpl = rid => {
    if (!rid) return null;
    if (!templates.has(rid)) {
      const r = own.get(rid);
      const rPr = r && kids(r).find(k => isW(k, 'rPr'));
      templates.set(rid, rPr ? rPr.cloneNode(true) : null);
    }
    return templates.get(rid);
  };

  // Group segments by the XML node they belong after.
  const groups = new Map(); // key -> {segs, base}
  let key = '^';
  const group = (k, base) => {
    if (!groups.has(k)) groups.set(k, { segs: [], base });
    return groups.get(k);
  };
  const presentAtoms = new Set();
  for (const tok of tokens) {
    if (tok.t === 'atom') {
      presentAtoms.add(tok.rid);
      key = 'after:' + tok.rid;
    } else if (tok.t === 'text' && tok.rid && info.textRuns.has(tok.rid) && own.has(tok.rid)) {
      key = tok.rid;
      group(key, tok.rid).segs.push({ text: tok.text, fmt: tok.fmt });
    } else {
      const g = group(key, tok.base);
      g.base ??= tok.base;
      g.segs.push(tok.t === 'br' ? { br: true } : { text: tok.text, fmt: tok.fmt });
    }
  }
  // Merge neighbouring text segments with the same formatting.
  for (const g of groups.values()) {
    const out = [];
    for (const s of g.segs) {
      const prev = out[out.length - 1];
      if (prev && !prev.br && !s.br && JSON.stringify(prev.fmt ?? {}) === JSON.stringify(s.fmt ?? {})) prev.text += s.text;
      else out.push({ ...s });
    }
    g.segs = out;
  }

  const firstTextRid = [...info.textRuns].find(rid => own.has(rid)) ?? null;
  const templateFor = g => (g.base && own.has(g.base) ? tmpl(g.base) : null)
    ?? (firstTextRid ? tmpl(firstTextRid) : paragraphMarkRPr(p));
  const insertClones = (segs, templateRPr, ref, where) => {
    for (const seg of segs) {
      const r = buildRun(doc, templateRPr, seg);
      r.setAttribute('data-bgen', '1');
      if (where === 'before') ref.before(r);
      else { ref.after(r); ref = r; }
    }
  };

  for (const rid of info.textRuns) {
    const r = own.get(rid);
    if (!r) continue;
    const segs = groups.get(rid)?.segs ?? [];
    const first = segs[0] ?? { text: '' };
    const fresh = buildRun(doc, tmpl(rid), first);
    for (const c of kids(r)) c.remove();
    for (const c of kids(fresh)) r.appendChild(c);
    insertClones(segs.slice(1), tmpl(rid), r, 'after');
  }
  for (const [k, g] of groups) {
    if (!k.startsWith('after:')) continue;
    const atom = own.get(k.slice(6));
    if (atom) insertClones(g.segs, templateFor(g), atom, 'after');
  }
  const lead = groups.get('^');
  if (lead?.segs.length) {
    const firstRun = [...own.values()].find(r => !ancestor(r, a => isW(a, 'del') || isW(a, 'moveFrom')));
    if (firstRun) insertClones(lead.segs, templateFor(lead), firstRun, 'before');
    else {
      const holder = doc.createElementNS(W, 'w:r');
      p.appendChild(holder);
      insertClones(lead.segs, templateFor(lead), holder, 'after');
      holder.remove();
    }
  }
  // Breaks and tabs the translator deleted.
  for (const rid of info.atoms) {
    if (!presentAtoms.has(rid)) { own.get(rid)?.remove(); info.atoms.delete(rid); }
  }
}

// Replaces a paragraph with a copy of another (revert to source).
export function replaceParagraph(target, source) {
  const copy = target.ownerDocument.importNode(source, true);
  target.replaceWith(copy);
  return copy;
}

// ── Links ──────────────────────────────────────────────────────────────────

const REL_HYPERLINK = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink';

export const emptyRels = () => parseXml(`<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="${PKG_REL}"/>`);

// id -> target, for hyperlink relationships.
export function relsMap(doc) {
  const m = new Map();
  if (!doc) return m;
  for (const r of doc.getElementsByTagNameNS(PKG_REL, 'Relationship')) m.set(r.getAttribute('Id'), r.getAttribute('Target'));
  return m;
}

// Adds (or reuses) an external hyperlink relationship; returns its id.
export function addLinkRel(doc, url) {
  const rels = [...doc.getElementsByTagNameNS(PKG_REL, 'Relationship')];
  const same = rels.find(r => r.getAttribute('Type') === REL_HYPERLINK && r.getAttribute('Target') === url && r.getAttribute('TargetMode') === 'External');
  if (same) return same.getAttribute('Id');
  const ids = new Set(rels.map(r => r.getAttribute('Id')));
  let n = 1;
  while (ids.has(`rIdBabel${n}`)) n++;
  const el = doc.createElementNS(PKG_REL, 'Relationship');
  el.setAttribute('Id', `rIdBabel${n}`);
  el.setAttribute('Type', REL_HYPERLINK);
  el.setAttribute('Target', url);
  el.setAttribute('TargetMode', 'External');
  doc.documentElement.appendChild(el);
  return `rIdBabel${n}`;
}

// HYPERLINK "url" \l "anchor" \o "tip" …
function parseHyperlinkInstr(instr) {
  const m = /^\s*HYPERLINK\b(.*)$/is.exec(instr ?? '');
  if (!m) return null;
  const anchor = /\\l\s+"([^"]*)"/i.exec(m[1])?.[1];
  const rest = m[1].replace(/\\[a-z]\s+"[^"]*"/gi, '').replace(/\\[a-z]/gi, '');
  const url = /"([^"]*)"/.exec(rest)?.[1] ?? rest.trim().split(/\s+/)[0] ?? '';
  if (url) return { url: url + (anchor ? '#' + anchor : '') };
  return anchor ? { anchor } : null;
}

// The link a run belongs to: a w:hyperlink, or a HYPERLINK field (simple or
// complex). rels: id -> target for the run's part.
export function linkOfRun(run, rels) {
  const h = ancestor(run, a => isW(a, 'hyperlink'));
  if (h) {
    const id = h.getAttributeNS(R, 'id');
    const anchor = h.getAttributeNS(W, 'anchor');
    const target = id ? rels.get(id) : null;
    if (target) return { url: target + (anchor ? '#' + anchor : '') };
    return anchor ? { anchor } : null;
  }
  const fs = ancestor(run, a => isW(a, 'fldSimple'));
  if (fs) return parseHyperlinkInstr(fs.getAttributeNS(W, 'instr'));
  const p = ownerPara(run);
  if (!p) return null;
  const stack = [];
  for (const r of p.getElementsByTagNameNS(W, 'r')) {
    if (ownerPara(r) !== p) continue;
    if (r === run) break;
    const type = kids(r).find(k => isW(k, 'fldChar'))?.getAttributeNS(W, 'fldCharType');
    if (type === 'begin') stack.push({ instr: '', result: false });
    else if (type === 'separate') { if (stack.length) stack.at(-1).result = true; }
    else if (type === 'end') stack.pop();
    else {
      const it = kids(r).find(k => isW(k, 'instrText'));
      if (it && stack.length && !stack.at(-1).result) stack.at(-1).instr += it.textContent;
    }
  }
  for (let i = stack.length - 1; i >= 0; i--) {
    if (!stack[i].result) continue;
    const l = parseHyperlinkInstr(stack[i].instr);
    if (l) return l;
  }
  return null;
}

// The character formatting that makes a run look like a link, so a pasted
// link looks like the one it was copied from.
export function linkStyleOf(run) {
  const rPr = kids(run).find(k => isW(k, 'rPr'));
  if (!rPr) return [];
  return kids(rPr).filter(k => ['rStyle', 'color', 'u'].includes(k.localName) && k.namespaceURI === W).map(k => ({
    name: k.localName,
    attrs: Object.fromEntries([...k.attributes].filter(a => a.namespaceURI === W).map(a => [a.localName, a.value])),
  }));
}

export function hasCharStyle(stylesDoc, id) {
  return !!stylesDoc && [...stylesDoc.getElementsByTagNameNS(W, 'style')]
    .some(s => s.getAttributeNS(W, 'styleId') === id && s.getAttributeNS(W, 'type') === 'character');
}

const insideSkipped = (el, stop) => {
  for (let a = el.parentElement; a && a !== stop; a = a.parentElement) {
    if (isW(a, 'del') || isW(a, 'moveFrom') || (a.namespaceURI === MC && a.localName === 'Fallback')) return true;
  }
  return false;
};

// The paragraph's rendered runs with their offsets in the text the editor
// sees: text as is, a tab or line break as one character, everything else
// (fields, images, references) as nothing.
export function runOffsets(p) {
  const runs = [];
  let text = '';
  for (const r of p.getElementsByTagNameNS(W, 'r')) {
    if (ownerPara(r) !== p || insideSkipped(r, p)) continue;
    const c = kids(r).filter(k => !isW(k, 'rPr'));
    let piece = '', t = null;
    if (!r.hasAttribute('data-bk') && c.length === 1) {
      const k = c[0];
      if (isW(k, 't')) piece = t = k.textContent;
      else if (isW(k, 'tab')) piece = '\t';
      else if (isW(k, 'br') && (k.getAttributeNS(W, 'type') || 'textWrapping') === 'textWrapping') piece = '\n';
    }
    runs.push({ run: r, start: text.length, end: text.length + piece.length, text: t });
    text += piece;
  }
  return { runs, text };
}

const setRunText = (r, text) => {
  const t = kids(r).find(k => isW(k, 't'));
  t.textContent = text;
  t.setAttributeNS(XMLNS_XML, 'xml:space', 'preserve');
};

// Every run in the paragraph becomes first-class (an id, no generated mark),
// as it is about to be rendered afresh.
export function adoptRuns(p, key) {
  for (const r of p.getElementsByTagNameNS(W, 'r')) {
    if (ownerPara(r) !== p) continue;
    if (r.hasAttribute('data-bgen') || !r.getAttribute('data-br')) {
      r.removeAttribute('data-bgen');
      r.setAttribute('data-br', `${key}:k${crypto.randomUUID().slice(0, 8)}`);
    }
  }
}

// Makes [start, end) of the paragraph's text a hyperlink. link: {relId} or
// {anchor}. style: [{name, attrs}] run properties to give the linked text.
// Returns null on success, or a message saying why it can't be done.
export function linkRange(p, start, end, link, style) {
  if (end <= start) return 'Select the text to link first.';
  let { runs } = runOffsets(p);
  const splitAt = pos => {
    const e = runs.find(x => x.text != null && x.start < pos && pos < x.end);
    if (!e) return;
    const clone = e.run.cloneNode(true);
    clone.removeAttribute('data-br');
    clone.removeAttribute('data-bgen');
    setRunText(e.run, e.text.slice(0, pos - e.start));
    setRunText(clone, e.text.slice(pos - e.start));
    e.run.after(clone);
    runs = runOffsets(p).runs;
  };
  splitAt(end);
  splitAt(start);
  const covered = runs.filter(x => x.start >= start && x.end <= end && x.end > x.start);
  if (!covered.length) return 'There is no text in the selection to link.';
  const first = covered[0].run, last = covered.at(-1).run;
  const parent = first.parentNode;
  const crosses = 'The selection crosses a link, field or tracked change. Select text inside one of them.';
  if (last.parentNode !== parent) return crosses;

  let h;
  if (isW(parent, 'hyperlink')) {
    // Already a link: give it the new address.
    h = parent;
    h.removeAttributeNS(R, 'id');
    h.removeAttributeNS(W, 'anchor');
  } else {
    if (!(isW(parent, 'p') || isW(parent, 'ins') || isW(parent, 'smartTag') || isW(parent, 'sdtContent') || isW(parent, 'customXml'))) return crosses;
    const nodes = [];
    for (let n = first; n; n = n.nextSibling) { nodes.push(n); if (n === last) break; }
    if (nodes.some(n => n.nodeType === 1 && (isW(n, 'hyperlink') || isW(n, 'fldSimple')
      || (isW(n, 'r') && kids(n).some(k => isW(k, 'fldChar') || isW(k, 'instrText')))))) return crosses;
    h = p.ownerDocument.createElementNS(W, 'w:hyperlink');
    first.before(h);
    for (const n of nodes) h.appendChild(n);
  }
  if (link.relId) {
    h.setAttributeNS(R, 'r:id', link.relId);
    h.setAttributeNS(W, 'w:history', '1');
  } else {
    h.setAttributeNS(W, 'w:anchor', link.anchor);
  }
  for (const r of h.getElementsByTagNameNS(W, 'r')) {
    if (!kids(r).some(k => isW(k, 't'))) continue;
    let rPr = kids(r).find(k => isW(k, 'rPr'));
    if (!rPr) { rPr = p.ownerDocument.createElementNS(W, 'w:rPr'); r.insertBefore(rPr, r.firstChild); }
    for (const { name, attrs } of style) setRPrChild(rPr, name, attrs);
  }
  return null;
}

// A hyperlink whose relationship is missing makes Word refuse the file;
// keep the text and drop the link instead.
function dropOrphanLinks(doc, rels) {
  for (const h of [...doc.getElementsByTagNameNS(W, 'hyperlink')]) {
    const id = h.getAttributeNS(R, 'id');
    if (id && !rels.has(id)) {
      while (h.firstChild) h.before(h.firstChild);
      h.remove();
    }
  }
}

// ── Export ─────────────────────────────────────────────────────────────────

const strip = doc => {
  for (const el of [...doc.querySelectorAll('[data-bp],[data-br],[data-bk],[data-bgen]')]) {
    el.removeAttribute('data-bp'); el.removeAttribute('data-br');
    el.removeAttribute('data-bk'); el.removeAttribute('data-bgen');
  }
  // Proofing marks describe the source language; Word recomputes them.
  for (const el of [...doc.getElementsByTagNameNS(W, 'proofErr')]) el.remove();
};

const textOnly = r => {
  const c = kids(r);
  const rPr = c.find(k => isW(k, 'rPr'));
  const rest = c.filter(k => k !== rPr);
  return rest.length === 1 && isW(rest[0], 't') ? { rPr, t: rest[0] } : null;
};

// Joins neighbouring runs that differ in nothing but rsids — keeps the
// exported XML close to what Word itself writes.
export function mergeRuns(doc) {
  const ser = new XMLSerializer();
  for (const p of [...doc.getElementsByTagNameNS(W, 'p')]) {
    const parents = new Set([...p.getElementsByTagNameNS(W, 'r')].map(r => r.parentNode));
    for (const parent of parents) {
      let prev = null, prevKey = null;
      for (const r of kids(parent)) {
        const a = isW(r, 'r') && textOnly(r);
        const k = a ? (a.rPr ? ser.serializeToString(a.rPr) : '') : null;
        if (a && prev && k === prevKey) {
          prev.t.textContent += a.t.textContent;
          r.remove();
          continue;
        }
        prev = a || null; prevKey = k;
      }
      for (const r of kids(parent)) {
        const a = isW(r, 'r') && textOnly(r);
        if (a && /^\s|\s$/.test(a.t.textContent)) a.t.setAttributeNS(XMLNS_XML, 'xml:space', 'preserve');
      }
    }
  }
}

// Sets the proofing language everywhere a language is declared, and declares
// it on the document defaults, so Word checks the translation in its language.
export function setLanguage(doc, lang) {
  for (const l of doc.getElementsByTagNameNS(W, 'lang')) {
    if (l.parentElement && isW(l.parentElement, 'rPr')) l.setAttributeNS(W, 'w:val', lang);
  }
  const defaults = doc.getElementsByTagNameNS(W, 'rPrDefault')[0];
  if (defaults) {
    let rPr = kids(defaults).find(k => isW(k, 'rPr'));
    if (!rPr) { rPr = doc.createElementNS(W, 'w:rPr'); defaults.appendChild(rPr); }
    const l = kids(rPr).find(k => isW(k, 'lang'));
    if (l) l.setAttributeNS(W, 'w:val', lang);
    else setRPrChild(rPr, 'lang', { val: lang });
  }
}

// Builds the translated .docx: original package, translated text parts,
// language set to the target, everything else untouched.
export async function buildDocx({ bytes, partXml, lang }) {
  const zip = await JSZip.loadAsync(bytes);
  const pkg = await openPackage(bytes);
  const out = {};
  for (const [path, xml] of Object.entries(partXml)) {
    if (!pkg.textParts.includes(path)) out[path] = xml; // relationship parts
  }
  for (const path of pkg.textParts) {
    const original = await zip.file(path).async('string');
    const doc = parseXml(partXml[path] ?? original);
    const relsXml = partXml[relsPathFor(path)] ?? await zip.file(relsPathFor(path))?.async('string');
    dropOrphanLinks(doc, relsMap(relsXml ? parseXml(relsXml) : null));
    strip(doc);
    mergeRuns(doc);
    if (lang) setLanguage(doc, lang);
    out[path] = serializeXml(doc, original);
  }
  if (lang && pkg.stylesPath) {
    const original = await zip.file(pkg.stylesPath).async('string');
    const doc = parseXml(original);
    setLanguage(doc, lang);
    out[pkg.stylesPath] = serializeXml(doc, original);
  }
  if (pkg.settingsPath) {
    const original = await zip.file(pkg.settingsPath).async('string');
    const doc = parseXml(original);
    for (const el of [...doc.getElementsByTagNameNS(W, 'proofState')]) el.remove();
    out[pkg.settingsPath] = serializeXml(doc, original);
  }
  for (const [path, xml] of Object.entries(out)) zip.file(path, xml, { createFolders: false });
  return zip.generateAsync({
    type: 'blob',
    compression: 'DEFLATE',
    compressionOptions: { level: 6 },
    mimeType: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  });
}

// A renderable package: the original with the given parts swapped in.
export async function buildRenderable(bytes, partXml) {
  const zip = await JSZip.loadAsync(bytes);
  for (const [path, xml] of Object.entries(partXml)) zip.file(path, xml, { createFolders: false });
  return zip.generateAsync({ type: 'arraybuffer', compression: 'STORE' });
}
