// End-to-end test in a real browser.
//   python3 -m http.server 8790   (from the repo root)
//   node test/e2e.mjs
// Uses Playwright from a sibling checkout or any installed copy (PLAYWRIGHT_DIR).

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
page.on('console', m => { if (m.type() === 'error' && !/favicon|404|simulated crash/.test(m.text())) errors.push(m.text()); });

await page.goto(BASE);
await page.setInputFiles('#fileInput', fixture);
await page.waitForSelector('#langDlg[open]');
await page.fill('#langSearch', 'swed');
await page.keyboard.press('Enter');
await page.waitForFunction(() => document.querySelector('#loading').hidden, null, { timeout: 30000 });

const pages = await page.evaluate(() => [document.querySelectorAll('#srcDoc section').length, document.querySelectorAll('#tgtDoc section').length]);
check(pages[0] > 1 && pages[0] === pages[1], `same page count in both panels (${pages})`);
const status = await page.textContent('#srcStatus');
check(/characters with spaces/.test(status), `source status bar: ${status.replace(/\s+/g, ' ').trim()}`);

// Edit a mixed-format paragraph: replace the first plain run's text.
const pid = 'document:2';
await page.evaluate(pid => {
  const p = document.querySelector(`#tgtDoc p[data-p="${pid}"]`);
  const t = p.querySelector('span[data-k="t"]').firstChild;
  const r = document.createRange(); r.setStart(t, 0); r.setEnd(t, t.length);
  p.focus(); getSelection().removeAllRanges(); getSelection().addRange(r);
}, pid);
await page.keyboard.type('Denna rapport är ');
await page.keyboard.press('End');
await page.keyboard.type(' Ett felstavat ord: meing.');
await page.waitForTimeout(400);
let xml = await page.evaluate(pid => new XMLSerializer().serializeToString(__babel.P.tgtEl.get(pid)), pid);
check(xml.includes('Denna rapport är ') && xml.includes('<w:b/>'), 'edit written into the run, bold run kept');
check(xml.includes('meing.'), 'text typed at the end landed in the XML');

// Bold a word with Ctrl+B on new text.
await page.keyboard.type(' ');
await page.keyboard.press('Control+b');
await page.keyboard.type('fetstil');
await page.keyboard.press('Control+b');
await page.waitForTimeout(400);
xml = await page.evaluate(pid => new XMLSerializer().serializeToString(__babel.P.tgtEl.get(pid)), pid);
check(/<w:r[^>]*><w:rPr>(?:(?!<\/w:r>).)*<w:b\/>(?:(?!<\/w:r>).)*<w:t[^>]*>fetstil<\/w:t>/.test(xml), 'Ctrl+B produced a bold run');

// Line break with Shift+Enter, Enter moves to the next paragraph.
await page.keyboard.press('Shift+Enter');
await page.keyboard.type('Ny rad');
await page.waitForTimeout(400);
xml = await page.evaluate(pid => new XMLSerializer().serializeToString(__babel.P.tgtEl.get(pid)), pid);
check(/<w:br\/>(?:(?!<w:t).)*<w:t[^>]*>Ny rad/.test(xml), 'Shift+Enter inserted a w:br before the new text');
await page.keyboard.press('Enter');
const active = await page.evaluate(() => document.activeElement.dataset.p);
check(active === 'document:3', `Enter moved to the next paragraph (${active})`);
const paraCount = await page.evaluate(() => __babel.P.tgt['word/document.xml'].getElementsByTagNameNS('http://schemas.openxmlformats.org/wordprocessingml/2006/main', 'p').length);

// Clicking inside a full-paragraph selection just places the caret (no
// native drag detection), and drag-selecting still works.
{
  const [x, y, w, h] = await page.evaluate(() => { const r = document.querySelector('#tgtDoc p[data-p="document:3"]').getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; });
  await page.mouse.click(x + 10, y + h / 2, { clickCount: 3 });
  const full = await page.evaluate(() => getSelection().toString().length);
  await page.mouse.click(x + w / 4, y + h / 2);
  const after = await page.evaluate(() => { const s = getSelection(); return { collapsed: s.isCollapsed, host: document.activeElement.dataset.p, inHost: !!s.anchorNode?.parentElement?.closest('p[data-p="document:3"]'), off: s.anchorOffset }; });
  check(full > 10 && after.collapsed && after.host === 'document:3' && after.inHost && after.off > 0, `click inside a selection places the caret (${JSON.stringify(after)})`);
  await page.mouse.move(x + 3, y + h / 2); await page.mouse.down(); await page.mouse.move(x + w / 2, y + h / 2, { steps: 6 }); await page.mouse.up();
  const dragged = await page.evaluate(() => getSelection().toString().length);
  check(dragged > 3, `drag-selecting still works (${dragged} chars)`);
}

// Deleting a footnote reference is refused.
await page.evaluate(() => {
  const p = document.querySelector('#tgtDoc article sup').closest('p');
  const t = p.querySelector('span[data-k="t"]').firstChild;
  const r = document.createRange(); r.setStart(t, t.length); r.collapse(true);
  p.focus(); getSelection().removeAllRanges(); getSelection().addRange(r);
});
await page.keyboard.press('Delete');
await page.keyboard.press('Delete');
await page.waitForTimeout(300);
const fnRef = await page.evaluate(() => document.querySelectorAll('#tgtDoc article sup').length === 1 && !!document.querySelector('#tgtDoc article sup').closest('p').textContent.includes('sentence.1'));
check(fnRef, 'footnote reference survives Delete');

// Tab text edit: change "Amount" heading, tabs stay.
await page.evaluate(() => {
  const s = document.querySelector('#tgtDoc p[data-p="document:4"] span[data-r="document:r14"]').firstChild;
  const r = document.createRange(); r.selectNodeContents(s);
  s.parentElement.closest('p').focus(); getSelection().removeAllRanges(); getSelection().addRange(r);
});
await page.keyboard.type('Belopp');
await page.waitForTimeout(400);
xml = await page.evaluate(() => new XMLSerializer().serializeToString(__babel.P.tgtEl.get('document:4')));
check(/Item<\/w:t>.*<w:tab\/>.*Belopp<\/w:t>.*<w:tab\/>.*Share/.test(xml), 'tabs preserved around edited text');

// Header edit propagates to all pages.
await page.evaluate(() => {
  const p = document.querySelector('#tgtDoc header p[data-p]');
  const t = p.querySelector('span[data-k="t"]').firstChild;
  const r = document.createRange(); r.selectNodeContents(t);
  p.focus(); getSelection().removeAllRanges(); getSelection().addRange(r);
});
await page.keyboard.type('Regionala utvecklingsverket');
await page.waitForTimeout(400);
const headers = await page.evaluate(() => [...document.querySelectorAll('#tgtDoc header')].map(h => h.textContent));
check(headers.every(h => h.startsWith('Regionala utvecklingsverket')), `header updated on all ${headers.length} pages`);

// Overflow: stuff a body paragraph on page 2 with text.
const before = await page.evaluate(() => document.querySelectorAll('#tgtDoc section.b-over').length);
await page.evaluate(() => {
  const p = document.querySelector('#tgtDoc p[data-p="document:25"]');
  const t = p.querySelector('span[data-k="t"]').firstChild;
  const r = document.createRange(); r.setStart(t, t.length); r.collapse(true);
  p.focus(); getSelection().removeAllRanges(); getSelection().addRange(r);
});
await page.keyboard.type(' Ytterligare text som gör sidan längre än originalet.'.repeat(40));
await page.waitForTimeout(900);
const after = await page.evaluate(() => [...document.querySelectorAll('#tgtDoc section.b-over')].map(s => s.dataset.over));
check(before === 0 && after.length >= 1, `page overflow flagged: ${after.join('; ')}`);

// Proofing (needs network): the misspelling should get underlined.
let spelled = 0;
for (let i = 0; i < 30 && !spelled; i++) {
  await page.waitForTimeout(500);
  spelled = await page.evaluate(() => CSS.highlights.get('b-spelling')?.size ?? 0);
}
check(spelled > 0, `spelling issues highlighted (${spelled})`);
await page.screenshot({ path: `${OUT}/babel-edit.png` });

// Context menu: copy a link in the original, paste it onto target text.
const selectWord = (word, pidHint) => page.evaluate(([word]) => {
  const w = document.createTreeWalker(document.querySelector('#tgtDoc'), NodeFilter.SHOW_TEXT);
  for (let n; (n = w.nextNode());) {
    const i = n.data.indexOf(word);
    if (i < 0 || !n.parentElement.closest('p[contenteditable="true"]')) continue;
    const host = n.parentElement.closest('p[data-p]');
    const r = document.createRange(); r.setStart(n, i); r.setEnd(n, i + word.length);
    host.focus(); getSelection().removeAllRanges(); getSelection().addRange(r);
    const b = r.getBoundingClientRect();
    return { pid: host.dataset.p, x: b.left + b.width / 2, y: b.top + b.height / 2 };
  }
  return null;
}, [word]);
{
  const linkBox = await page.evaluate(() => {
    const a = document.querySelector('#srcDoc a');
    a.scrollIntoView({ block: 'center' });
    const b = a.getBoundingClientRect(); return { x: b.left + 5, y: b.top + b.height / 2 };
  });
  await page.waitForTimeout(300);
  await page.mouse.click(linkBox.x, linkBox.y, { button: 'right' });
  const items = await page.$$eval('#ctxMenu:not([hidden]) .menu-item', bs => bs.map(b => b.textContent.replace(/\s+/g, ' ').trim()));
  check(items.some(t => t.startsWith('Copy link') && t.includes('https://example.org/')), `source menu offers Copy link (${items.join(' | ')})`);
  await page.click('#ctxMenu .menu-item:has-text("Copy link")');
  const clip = await page.evaluate(() => navigator.clipboard.readText());
  check(clip === 'https://example.org/', `link on the clipboard (${clip})`);

  // Look up a selected source term in Merriam-Webster.
  await ctx.route('https://www.merriam-webster.com/**', r => r.fulfill({ body: 'mw', contentType: 'text/plain' }));
  const term = await page.evaluate(() => {
    const w = document.createTreeWalker(document.querySelector('#srcDoc'), NodeFilter.SHOW_TEXT);
    for (let n; (n = w.nextNode());) {
      const i = n.data.indexOf('board of directors');
      if (i < 0) continue;
      const r = document.createRange(); r.setStart(n, i); r.setEnd(n, i + 18);
      getSelection().removeAllRanges(); getSelection().addRange(r);
      const b = r.getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 };
    }
  });
  await page.mouse.click(term.x, term.y, { button: 'right' });
  const lookItems = await page.$$eval('#ctxMenu:not([hidden]) .menu-item', bs => bs.map(b => b.textContent.replace(/\s+/g, ' ').trim()));
  check(lookItems.some(t => t === 'Look up “board of directors” in Merriam-Webster'), `source menu offers the look-up (${lookItems.join(' | ')})`);
  const [popup] = await Promise.all([ctx.waitForEvent('page'), page.click('#ctxMenu .menu-item:has-text("Merriam-Webster")')]);
  await popup.waitForURL(/merriam-webster/, { timeout: 10000 }).catch(() => {});
  check(popup.url() === 'https://www.merriam-webster.com/dictionary/board%20of%20directors', `look-up opens ${popup.url()}`);
  await popup.close();
  await page.mouse.click(linkBox.x, linkBox.y, { button: 'right' });
  const noSel = await page.$$eval('#ctxMenu:not([hidden]) .menu-item', bs => bs.map(b => b.textContent));
  check(!noSel.some(t => t.includes('Merriam')), 'no look-up without a selection');
  await page.keyboard.press('Escape');

  const w = await selectWord('budget');
  await page.mouse.click(w.x, w.y, { button: 'right' });
  const titems = await page.$$eval('#ctxMenu:not([hidden]) .menu-item', bs => bs.map(b => b.textContent.replace(/\s+/g, ' ').trim()));
  check(titems.some(t => t.startsWith('Paste link')), `target menu offers Paste link (${titems.join(' | ')})`);
  await page.click('#ctxMenu .menu-item:has-text("Paste link")');
  await page.waitForTimeout(500);
  const res = await page.evaluate(pid => {
    const el = __babel.P.tgtEl.get(pid);
    const xml = new XMLSerializer().serializeToString(el);
    const rels = new XMLSerializer().serializeToString(__babel.P.rels['word/_rels/document.xml.rels']);
    const a = document.querySelector(`#tgtDoc p[data-p="${pid}"] a`);
    return { xml, rels, a: a?.textContent };
  }, w.pid);
  const relId = res.xml.match(/<w:hyperlink [^>]*r:id="([^"]+)"/)?.[1];
  check(/<w:hyperlink[^>]*>(?:(?!<\/w:hyperlink>).)*<w:rStyle w:val="Hyperlink"\/>(?:(?!<\/w:hyperlink>).)*>budget<\/w:t>/.test(res.xml), 'selected word wrapped in w:hyperlink with the source link style');
  check(relId && res.rels.includes(`Id="${relId}"`) && res.rels.includes('Target="https://example.org/"'), `relationship ${relId} added for the link`);
  check(res.a === 'budget', `the link is shown in the translation (${res.a})`);

  // Typing inside the link keeps the text in the link.
  await page.evaluate(pid => {
    const t = document.querySelector(`#tgtDoc p[data-p="${pid}"] a [data-k="t"]`).firstChild;
    const r = document.createRange(); r.setStart(t, 3); r.collapse(true);
    t.parentElement.closest('p').focus(); getSelection().removeAllRanges(); getSelection().addRange(r);
  }, w.pid);
  await page.keyboard.type('XX');
  await page.waitForTimeout(400);
  const typed = await page.evaluate(pid => new XMLSerializer().serializeToString(__babel.P.tgtEl.get(pid)), w.pid);
  check(/<w:hyperlink[^>]*>(?:(?!<\/w:hyperlink>).)*budXXget(?:(?!<\/w:hyperlink>).)*<\/w:hyperlink>/.test(typed), 'typing inside the link stays in the link');

  // Ctrl+K with a web address copied elsewhere.
  await page.evaluate(() => navigator.clipboard.writeText('www.babel-test.example/page'));
  const w2 = await selectWord('review');
  await page.keyboard.press('Control+k');
  await page.waitForTimeout(500);
  const rels2 = await page.evaluate(() => new XMLSerializer().serializeToString(__babel.P.rels['word/_rels/document.xml.rels']));
  const x2 = await page.evaluate(pid => new XMLSerializer().serializeToString(__babel.P.tgtEl.get(pid)), w2.pid);
  check(rels2.includes('Target="https://www.babel-test.example/page"') && /<w:hyperlink[^>]*>(?:(?!<\/w:hyperlink>).)*>review<\/w:t>/.test(x2), 'Ctrl+K links the selection to the address on the clipboard');

  // Selections that cross an existing link are refused, nothing changes.
  const before = await page.evaluate(pid => new XMLSerializer().serializeToString(__babel.P.tgtEl.get(pid)), w.pid);
  await page.evaluate(pid => {
    const p = document.querySelector(`#tgtDoc p[data-p="${pid}"]`);
    const r = document.createRange(); r.selectNodeContents(p);
    p.focus(); getSelection().removeAllRanges(); getSelection().addRange(r);
  }, w.pid);
  await page.keyboard.press('Control+k');
  await page.waitForTimeout(400);
  const after = await page.evaluate(pid => new XMLSerializer().serializeToString(__babel.P.tgtEl.get(pid)), w.pid);
  const msg = await page.textContent('#toast');
  check(before === after && /crosses a link/.test(msg), `a selection across a link is refused (${msg.trim()})`);
  await page.keyboard.press('Escape');
}

// Reload: everything must come back.
await page.evaluate(() => __babel.flushAll());
await page.waitForTimeout(1500);
await page.reload();
await page.waitForFunction(() => document.querySelector('#loading')?.hidden && __babel.P, null, { timeout: 30000 });
const restored = await page.textContent(`#tgtDoc p[data-p="${pid}"]`);
check(restored.startsWith('Denna rapport är ') && restored.includes('fetstil'), 'edits restored after reload');
check(await page.evaluate(() => !!document.querySelector('#tgtDoc p[data-p="document:2"] .b-br, #tgtDoc p[data-p="document:2"] [data-k="br"]')), 'line break restored after reload');
check(await page.evaluate(() => [...document.querySelectorAll('#tgtDoc a')].map(a => a.textContent).join('|')).then(t => t.includes('budXXget') && t.includes('review')), 'pasted links restored after reload');

// Crash recovery: an edit that reaches only the journal.
await page.evaluate(() => {
  const p = document.querySelector('#tgtDoc p[data-p="document:3"]');
  const t = p.querySelector('span[data-k="t"]').firstChild;
  const r = document.createRange(); r.setStart(t, 0); r.setEnd(t, t.length);
  p.focus(); getSelection().removeAllRanges(); getSelection().addRange(r);
});
// Make every IndexedDB write fail from here on, as if the tab died before
// the debounced save: only the synchronous journal can carry this edit.
await page.evaluate(() => {
  IDBObjectStore.prototype.put = function () { throw new Error('simulated crash'); };
  IDBObjectStore.prototype.add = IDBObjectStore.prototype.put;
});
await page.keyboard.type('Se även ');
await page.evaluate(() => __babel.flushAll());
const id = await page.evaluate(() => __babel.P.project.id);
const journal = await page.evaluate(id => localStorage.getItem('babel.journal.' + id), id);
check(journal && journal.includes('Se även'), 'edit is in the synchronous journal immediately');
await page.waitForTimeout(1500);
const page2 = await ctx.newPage();
page2.on('pageerror', e => errors.push(e.message));
page2.on('console', m => { if (m.type() === 'error' && !/favicon|404/.test(m.text())) errors.push(m.text()); });
await page.close({ runBeforeUnload: false });
await page2.goto(BASE + '#p=' + id);
await page2.waitForFunction(() => document.querySelector('#loading')?.hidden && __babel.P, null, { timeout: 30000 });
const rec = await page2.textContent('#tgtDoc p[data-p="document:3"]');
check(rec.startsWith('Se även '), 'edit recovered after a renderer crash');

// Download and inspect the file.
const [dl] = await Promise.all([page2.waitForEvent('download'), page2.click('#btnDownload')]);
const file = `${OUT}/babel-out.docx`;
await dl.saveAs(file);
check(/_SV\.docx$/.test(dl.suggestedFilename()), `download name ${dl.suggestedFilename()}`);
const py = `
import zipfile, xml.dom.minidom as m, sys
z = zipfile.ZipFile(sys.argv[1]); src = zipfile.ZipFile(sys.argv[2])
for n in z.namelist():
    if n.endswith('.xml') or n.endswith('.rels'): m.parseString(z.read(n))
d = z.read('word/document.xml').decode()
print('names', sorted(z.namelist()) == sorted(src.namelist()))
print('ids', 'data-b' in d or 'data-b' in z.read('word/header1.xml').decode())
print('text', 'Denna rapport' in d and 'Belopp' in d and 'Se även' in d)
print('lang', 'w:val="sv-SE"' in z.read('word/styles.xml').decode())
print('media', z.read('word/media/chart.png') == src.read('word/media/chart.png'))
print('numbering', z.read('word/numbering.xml') == src.read('word/numbering.xml'))
print('decl', d.startswith('<?xml'))
import re
print('paras', len(re.findall(r'<w:p[ >/]', d)))
print('hdr', 'Regionala utvecklingsverket' in z.read('word/header1.xml').decode())
import re
rels = z.read('word/_rels/document.xml.rels').decode()
ids = re.findall(r'<w:hyperlink [^>]*r:id="([^"]+)"', d)
print('links', len(ids) >= 3 and all(f'Id="{i}"' in rels for i in ids) and 'babel-test.example' in rels)
`;
const res = execFileSync('python3', ['-c', py, file, fixture]).toString();
console.log(res.trim().split('\n').map(l => '     ' + l).join('\n'));
check(/names True/.test(res) && /ids False/.test(res) && /text True/.test(res) && /lang True/.test(res)
  && /media True/.test(res) && /links True/.test(res) && /numbering True/.test(res) && /decl True/.test(res) && /hdr True/.test(res), 'downloaded docx is clean and complete');
const outParas = +res.match(/paras (\d+)/)[1];
check(outParas === paraCount, `paragraph count unchanged (${outParas} vs ${paraCount})`);

// Pair the original with the downloaded translation.
await page2.goto(BASE + '#');
await page2.waitForSelector('#home:not([hidden])');
await page2.click('.resume summary');
await page2.setInputFiles('#resumeSrc', fixture);
await page2.setInputFiles('#resumeTgt', file);
await page2.click('#resumeGo');
await page2.waitForSelector('#langDlg[open]');
await page2.keyboard.press('Enter');
await page2.waitForFunction(() => document.querySelector('#loading')?.hidden && __babel.P, null, { timeout: 30000 });
const paired = await page2.textContent('#tgtDoc p[data-p="document:2"]');
check(paired.startsWith('Denna rapport är '), 'paired translation opens with its text');
const pstat = await page2.textContent('#tgtStatus');
check(/% done/.test(pstat) && !/ 0 % done/.test(pstat), `progress after pairing: ${pstat.replace(/\s+/g, ' ').trim()}`);
await page2.screenshot({ path: `${OUT}/babel-paired.png` });

check(errors.length === 0, `no page errors${errors.length ? ': ' + errors.join(' | ') : ''}`);
await browser.close();
console.log(failures ? `\n${failures} FAILED` : '\nall passed');
process.exit(failures ? 1 : 0);
