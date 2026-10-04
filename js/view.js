// Rendering, pagination, zoom and page-overflow detection for both panels.

import { renderAsync, babelRefreshTab } from '../vendor/docx-preview.mjs';

export async function render(bytes, bodyEl, styleEl, className) {
  await renderAsync(bytes, bodyEl, styleEl, {
    className,
    inWrapper: true,
    breakPages: true,
    ignoreLastRenderedPageBreak: false, // Word's own pagination, as last saved
    experimental: true,                 // real tab stops
    renderHeaders: true,
    renderFooters: true,
    renderFootnotes: true,
    renderEndnotes: true,
    renderChanges: false,
    renderComments: false,
    useBase64URL: false,
  });
  // docx-preview computes tab widths half a second after rendering.
  await new Promise(r => setTimeout(r, 600));
  return pages(bodyEl);
}

export const pages = root => [...root.querySelectorAll(':scope > div > section')];

export function refreshTabs(el) {
  for (const t of el.querySelectorAll('[data-k="tab"] > span')) babelRefreshTab(t);
}

// ── Pagination ─────────────────────────────────────────────────────────────
// docx-preview breaks pages only where Word recorded a break. A document that
// was never paginated by Word (generated files, Google Docs exports) or whose
// fonts measure differently would otherwise show one endless page. Pages that
// overflow are split at block level — whole paragraphs, or between table rows.
// The source decides where; the target gets the identical breaks so that any
// difference in page height is the translation's doing.

const pageHeight = sec => parseFloat(getComputedStyle(sec).minHeight) || sec.offsetHeight;

const outer = el => {
  if (!el) return 0;
  const cs = getComputedStyle(el);
  return el.offsetHeight + parseFloat(cs.marginTop) + parseFloat(cs.marginBottom);
};

const blocksOf = sec => [...sec.querySelectorAll(':scope > article')].flatMap(a => [...a.children]);
const scaleOf = sec => (sec.getBoundingClientRect().height / sec.offsetHeight) || 1;
const isHeading = el => el?.tagName === 'P' && /heading|title|rubrik/i.test(el.className) && el.textContent.length < 200;

function findSplit(sec) {
  const pageH = pageHeight(sec);
  if (sec.offsetHeight <= pageH + 1) return null;
  const cs = getComputedStyle(sec);
  const footer = sec.querySelector(':scope > footer');
  const notes = sec.querySelector(':scope > ol');
  const avail = pageH - parseFloat(cs.paddingBottom) - Math.max(0, outer(footer)) - outer(notes);
  const s = scaleOf(sec);
  const top = sec.getBoundingClientRect().top;
  const bottom = el => (el.getBoundingClientRect().bottom - top) / s;
  const blocks = blocksOf(sec);
  let i = blocks.findIndex(b => bottom(b) > avail + 0.5);
  if (i < 0) return null;
  const b = blocks[i];
  if (b.tagName === 'TABLE' && b.rows.length > 1) {
    const r = [...b.rows].findIndex(row => bottom(row) > avail + 0.5);
    if (r >= 1) return { block: i, row: r }; // break between rows, like Word
  }
  if (i > 0 && isHeading(blocks[i - 1])) i--; // keep headings with what follows
  if (i > 0) return { block: i, row: null };
  const t = blocks[0];
  if (t.tagName === 'TABLE' && t.rows.length > 1) {
    let r = [...t.rows].findIndex(row => bottom(row) > avail + 0.5);
    if (r < 1) r = 1;
    return { block: 0, row: r };
  }
  return null; // a single block taller than the page: leave it
}

function splitPage(sec, { block, row }) {
  const page = sec.cloneNode(false);
  page.dataset.continued = '1';
  const header = sec.querySelector(':scope > header');
  const footer = sec.querySelector(':scope > footer');
  if (header) page.appendChild(header.cloneNode(true));
  const articles = [...sec.querySelectorAll(':scope > article')];
  let n = 0;
  let moving = false;
  for (const a of articles) {
    if (moving) { page.appendChild(a); continue; }
    const children = [...a.children];
    if (block < n + children.length) {
      const fresh = a.cloneNode(false);
      const first = children[block - n];
      if (row != null) {
        const t2 = first.cloneNode(false);
        const cols = first.querySelector(':scope > colgroup');
        if (cols) t2.appendChild(cols.cloneNode(true));
        for (const r of [...first.rows].slice(row)) t2.appendChild(r);
        fresh.appendChild(t2);
        for (const c of children.slice(block - n + 1)) fresh.appendChild(c);
      } else {
        for (const c of children.slice(block - n)) fresh.appendChild(c);
      }
      page.appendChild(fresh);
      moving = true;
    }
    n += children.length;
  }
  if (footer) page.appendChild(footer.cloneNode(true));
  sec.after(page);
  return page;
}

// Splits overflowing pages; returns the splits so they can be replayed.
export function paginate(root) {
  const splits = [];
  for (let i = 0; i < pages(root).length && splits.length < 5000; i++) {
    const sec = pages(root)[i];
    const at = findSplit(sec);
    if (!at) continue;
    splitPage(sec, at);
    splits.push({ page: i, ...at });
  }
  return splits;
}

export function replaySplits(root, splits) {
  for (const s of splits) {
    const sec = pages(root)[s.page];
    if (sec) splitPage(sec, s);
  }
}

// Page-number fields show their cached value from the last save; show the
// real numbers instead (display only, the XML is left alone).
export function numberPages(root) {
  const all = pages(root);
  all.forEach((sec, i) => {
    for (const s of sec.querySelectorAll('[data-k="page"]')) s.textContent = String(i + 1);
    for (const s of sec.querySelectorAll('[data-k="numpages"]')) s.textContent = String(all.length);
  });
}

// ── Overflow ───────────────────────────────────────────────────────────────
// How much taller the target page is than the source page it mirrors.
export function overflowOf(srcSec, tgtSec) {
  if (!srcSec || !tgtSec) return 0;
  const ref = Math.max(pageHeight(srcSec), srcSec.offsetHeight);
  return tgtSec.offsetHeight - ref;
}

export function lineHeightOf(sec) {
  const p = sec.querySelector('article p');
  if (!p) return 16;
  const cs = getComputedStyle(p);
  const lh = parseFloat(cs.lineHeight);
  return Number.isFinite(lh) ? lh : parseFloat(cs.fontSize) * 1.2;
}

// ── Zoom ───────────────────────────────────────────────────────────────────
// transform: scale keeps layout (offsetHeight etc.) in document pixels, which
// the paginator and overflow checks rely on; the sizer gives the scroll area
// the scaled dimensions.

export class Zoom {
  constructor(sizer, scaler) {
    this.sizer = sizer;
    this.scaler = scaler;
    this.z = 1;
    new ResizeObserver(() => this.fit()).observe(scaler);
  }
  set(z) {
    this.z = z;
    this.scaler.style.transform = `scale(${z})`;
    this.fit();
  }
  fit() {
    this.sizer.style.width = `${this.scaler.offsetWidth * this.z}px`;
    this.sizer.style.height = `${this.scaler.offsetHeight * this.z}px`;
  }
}

export function pageWidth(root) {
  const sec = pages(root)[0];
  return sec ? sec.offsetWidth : 794;
}
