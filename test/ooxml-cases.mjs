// Cases for the Rust port of the rest of web/js/ooxml.js (crates/babel-docx
// src/ooxml.rs): normalising, ids after edits, paragraph text, links. Each
// output comes from the JavaScript itself.
//   (cd web && python3 -m http.server 8790) &
//   node test/ooxml-cases.mjs > crates/babel-docx/tests/ooxml-cases.json

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
  const ser = el => new XMLSerializer().serializeToString(el);
  const out = { normalize: [], ids: [], text: [], links: [], linkRange: [] };
  for (const [name, b64] of fixtures) {
    const pkg = await X.openPackage(Uint8Array.from(atob(b64), c => c.charCodeAt(0)).buffer);
    for (const partPath of pkg.textParts) {
      const key = X.partKey(partPath);
      const orig = await pkg.zip.file(partPath).async('string');
      const doc = X.normalize(X.parseXml(orig), key);
      out.normalize.push({ file: name, part: partPath, key, input: orig, output: X.serializeXml(doc, orig) });
      const relsXml = await pkg.zip.file(X.relsPathFor(partPath))?.async('string') ?? null;
      const rels = X.relsMap(relsXml ? X.parseXml(relsXml) : null);
      for (const p of X.paragraphs(doc)) {
        out.text.push({ xml: ser(p), text: X.paraText(p) });
        for (const r of p.getElementsByTagNameNS(W, 'r')) {
          const l = X.linkOfRun(r, rels);
          if (l) out.links.push({ xml: ser(p), rid: r.getAttribute('data-br'), rels: Object.fromEntries(rels), link: l });
        }
        // Links over a few ranges of the paragraph's run text.
        const { text } = X.runOffsets(p);
        if (!text.length) continue;
        const ranges = [[0, Math.min(4, text.length)], [Math.floor(text.length / 3), Math.floor(2 * text.length / 3)], [0, text.length], [2, 2]];
        for (const [i, [s, e]] of ranges.entries()) {
          const copy = p.cloneNode(true);
          const target = i % 2 ? { anchor: 'kapitel_2' } : { relId: 'rIdBabel1' };
          const style = i === 2 ? [{ name: 'color', attrs: { val: '0563C1' } }, { name: 'u', attrs: { val: 'single' } }] : [{ name: 'rStyle', attrs: { val: 'Hyperlink' } }];
          const before = ser(copy);
          const problem = X.linkRange(copy, s, e, target, style);
          out.linkRange.push({ xml: before, start: s, end: e, target, style, problem, after: problem ? null : ser(copy) });
        }
      }
      // Ids after an edit session: Babel's generated runs become first-class runs.
      const edited = X.parseXml(X.serializeXml(doc, orig));
      for (const p of X.paragraphs(edited).slice(0, 6)) {
        const runs = [...p.getElementsByTagNameNS(W, 'r')].filter(r => r.querySelector('t'));
        const last = runs.at(-1)?.getAttribute('data-br') ?? null;
        const tokens = [{ t: 'text', rid: null, base: null, text: 'Ny ', fmt: null },
          ...runs.map(r => ({ t: 'text', rid: r.getAttribute('data-br'), base: r.getAttribute('data-br'), text: r.textContent, fmt: null })),
          { t: 'text', rid: null, base: last, text: ' slut', fmt: { b: true } }];
        X.applyParagraph(p, tokens, { textRuns: new Set(runs.map(r => r.getAttribute('data-br'))), atoms: new Set() }, new Map());
      }
      const input = X.serializeXml(edited, orig);
      X.ensureIds(edited, key);
      out.ids.push({ key, input, output: X.serializeXml(edited, orig) });
    }
  }
  return out;
}, fixtures);
await browser.close();
process.stdout.write(JSON.stringify(cases));
console.error(Object.entries(cases).map(([k, v]) => `${k}: ${v.length}`).join(', '));
