// Cases for the Rust port of openPackage and buildDocx (crates/babel-docx
// src/package.rs): each fixture imported, edited (text, formatting, line
// breaks, a link), and exported by the JavaScript.
//   (cd web && python3 -m http.server 8790) &
//   node test/export-cases.mjs > crates/babel-docx/tests/export-cases.json

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
  for (const [name, b64] of fixtures) {
    const bytes = Uint8Array.from(atob(b64), c => c.charCodeAt(0)).buffer;
    const pkg = await X.openPackage(bytes);
    const parts = {};
    for (const p of pkg.textParts) {
      const orig = await pkg.zip.file(p).async('string');
      parts[p] = X.serializeXml(X.normalize(X.parseXml(orig), X.partKey(p)), orig);
    }
    const imported = { ...parts };
    // Edits, as the editor would make them.
    for (const p of pkg.textParts) {
      const doc = X.parseXml(parts[p]);
      let k = 0;
      for (const para of X.paragraphs(doc)) {
        const runs = [...para.getElementsByTagNameNS(W, 'r')].filter(r => r.querySelector('t'));
        if (!runs.length || k++ % 3) continue;
        const last = runs.at(-1).getAttribute('data-br');
        const tokens = [{ t: 'text', rid: null, base: null, text: 'Översatt ', fmt: null },
          ...runs.map(r => ({ t: 'text', rid: r.getAttribute('data-br'), base: r.getAttribute('data-br'), text: r.textContent, fmt: null })),
          { t: 'br', base: last }, { t: 'text', rid: null, base: last, text: ' fet', fmt: { b: true } }];
        X.applyParagraph(para, tokens, { textRuns: new Set(runs.map(r => r.getAttribute('data-br'))), atoms: new Set() }, new Map());
      }
      parts[p] = X.serializeXml(doc, parts[p]);
    }
    // A link on the main part's first paragraph with text, with a new relationship.
    const main = X.parseXml(parts[pkg.mainPath]);
    const relsPath = X.relsPathFor(pkg.mainPath);
    const relsXml = await pkg.zip.file(relsPath)?.async('string');
    const rels = relsXml ? X.parseXml(relsXml) : X.emptyRels();
    const id = X.addLinkRel(rels, 'https://babel.example/länk');
    const first = X.paragraphs(main).find(p => X.runOffsets(p).text.length > 6);
    X.linkRange(first, 0, 5, { relId: id }, [{ name: 'color', attrs: { val: '0563C1' } }]);
    parts[pkg.mainPath] = X.serializeXml(main, parts[pkg.mainPath]);
    parts[relsPath] = X.serializeXml(rels, relsXml);
    const blob = await X.buildDocx({ bytes, partXml: parts, lang: 'sv-SE' });
    const z = await JSZip.loadAsync(await blob.arrayBuffer());
    const output = {};
    for (const p of [...pkg.textParts, pkg.stylesPath, pkg.settingsPath, relsPath].filter(Boolean)) output[p] = await z.file(p).async('string');
    out.push({ file: name, textParts: pkg.textParts, imported, parts, lang: 'sv-SE', output, entries: Object.keys(z.files).filter(n => !z.files[n].dir).sort() });
  }
  return out;
}, fixtures);
await browser.close();
process.stdout.write(JSON.stringify(cases));
console.error(`${cases.length} cases`);
