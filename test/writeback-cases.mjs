// Generates write-back cases for the Rust port (crates/babel-docx): every
// paragraph of the fixtures, edited several ways, through the real
// web/js/ooxml.js applyParagraph in a browser.
//   (cd web && python3 -m http.server 8790) &
//   node test/writeback-cases.mjs > crates/babel-docx/tests/cases.json

import { createRequire } from 'module';
import fs from 'fs';
import path from 'path';

const require = createRequire(process.env.PLAYWRIGHT_DIR ?? '/home/jesper/cprsvp/');
const { chromium } = require('playwright');
const BASE = process.env.BASE ?? 'http://localhost:8790/';
const here = path.dirname(new URL(import.meta.url).pathname);
const fixtures = ['sample.docx', 'long.docx'].map(f => [f, fs.readFileSync(path.join(here, 'fixtures', f)).toString('base64')]);

const browser = await chromium.launch({ executablePath: process.env.CHROME ?? '/usr/bin/google-chrome' });
const page = await browser.newPage();
await page.goto(BASE);
const cases = await page.evaluate(async fixtures => {
  const X = await import('./js/ooxml.js');
  const W = X.W;
  const out = [];
  const ser = el => new XMLSerializer().serializeToString(el);
  const isW = (el, n) => el.namespaceURI === W && el.localName === n;
  const ownerPara = el => { for (let a = el.parentElement; a; a = a.parentElement) if (isW(a, 'p')) return a; return null; };
  const skipped = el => { for (let a = el.parentElement; a; a = a.parentElement) if (isW(a, 'del') || isW(a, 'moveFrom') || (a.localName === 'Fallback')) return true; return false; };

  for (const [name, b64] of fixtures) {
    const bytes = Uint8Array.from(atob(b64), c => c.charCodeAt(0));
    const pkg = await X.openPackage(bytes.buffer);
    for (const partPath of pkg.textParts) {
      const xml = await pkg.zip.file(partPath).async('string');
      const doc = X.normalize(X.parseXml(xml), X.partKey(partPath));
      for (const p of X.paragraphs(doc)) {
        // The paragraph's own runs, as the editor would render them.
        const runs = [...p.getElementsByTagNameNS(W, 'r')].filter(r => ownerPara(r) === p && !skipped(r)).map(r => {
          const content = [...r.children].filter(k => !isW(k, 'rPr'));
          const only = content.length === 1 ? content[0] : null;
          const kind = only && isW(only, 't') && !r.hasAttribute('data-bk') ? 't'
            : only && (isW(only, 'tab') || (isW(only, 'br') && !only.getAttribute('w:type'))) ? 'atom'
              : 'other';
          return { rid: r.getAttribute('data-br'), kind, text: only && isW(only, 't') ? only.textContent : '' };
        });
        const textRuns = runs.filter(r => r.kind === 't').map(r => r.rid);
        const atoms = runs.filter(r => r.kind === 'atom').map(r => r.rid);
        // Visual-order tokens for the unchanged paragraph.
        const asIs = runs.map(r => r.kind === 't' ? { t: 'text', rid: r.rid, base: r.rid, text: r.text, fmt: null } : { t: 'atom', rid: r.rid });
        const lastText = [...runs].reverse().find(r => r.kind === 't')?.rid ?? null;
        const variants = {
          unchanged: asIs,
          translated: asIs.map(t => t.t === 'text' ? { ...t, text: t.text.split('').reverse().join('') + 'å' } : t),
          emptied: asIs.filter(t => t.t !== 'text'),
          firstOnly: (() => { let seen = false; return asIs.filter(t => t.t !== 'text' || (!seen && (seen = true))).map(t => t.t === 'text' ? { ...t, text: 'Bara detta' } : t); })(),
          noAtoms: asIs.filter(t => t.t !== 'atom' || !atoms.includes(t.rid)),
          typedAround: [
            { t: 'text', rid: null, base: null, text: 'Före ', fmt: null },
            ...asIs.flatMap(t => t.t === 'atom' ? [t, { t: 'text', rid: null, base: lastText, text: ' efter', fmt: null }] : [t]),
            { t: 'br', base: lastText },
            { t: 'text', rid: null, base: lastText, text: 'Ny rad', fmt: null },
            { t: 'text', rid: null, base: lastText, text: ' fet', fmt: { b: true } },
            { t: 'text', rid: null, base: lastText, text: ' kursiv', fmt: { i: true } },
          ],
          boldInRun: asIs.flatMap(t => t.t === 'text' ? [t, { t: 'text', rid: t.rid, base: t.rid, text: '!', fmt: { b: true, u: false } }] : [t]),
        };
        for (const [variant, tokens] of Object.entries(variants)) {
          const copy = p.cloneNode(true);
          const before = ser(copy);
          const info = { textRuns: new Set(textRuns), atoms: new Set(atoms) };
          X.applyParagraph(copy, tokens, info, new Map());
          out.push({ file: name, part: partPath, pid: p.getAttribute('data-bp'), variant, before, tokens, textRuns, atoms, after: ser(copy) });
        }
      }
    }
  }
  return out;
}, fixtures);
await browser.close();
process.stdout.write(JSON.stringify(cases));
console.error(`${cases.length} cases`);
