// Two devices on one account, against babel-server.
//   babel-server --db /tmp/t.db user add tester   (password testpass123)
//   babel-server --db /tmp/t.db serve --listen 127.0.0.1:8791
//   BASE=http://localhost:8791/ LOGIN=tester:testpass123 node test/sync.mjs
// Checks that a save on one shows up on the other while it is open, and that
// near-simultaneous edits to different paragraphs are merged, not lost.

import { createRequire } from 'module';
import path from 'path';

const require = createRequire(process.env.PLAYWRIGHT_DIR ?? '/home/jesper/cprsvp/');
const { chromium } = require('playwright');
const BASE = process.env.BASE ?? 'http://localhost:8791/';
const [USER, PASS] = (process.env.LOGIN ?? 'tester:testpass123').split(/:(.*)/);
const fixture = path.resolve(path.dirname(new URL(import.meta.url).pathname), 'fixtures/sample.docx');

let failures = 0;
const check = (cond, msg) => { console.log(`${cond ? 'ok  ' : 'FAIL'} ${msg}`); if (!cond) failures++; };

const browser = await chromium.launch({ executablePath: process.env.CHROME ?? '/usr/bin/google-chrome' });
const errors = [];

async function device(name) {
  const ctx = await browser.newContext({ viewport: { width: 1400, height: 900 } });
  const page = await ctx.newPage();
  page.on('pageerror', e => errors.push(`${name}: ${e.message}`));
  await page.goto(BASE);
  await page.waitForSelector('#loginForm', { state: 'visible' });
  await page.fill('#loginUser', USER);
  await page.fill('#loginPass', PASS);
  await page.click('#loginForm button[type=submit]');
  await page.waitForSelector('#home', { state: 'visible' });
  return page;
}

const opened = page => page.waitForFunction(() => document.querySelector('#loading').hidden && window.__babel?.P, null, { timeout: 30000 });
const saved = page => page.waitForFunction(() => document.querySelector('#saveState').dataset.state === 'saved', null, { timeout: 15000 });
const text = (page, pid) => page.evaluate(pid => __babel.P.tgtEl.get(pid)?.textContent ?? null, pid);

// Replaces the first text run of a paragraph with `value`.
async function edit(page, pid, value) {
  await page.evaluate(pid => {
    const p = document.querySelector(`#tgtDoc p[data-p="${pid}"]`);
    const t = p.querySelector('span[data-k="t"]').firstChild;
    const r = document.createRange(); r.setStart(t, 0); r.setEnd(t, t.length);
    p.focus(); getSelection().removeAllRanges(); getSelection().addRange(r);
  }, pid);
  await page.keyboard.type(value);
}

const a = await device('A');
await a.setInputFiles('#fileInput', fixture);
await a.waitForSelector('#langDlg[open]');
await a.fill('#langSearch', 'swed');
await a.keyboard.press('Enter');
await opened(a);
const id = await a.evaluate(() => __babel.P.project.id);

await edit(a, 'document:2', 'Från A ');
await a.keyboard.press('Control+s');
await saved(a);

const b = await device('B');
check(await b.isVisible(`a[href="#p=${id}"]`), 'the translation is listed on the other device');
await b.goto(BASE + '#p=' + id);
await opened(b);
check((await text(b, 'document:2')).startsWith('Från A'), 'B opens it with A’s edit');

// B edits; A has it open and picks it up without doing anything.
await edit(b, 'document:4', 'Från B ');
await b.keyboard.press('Control+s');
await saved(b);
await a.waitForFunction(() => __babel.P?.tgtEl.get('document:4')?.textContent.startsWith('Från B'), null, { timeout: 15000 })
  .then(() => check(true, 'A shows B’s edit live'), () => check(false, 'A shows B’s edit live'));
await opened(a);

// Both edit different paragraphs at about the same time.
await Promise.all([edit(a, 'document:6', 'Samtidigt A '), edit(b, 'document:8', 'Samtidigt B ')]);
await Promise.all([a.keyboard.press('Control+s'), b.keyboard.press('Control+s')]);
await a.waitForTimeout(3000);
await Promise.all([opened(a), opened(b)]);

const res = await a.evaluate(async id => (await (await fetch(`api/projects/${id}/parts`)).json()), id);
const doc = res.parts['word/document.xml'] ?? '';
check(doc.includes('Från A') && doc.includes('Från B'), 'the server has both earlier edits');
check(doc.includes('Samtidigt A') && doc.includes('Samtidigt B'), `simultaneous edits were merged (rev ${res.rev})`);
for (const [name, page] of [['A', a], ['B', b]]) {
  const six = await text(page, 'document:6'), eight = await text(page, 'document:8');
  check(six?.startsWith('Samtidigt A') && eight?.startsWith('Samtidigt B'), `${name} ends up showing both`);
}

check(!errors.length, 'no page errors' + (errors.length ? ': ' + errors.join(' | ') : ''));
await browser.close();
console.log(failures ? `\n${failures} FAILED` : '\nall passed');
process.exit(failures ? 1 : 0);
