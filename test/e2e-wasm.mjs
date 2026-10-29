// The WebAssembly editor (web/js/wasm-view.js over crates/babel-wasm) in a
// real browser, on the static site.
//   make wasm && (cd web && python3 -m http.server 8790) &
//   node test/e2e-wasm.mjs
// Same Playwright and Chrome settings as e2e.mjs; against babel-server set
// BASE and LOGIN=user:password.

import { createRequire } from 'module';
import { execFileSync } from 'child_process';
import fs from 'fs';
import path from 'path';

const require = createRequire(process.env.PLAYWRIGHT_DIR ?? '/home/jesper/cprsvp/');
const { chromium } = require('playwright');
const BASE = process.env.BASE ?? 'http://localhost:8790/';
const OUT = process.env.OUT ?? '/tmp';
const fixture = path.resolve(path.dirname(new URL(import.meta.url).pathname), 'fixtures/sample.docx');

let failures = 0;
const check = (cond, msg) => { console.log(`${cond ? 'ok  ' : 'FAIL'} ${msg}`); if (!cond) failures++; };

const browser = await chromium.launch({ executablePath: process.env.CHROME ?? '/usr/bin/google-chrome' });
const ctx = await browser.newContext({ viewport: { width: 1500, height: 950 }, acceptDownloads: true });
await ctx.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: new URL(BASE).origin });
const page = await ctx.newPage();
const errors = [];
page.on('pageerror', e => errors.push(e.message));
page.on('console', m => { if (m.type() === 'error' && !/favicon|404/.test(m.text())) errors.push(m.text()); });

await page.goto(BASE);
await page.evaluate(() => { localStorage.clear(); indexedDB.deleteDatabase('babel'); });
await page.reload();
if (process.env.LOGIN) {
  const [user, pass] = process.env.LOGIN.split(/:(.*)/);
  await page.waitForSelector('#loginForm', { state: 'visible' });
  await page.fill('#loginUser', user);
  await page.fill('#loginPass', pass);
  await page.click('#loginForm button[type=submit]');
  await page.waitForSelector('#home', { state: 'visible' });
}
await page.setInputFiles('#fileInput', fixture);
await page.waitForSelector('#langDlg[open]');
await page.fill('#langSearch', 'swed');
await page.keyboard.press('Enter');
await page.waitForFunction(() => document.querySelector('#loading').hidden && window.__babel?.P?.view, null, { timeout: 60000 });

const view = () => page.evaluate(() => !!__babel.P.view);
check(await view(), 'Babel’s layout engine is the editor by default');
check(await page.evaluate(() => [...document.querySelectorAll('#tgtDoc .w-canvas')].some(c => c.width > 100)), 'pages are drawn on canvases');
check(/5,534 characters with spaces/.test(await page.textContent('#srcStatus')), 'the original counts 5,534 characters, as the classic editor does');

// Client coordinates of paragraph `pid` at byte offset `off`.
const where = (pid, off) => page.evaluate(([pid, off]) => {
  const v = __babel.P.view;
  const c = JSON.parse(v.editor.caretAt(pid, off));
  const box = v.tgt[c.page].wrap.getBoundingClientRect();
  const z = 96 / 72 * v.zoom;
  return { x: box.left + c.x * z + 0.5, y: box.top + (c.top + c.height / 2) * z };
}, [pid, off]);
// Offsets are UTF-8 bytes (the editor's), not JavaScript string lengths.
const bytes = s => new TextEncoder().encode(s).length;
const clickAt = async (pid, off) => {
  const p = await where(pid, off);
  await page.mouse.click(p.x, p.y);
};
const partXml = () => page.evaluate(() => { __babel.P.view.flush(); return new XMLSerializer().serializeToString(__babel.P.tgt['word/document.xml']); });
const paraXml = async pid => {
  const x = await partXml();
  const i = x.indexOf(`data-bp="${pid}"`);
  const start = x.lastIndexOf('<w:p ', i);
  return x.slice(start, x.indexOf('</w:p>', i) + 6);
};

// Typing at the start of a paragraph goes into its first run.
await clickAt('document:2', 0);
await page.keyboard.type('Denna rapport är ');
let xml = await paraXml('document:2');
check(xml.includes('>Denna rapport är This report is </w:t>'), 'typed text is in the first run');
const journal = await page.evaluate(() => JSON.parse(localStorage.getItem(`babel.journal.${__babel.P.project.id}`) ?? '{}'));
check(Object.values(journal.entries ?? {}).some(e => e.xml.includes('Denna rapport')), 'the edit is in the synchronous journal at once');

// Shift+Left ×5 selects "This " back from …; Ctrl+B makes it bold.
await page.keyboard.press('End');
await clickAt('document:2', bytes('Denna rapport är This report'));
for (let i = 0; i < 6; i++) await page.keyboard.press('Shift+ArrowLeft');
await page.keyboard.press('Control+b');
xml = await paraXml('document:2');
check(/<w:b\/><w:bCs\/><\/w:rPr><w:t xml:space="preserve">report<\/w:t>/.test(xml), 'Shift+Left selection made bold with Ctrl+B');
await page.keyboard.press('ArrowRight');

// Shift+Enter: a line break.
await page.keyboard.press('Shift+Enter');
await page.keyboard.type('Ny rad');
xml = await paraXml('document:2');
check(/<w:br\/>(?:(?!<w:t).)*<w:t[^>]*>Ny rad/.test(xml), 'Shift+Enter inserted a w:br before the new text');

// Undo takes the typing back.
await page.keyboard.press('Control+z');
xml = await paraXml('document:2');
check(!xml.includes('Ny rad'), 'Ctrl+Z undoes the typing');

// Ctrl+Enter marks done and moves on.
await page.keyboard.press('Control+Enter');
check(await page.evaluate(() => { __babel.P.view.flush(); return __babel.P.confirmed.has('document:2'); }), 'Ctrl+Enter marks the paragraph done');
check(await page.evaluate(() => document.querySelectorAll('#tgtDoc .w-mark.done').length > 0), 'done paragraphs get a green mark');

// Copy link in the original, Ctrl+K on a selection in the translation.
const seeAlso = await page.evaluate(() => {
  const v = __babel.P.view;
  for (let i = 0; i < 40; i++) {
    const c = JSON.parse(v.editor.caretAt(`document:${i}`, 0));
    if (c && v.editor.state && JSON.stringify(c)) {
      const t = __babel.P.tgt['word/document.xml'].querySelector(`[data-bp="document:${i}"]`)?.textContent ?? '';
      if (t.startsWith('See also')) return `document:${i}`;
    }
  }
  return null;
});
check(!!seeAlso, `found the paragraph with the link (${seeAlso})`);
{
  const p = await where(seeAlso, 'See also the pro'.length);
  const dx = await page.evaluate(() => { const v = __babel.P.view; return v.src[0].wrap.getBoundingClientRect().left - v.tgt[0].wrap.getBoundingClientRect().left; });
  await page.mouse.click(p.x + dx, p.y, { button: 'right' });
  const items = await page.$$eval('.menu.ctx:not([hidden]) .menu-item', els => els.map(e => e.textContent.replace(/\s+/g, ' ').trim()));
  check(items.some(t => t.startsWith('Copy link')), `the original's menu offers Copy link (${items.join(' | ')})`);
  await page.click('.menu.ctx:not([hidden]) .menu-item:has-text("Copy link")');
  check((await page.evaluate(() => navigator.clipboard.readText())) === 'https://example.org/', 'the link is on the clipboard');
}
await clickAt('document:4', 0);
for (let i = 0; i < 4; i++) await page.keyboard.press('Shift+ArrowRight');
await page.keyboard.press('Control+k');
await page.waitForFunction(() => { __babel.P.view.flush(); return /<w:hyperlink[^>]*>(?:(?!<\/w:hyperlink>).)*>Item</.test(new XMLSerializer().serializeToString(__babel.P.tgt['word/document.xml'])); }, null, { timeout: 5000 }).catch(() => {});
xml = await paraXml('document:4');
const rid = /<w:hyperlink r:id="([^"]+)"/.exec(xml)?.[1];
check(!!rid && /<w:hyperlink[^>]*>(?:(?!<\/w:hyperlink>).)*>Item</.test(xml), `Ctrl+K wrapped "Item" in a w:hyperlink (${rid})`);
// The address already had a relationship in this file (the original's link): it is reused, as on the classic editor.
const relsXml = await page.evaluate(async () => { __babel.P.view.flush(); const r = __babel.P.rels['word/_rels/document.xml.rels']; if (r) return new XMLSerializer().serializeToString(r); const z = await JSZip.loadAsync(__babel.P.source.bytes); return z.file('word/_rels/document.xml.rels').async('string'); });
check(new RegExp(`Id="${rid}"[^>]*Target="https://example.org/"|Target="https://example.org/"[^>]*Id="${rid}"`).test(relsXml), 'its relationship points to the copied address');

// Revert puts the original paragraph back.
await clickAt('document:2', 3);
await page.click('#tgtDoc', { button: 'right', position: await page.evaluate(async () => { const v = __babel.P.view; const c = JSON.parse(v.editor.caretAt('document:2', 3)); const b = v.tgt[c.page].wrap.getBoundingClientRect(); const d = document.querySelector('#tgtDoc').getBoundingClientRect(); const z = 96 / 72 * v.zoom; return { x: b.left - d.left + c.x * z, y: b.top - d.top + (c.top + 3) * z }; }) });
await page.click('.menu.ctx:not([hidden]) .menu-item:has-text("Revert paragraph")');
xml = await paraXml('document:2');
check(xml.includes('>This report is </w:t>') && !xml.includes('Denna'), 'Revert puts the original paragraph back');
await page.keyboard.press('Control+z');
check((await paraXml('document:2')).includes('Denna'), 'and Ctrl+Z brings the translation back');

// Spelling: a typo gets underlined (needs LanguageTool on the network).
await clickAt('document:12', 0);
await page.keyboard.type('Ett felstavvat ord. ');
const underlined = await page.waitForFunction(() => document.querySelectorAll('#tgtDoc .w-und.spelling').length > 0, null, { timeout: 25000 }).then(() => true, () => false);
check(underlined, 'a misspelling is underlined');

// Saved: reload and the edits are there.
await page.keyboard.press('Control+s');
await page.waitForFunction(() => document.querySelector('#saveState').dataset.state === 'saved', null, { timeout: 15000 });
await page.reload();
await page.waitForFunction(() => document.querySelector('#loading').hidden && window.__babel?.P?.view, null, { timeout: 60000 });
xml = await paraXml('document:2');
check(xml.includes('Denna rapport') && xml.includes('<w:b/><w:bCs/>'), 'edits and formatting restored after reload');
check(/<w:hyperlink[^>]*>(?:(?!<\/w:hyperlink>).)*>Item</.test(await paraXml('document:4')), 'the pasted link restored after reload');
check(await page.evaluate(() => __babel.P.confirmed.has('document:2')), 'the done mark restored after reload');

// Typing in a run made in the last session still works (ids given on opening).
await clickAt('document:2', bytes('Denna rapport är This ') + 2);
await page.keyboard.type('X');
check((await paraXml('document:2')).includes('reXport'), 'text made in an earlier session can be edited');

// Download: a valid .docx with the translation in it.
const [dl] = await Promise.all([page.waitForEvent('download'), page.click('#btnDownload')]);
const file = path.join(OUT, 'wasm-' + dl.suggestedFilename());
await dl.saveAs(file);
const docXml = execFileSync('unzip', ['-p', file, 'word/document.xml']).toString();
const stylesXml = execFileSync('unzip', ['-p', file, 'word/styles.xml']).toString();
check(docXml.includes('Denna rapport') && !docXml.includes('data-bp') && !docXml.includes('data-bgen'), 'the downloaded document has the translation and no Babel ids');
check(stylesXml.includes('w:val="sv-SE"'), 'and is marked Swedish');

check(!errors.length, 'no page errors' + (errors.length ? ': ' + errors.join(' | ') : ''));
await browser.close();
console.log(failures ? `\n${failures} FAILED` : '\nall passed');
process.exit(failures ? 1 : 0);
