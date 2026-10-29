// Writes a fixture's text parts as Babel stores them (normalised by
// web/js/ooxml.js: one content element per run, data-bp/data-br ids), for
// the Rust editing tests.
//   (cd web && python3 -m http.server 8790) &
//   node test/normalized-parts.mjs sample.docx > crates/babel-pages/tests/sample-parts.json

import { createRequire } from 'module';
import fs from 'fs';
import path from 'path';

const require = createRequire(process.env.PLAYWRIGHT_DIR ?? '/home/jesper/cprsvp/');
const { chromium } = require('playwright');
const BASE = process.env.BASE ?? 'http://localhost:8790/';
const here = path.dirname(new URL(import.meta.url).pathname);
const b64 = fs.readFileSync(path.join(here, 'fixtures', process.argv[2] ?? 'sample.docx')).toString('base64');

const browser = await chromium.launch({ executablePath: process.env.CHROME ?? '/usr/bin/google-chrome' });
const page = await browser.newPage();
await page.goto(BASE);
const parts = await page.evaluate(async b64 => {
  const X = await import('./js/ooxml.js');
  const pkg = await X.openPackage(Uint8Array.from(atob(b64), c => c.charCodeAt(0)).buffer);
  const out = {};
  for (const p of pkg.textParts) {
    const orig = await pkg.zip.file(p).async('string');
    out[p] = X.serializeXml(X.normalize(X.parseXml(orig), X.partKey(p)), orig);
  }
  return out;
}, b64);
await browser.close();
process.stdout.write(JSON.stringify(parts));
