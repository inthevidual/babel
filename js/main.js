import * as X from './ooxml.js';
import * as S from './store.js';
import * as V from './view.js';
import * as E from './editor.js';
import { Proofer } from './proof.js';
import { LANGS, langByCode } from './langs.js';

const $ = (sel, el = document) => el.querySelector(sel);
const $$ = (sel, el = document) => [...el.querySelectorAll(sel)];
const nf = new Intl.NumberFormat();
const pct = new Intl.NumberFormat(undefined, { maximumFractionDigits: 1, signDisplay: 'exceptZero' });
const esc = s => String(s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));

// ── Settings ───────────────────────────────────────────────────────────────

const SETTINGS_KEY = 'babel.settings';
const DICT_KEY = 'babel.dictionary';
const settings = Object.assign({
  proof: true, picky: false, native: false, marks: true, sync: true,
  endpoint: 'https://api.languagetool.org', user: '', key: '', lastLang: 'sv-SE', zoom: 'fit',
}, readJSON(SETTINGS_KEY, {}));
let dictionary = new Set(readJSON(DICT_KEY, []));

function readJSON(key, fallback) {
  try { return JSON.parse(localStorage.getItem(key)) ?? fallback; } catch { return fallback; }
}
function writeJSON(key, value) {
  try { localStorage.setItem(key, JSON.stringify(value)); } catch { /* storage full */ }
}
const saveSettings = () => writeJSON(SETTINGS_KEY, settings);

// ── Session state ──────────────────────────────────────────────────────────

let P = null; // the open project session

const srcRoot = $('#srcDoc');
const tgtRoot = $('#tgtDoc');
const srcZoom = new V.Zoom($('#srcPanel .sizer'), $('#srcPanel .scaler'));
const tgtZoom = new V.Zoom($('#tgtPanel .sizer'), $('#tgtPanel .scaler'));

// ── Routing ────────────────────────────────────────────────────────────────

addEventListener('hashchange', route);
route();

async function route() {
  const m = location.hash.match(/^#p=([\w-]+)/);
  if (P && (!m || m[1] !== P.project.id)) await closeProject();
  if (m) {
    if (!P) await openProject(m[1]).catch(fatal);
  } else showHome();
}

function fatal(e) {
  console.error(e);
  $('#loading').hidden = true;
  toast(e.message || String(e), 'error', 12000);
  if (!P) { history.replaceState(null, '', '#'); showHome(); }
}

// ── Home ───────────────────────────────────────────────────────────────────

async function showHome() {
  document.body.classList.remove('working');
  $('#work').hidden = true;
  $('#barDoc').hidden = $('#barTools').hidden = true;
  $('#home').hidden = false;
  document.title = 'Babel';
  let projects = [];
  try { projects = await S.listProjects(); } catch (e) { toast('Browser storage is unavailable: ' + e.message, 'error', 12000); }
  const list = $('#recentList');
  list.innerHTML = '';
  $('#recent').hidden = !projects.length;
  for (const p of projects) {
    const st = p.stats ?? {};
    const done = st.total ? Math.round(100 * (st.done ?? 0) / st.total) : 0;
    const li = document.createElement('li');
    li.innerHTML = `
      <a class="rec-main" href="#p=${p.id}">
        <span class="rec-name">${esc(p.name)}</span>
        <span class="rec-meta">→ ${esc(langByCode(p.targetLang).name)} · ${done} % translated · ${esc(timeAgo(p.updatedAt))}</span>
        <span class="rec-bar"><i style="width:${done}%"></i></span>
      </a>
      <button class="ghost small" data-act="download">Download</button>
      <button class="ghost small danger" data-act="delete">Delete</button>`;
    li.querySelector('[data-act=download]').onclick = () => downloadProject(p.id).catch(fatal);
    li.querySelector('[data-act=delete]').onclick = async () => {
      if (!confirm(`Delete “${p.name}” and all its versions from this browser? Download it first if you want to keep it.`)) return;
      await S.deleteProject(p.id);
      showHome();
    };
    list.appendChild(li);
  }
  const info = await S.storageInfo();
  $('#storageNote').textContent = info
    ? `Using ${fmtBytes(info.usage)} of browser storage. ${info.persisted ? 'Storage is persistent: the browser won’t clear it on its own.' : 'The browser may clear this storage when space runs low — download your work regularly.'}`
    : '';
}

const fileInput = $('#fileInput');
fileInput.onchange = () => { if (fileInput.files[0]) startNew(fileInput.files[0]); fileInput.value = ''; };
const drop = $('#drop');
drop.addEventListener('keydown', e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); fileInput.click(); } });
for (const ev of ['dragenter', 'dragover']) drop.addEventListener(ev, e => { e.preventDefault(); drop.classList.add('over'); });
for (const ev of ['dragleave', 'drop']) drop.addEventListener(ev, () => drop.classList.remove('over'));
drop.addEventListener('drop', e => {
  e.preventDefault();
  const f = e.dataTransfer.files[0];
  if (f) startNew(f);
});
addEventListener('dragover', e => e.preventDefault());
addEventListener('drop', e => e.preventDefault());

const resumeCheck = () => { $('#resumeGo').disabled = !($('#resumeSrc').files[0] && $('#resumeTgt').files[0]); };
$('#resumeSrc').onchange = $('#resumeTgt').onchange = resumeCheck;
$('#resumeGo').onclick = e => { e.preventDefault(); startNew($('#resumeSrc').files[0], $('#resumeTgt').files[0]); };

async function startNew(file, translated = null) {
  try {
    if (!/\.docx$/i.test(file.name)) throw new Error(`“${file.name}” is not a .docx file. Open it in Word and save it as .docx first.`);
    const bytes = await file.arrayBuffer();
    const pkg = await X.openPackage(bytes);
    const lang = await askLanguage({ title: 'Translate into…', sub: file.name, current: settings.lastLang, ok: 'Start translating' });
    if (!lang) return;
    settings.lastLang = lang; saveSettings();

    const srcParts = {};
    for (const path of pkg.textParts) {
      const orig = await pkg.zip.file(path).async('string');
      srcParts[path] = X.serializeXml(X.normalize(X.parseXml(orig), X.partKey(path)), orig);
    }
    const tgtParts = {};
    if (translated) {
      const tpkg = await X.openPackage(await translated.arrayBuffer());
      for (const path of pkg.textParts) {
        const f = tpkg.zip.file(path);
        if (!f) throw new Error(`The translation has no ${path}; it doesn’t share the original’s structure.`);
        const orig = await f.async('string');
        const doc = X.normalize(X.parseXml(orig), X.partKey(path));
        const a = X.parseXml(srcParts[path]).getElementsByTagNameNS(X.W, 'p').length;
        const b = doc.getElementsByTagNameNS(X.W, 'p').length;
        if (a !== b) throw new Error(`The translation’s structure differs from the original (${X.partKey(path)}: ${a} vs ${b} paragraphs), so they can’t be paired.`);
        tgtParts[path] = X.serializeXml(doc, orig);
      }
    }
    const now = Date.now();
    const project = {
      id: crypto.randomUUID(),
      name: file.name.replace(/\.docx$/i, ''),
      fileName: file.name,
      targetLang: lang,
      createdAt: now,
      updatedAt: now,
      textParts: pkg.textParts,
      confirmed: [],
      ignored: [],
      stats: {},
    };
    await S.createProject(project, { id: project.id, bytes, parts: srcParts });
    if (translated) await S.saveParts(project, tgtParts);
    S.requestPersistence();
    location.hash = '#p=' + project.id;
  } catch (e) { fatal(e); }
}

// ── Language picker ────────────────────────────────────────────────────────

function askLanguage({ title, sub, current, ok }) {
  const dlg = $('#langDlg');
  $('#langTitle').textContent = title;
  $('#langSub').textContent = sub ?? '';
  $('#langOk').textContent = ok;
  const list = $('#langList');
  const search = $('#langSearch');
  let chosen = current;
  const draw = () => {
    const q = search.value.trim().toLowerCase();
    list.innerHTML = '';
    for (const l of LANGS) {
      if (q && !`${l.name} ${l.en} ${l.code}`.toLowerCase().includes(q)) continue;
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'lang-opt' + (l.code === chosen ? ' on' : '');
      b.setAttribute('role', 'option');
      b.setAttribute('aria-selected', l.code === chosen);
      b.innerHTML = `<span>${esc(l.name)}</span><small>${esc(l.en)}${l.lt ? '' : ' · no proofing'}</small>`;
      b.onclick = () => { chosen = l.code; $('#langOk').disabled = false; draw(); };
      b.ondblclick = () => { chosen = l.code; dlg.close('ok'); };
      list.appendChild(b);
    }
  };
  search.value = '';
  search.oninput = draw;
  search.onkeydown = e => {
    if (e.key === 'Enter') {
      e.preventDefault();
      const first = list.querySelector('.lang-opt');
      if (first) first.click();
      if (chosen) dlg.close('ok');
    }
  };
  $('#langOk').disabled = !chosen;
  draw();
  dlg.returnValue = '';
  dlg.showModal();
  search.focus();
  list.querySelector('.on')?.scrollIntoView({ block: 'center' });
  return new Promise(resolve => {
    dlg.onclose = () => resolve(dlg.returnValue === 'ok' ? chosen : null);
  });
}

// ── Open / close ───────────────────────────────────────────────────────────

async function openProject(id) {
  $('#home').hidden = true;
  $('#work').hidden = false;
  $('#loading').hidden = false;
  $('#loadingText').textContent = 'Opening…';
  document.body.classList.add('working');

  const project = await S.getProject(id);
  if (!project) throw new Error('That translation is not in this browser’s storage.');
  const source = await S.getSource(id);
  const stored = await S.getParts(id);

  const lock = await acquireLock(id);
  const session = P = {
    project, source, lock,
    keyToPath: Object.fromEntries(project.textParts.map(p => [X.partKey(p), p])),
    src: {}, tgt: {},               // path -> XML Document
    srcEl: new Map(), tgtEl: new Map(), // pid -> XML element
    meta: new Map(),                // pid -> paragraph bookkeeping
    info: new Map(),                // pid -> rendered run ids
    templates: new Map(),
    dirty: new Set(),
    pending: new Map(),             // pid -> {host, timer}
    issues: new Map(),
    confirmed: new Set(project.confirmed ?? []),
    ignored: new Set(project.ignored ?? []),
    splits: [],
    readOnly: false,
    lastSnapshotAt: Date.now(),
    changedSinceSnapshot: false,
  };

  // Target XML: stored parts, else the source; then replay the journal.
  for (const path of project.textParts) {
    P.src[path] = X.parseXml(source.parts[path]);
    P.tgt[path] = X.parseXml(stored[path] ?? source.parts[path]);
  }
  const journal = S.journalRead(id);
  let replayed = 0;
  for (const [pid, e] of Object.entries(journal.entries)) {
    const path = P.keyToPath[pid.split(':')[0]];
    const doc = P.tgt[path];
    const el = doc?.querySelector(`[data-bp="${CSS.escape(pid)}"]`);
    if (!el) continue;
    try {
      el.replaceWith(doc.importNode(parseFragment(doc, e.xml), true));
      P.dirty.add(path);
      replayed++;
    } catch (err) { console.warn('journal entry skipped', pid, err); }
  }
  for (const path of project.textParts) X.ensureIds(P.tgt[path], X.partKey(path));
  indexXml();

  $('#docName').textContent = project.name;
  $('#barDoc').hidden = $('#barTools').hidden = false;
  document.title = `${project.name} — Babel`;
  updateLangPill();

  // Render the source, paginate it, then mirror onto the target.
  $('#loadingText').textContent = 'Laying out the original…';
  const srcBytes = await X.buildRenderable(source.bytes, source.parts);
  await V.render(srcBytes, srcRoot, styleHolder('src'), 'bsrc');
  if (P !== session) return;
  P.splits = V.paginate(srcRoot);
  V.numberPages(srcRoot);
  $('#loadingText').textContent = 'Laying out the translation…';
  await renderTarget();
  if (P !== session) return;
  for (const a of $$('a[href]', srcRoot)) a.removeAttribute('href');

  applyZoom(settings.zoom);
  $('#syncScroll').checked = settings.sync;
  setMarks();
  computeStats();
  updateStatus();
  configureProofer();
  for (const [pid, m] of P.meta) if (m.edited) proofer.queue(pid, 0);
  checkOverflow();
  $('#loading').hidden = true;
  if (replayed) { toast(`Recovered ${replayed} paragraph${replayed > 1 ? 's' : ''} from the last session.`); save(); }
  setSaveState('saved', project.updatedAt);
  if (P.dirty.size) scheduleSave(500);
  // A version at the start of each working session, if there is work to keep.
  if (Object.keys(stored).length) snapshot('session start');
  const first = $$('p[data-p]', tgtRoot).find(p => !p.closest('header, footer') && /\p{L}/u.test(p.textContent));
  if (first) E.placeCaret(first, 'start');
}

function styleHolder(which) {
  let el = $(`#docStyles [data-for="${which}"]`);
  if (!el) {
    el = document.createElement('div');
    el.dataset.for = which;
    $('#docStyles').appendChild(el);
  }
  return el;
}

async function renderTarget() {
  const parts = {};
  for (const path of P.project.textParts) parts[path] = X.serializeXml(P.tgt[path], P.source.parts[path]);
  const bytes = await X.buildRenderable(P.source.bytes, parts);
  await V.render(bytes, tgtRoot, styleHolder('tgt'), 'btgt');
  V.replaySplits(tgtRoot, P.splits);
  V.numberPages(tgtRoot);
  E.makeEditable(tgtRoot, { lang: P.project.targetLang, spellcheck: settings.native });
  P.info.clear();
  P.templates.clear();
  P.issues.clear();
  for (const pid of new Set($$('p[data-p]', tgtRoot).map(p => p.dataset.p))) {
    P.info.set(pid, E.runInfo(E.fragments(tgtRoot, pid, firstInstance(pid))));
  }
  paintHighlights();
}

const firstInstance = pid => tgtRoot.querySelector(`p[data-p="${CSS.escape(pid)}"]`);

function parseFragment(doc, xml) {
  const root = doc.documentElement;
  const decls = [...root.attributes].filter(a => a.name.startsWith('xmlns')).map(a => `${a.name}="${esc(a.value)}"`).join(' ');
  return X.parseXml(`<babel-wrap ${decls}>${xml}</babel-wrap>`).documentElement.firstElementChild;
}

function indexXml() {
  P.srcEl.clear(); P.tgtEl.clear();
  for (const path of P.project.textParts) {
    for (const el of P.src[path].querySelectorAll('[data-bp]')) P.srcEl.set(el.getAttribute('data-bp'), el);
    for (const el of P.tgt[path].querySelectorAll('[data-bp]')) P.tgtEl.set(el.getAttribute('data-bp'), el);
  }
}

async function closeProject() {
  if (!P) return;
  flushAll();
  await save();
  P.lock?.release();
  proofer.dirty.clear();
  P = null;
  srcRoot.innerHTML = tgtRoot.innerHTML = '';
  $('#docStyles').innerHTML = '';
  $('#banner').hidden = true;
  CSS.highlights?.clear();
}

// One tab at a time per translation. Opening it elsewhere takes over, and
// this tab stops editing instead of silently overwriting.
async function acquireLock(id) {
  if (!navigator.locks) return null;
  let release;
  const held = new Promise(r => { release = r; });
  await new Promise(resolve => {
    navigator.locks.request('babel:' + id, { steal: true }, () => { resolve(); return held; })
      .catch(e => {
        if (e.name !== 'AbortError' || !P || P.project.id !== id) return;
        flushAll();
        setReadOnly('This translation was opened in another tab or window. Editing continues there; this copy is paused.');
      });
  });
  return { release };
}

function setReadOnly(message) {
  P.readOnly = true;
  for (const h of $$('[contenteditable="true"]', tgtRoot)) h.contentEditable = 'false';
  const b = $('#banner');
  b.innerHTML = `<span>${esc(message)}</span> <button class="primary small" id="reclaim">Continue here instead</button>`;
  b.hidden = false;
  $('#reclaim').onclick = () => location.reload();
}

// ── Paragraph bookkeeping and statistics ───────────────────────────────────

function computeStats() {
  P.meta.clear();
  for (const path of P.project.textParts) {
    const kind = X.partKind(path);
    const counted = kind === 'main' || kind === 'footnotes' || kind === 'endnotes';
    for (const el of X.paragraphs(P.src[path])) {
      const pid = el.getAttribute('data-bp');
      const t = P.tgtEl.get(pid);
      if (!t) continue;
      const srcText = X.paraText(el);
      P.meta.set(pid, { path, kind, counted, srcText, tgtText: X.paraText(t), translatable: /\p{L}/u.test(srcText) });
    }
  }
  for (const [pid, m] of P.meta) refreshMeta(pid, m);
}

function refreshMeta(pid, m = P.meta.get(pid)) {
  m.edited = m.tgtText !== m.srcText;
  for (const f of $$(`p[data-p="${CSS.escape(pid)}"]`, tgtRoot)) {
    f.classList.toggle('b-edited', m.edited && !P.confirmed.has(pid));
    f.classList.toggle('b-done', P.confirmed.has(pid));
  }
}

function totals() {
  const t = { srcChars: 0, tgtChars: 0, srcWords: 0, tgtWords: 0, total: 0, done: 0 };
  for (const [pid, m] of P.meta) {
    if (m.counted) {
      t.srcChars += X.countChars(m.srcText); t.tgtChars += X.countChars(m.tgtText);
      t.srcWords += X.countWords(m.srcText); t.tgtWords += X.countWords(m.tgtText);
    }
    if (m.translatable) {
      t.total++;
      if (m.edited || P.confirmed.has(pid)) t.done++;
    }
  }
  return t;
}

let statusTimer = 0;
function updateStatus() {
  cancelAnimationFrame(statusTimer);
  statusTimer = requestAnimationFrame(() => {
    if (!P) return;
    const t = totals();
    P.project.stats = t;
    const lang = langByCode(P.project.targetLang);
    const diff = t.srcChars ? (t.tgtChars - t.srcChars) / t.srcChars * 100 : 0;
    const diffCls = Math.abs(diff) > 10 ? 'warn' : Math.abs(diff) > 3 ? 'note' : 'ok';
    $('#srcStatus').innerHTML = `
      <span class="tag">Original</span>
      <span title="Characters including spaces, as Word counts them (body, footnotes and endnotes). ${nf.format(t.srcWords)} words."><b>${nf.format(t.srcChars)}</b> characters with spaces</span>
      <span>${V.pages(srcRoot).length} pages</span>
      <span class="grow"></span>
      <span id="srcPara" class="para"></span>`;
    const counts = issueCounts();
    const over = $$('section.b-over', tgtRoot).length;
    $('#tgtStatus').innerHTML = `
      <span class="tag tgt">${esc(lang.name)}</span>
      <span title="Characters including spaces, as Word counts them (body, footnotes and endnotes). ${nf.format(t.tgtWords)} words."><b>${nf.format(t.tgtChars)}</b> characters with spaces</span>
      <span class="delta ${diffCls}" title="Length compared with the original">${pct.format(diff)} %</span>
      <span title="Paragraphs changed or confirmed, of those with text">${t.total ? Math.floor(100 * t.done / t.total) : 0} % done <small>(${t.done}/${t.total})</small></span>
      ${proofChip(counts)}
      ${over ? `<button class="chip bad" id="overChip" title="Pages that no longer fit — click to go to the next one">${over} page${over > 1 ? 's' : ''} overflow</button>` : ''}
      <span class="grow"></span>
      <span id="tgtPara" class="para"></span>`;
    $('#issueChip')?.addEventListener('click', openIssueMenu);
    $('#overChip')?.addEventListener('click', gotoOverflow);
    updateParaStatus();
  });
}

function proofChip(c) {
  if (!proofer.active) return `<span class="muted" title="${esc(proofState.detail || '')}">${esc(proofState.detail || 'No proofing')}</span>`;
  const n = c.spelling + c.grammar + c.style;
  const busy = proofState.state === 'busy' ? '<i class="dot busy"></i>' : proofState.state === 'error' ? '<i class="dot err"></i>' : '';
  if (!n) return `<span class="chip ok" title="${esc(proofState.detail || 'No spelling or grammar issues in edited paragraphs')}">${busy}No issues</span>`;
  return `<button class="chip issues" id="issueChip" title="${esc(proofState.detail || 'Show all issues')}">${busy}
    ${c.spelling ? `<i class="sw sp"></i>${c.spelling} spelling` : ''}
    ${c.grammar ? `<i class="sw gr"></i>${c.grammar} grammar` : ''}
    ${c.style ? `<i class="sw st"></i>${c.style} style` : ''}</button>`;
}

function updateParaStatus() {
  const pid = P?.active;
  const tp = $('#tgtPara'); const sp = $('#srcPara');
  if (!tp || !sp) return;
  const m = pid && P.meta.get(pid);
  if (!m) { tp.textContent = sp.textContent = ''; return; }
  const a = X.countChars(m.srcText); const b = X.countChars(m.tgtText);
  sp.textContent = `¶ ${nf.format(a)}`;
  const d = a ? (b - a) / a * 100 : 0;
  tp.innerHTML = `${P.confirmed.has(pid) ? '<span class="ok">✓ confirmed</span> · ' : ''}¶ ${nf.format(b)} <span class="delta ${Math.abs(d) > 15 ? 'warn' : 'ok'}">${a ? pct.format(d) + ' %' : ''}</span>`;
}

// ── Editing ────────────────────────────────────────────────────────────────

const editorCallbacks = {
  input(host) {
    if (!P || P.readOnly) return;
    const pid = host.dataset.p;
    dropIssuesAtCaret(pid);
    const pend = P.pending.get(pid);
    clearTimeout(pend?.timer);
    P.pending.set(pid, { host, timer: setTimeout(() => commit(pid), 120) });
  },
  focus(host) { setActive(host); },
  next: navigate,
  confirm(host) {
    const pid = host.dataset.p;
    flush(pid);
    if (P.confirmed.has(pid)) P.confirmed.delete(pid); else P.confirmed.add(pid);
    P.project.confirmed = [...P.confirmed];
    refreshMeta(pid);
    updateStatus();
    scheduleSave();
    if (P.confirmed.has(pid)) navigate(host, 1, 'todo');
  },
  blocked(msg) { toast(msg, 'warn'); },
};
E.attach(tgtRoot, editorCallbacks);
E.guardSelectionClicks(tgtRoot, { editable: true });
E.guardSelectionClicks(srcRoot, { editable: false });

function flush(pid) {
  const pend = P?.pending.get(pid);
  if (!pend) return;
  clearTimeout(pend.timer);
  commit(pid);
}
function flushAll() {
  if (!P) return;
  for (const pid of [...P.pending.keys()]) flush(pid);
}

// Writes one paragraph from the DOM into the target XML.
function commit(pid) {
  const pend = P.pending.get(pid);
  P.pending.delete(pid);
  if (!pend || P.readOnly) return;
  const el = P.tgtEl.get(pid);
  const info = P.info.get(pid);
  const m = P.meta.get(pid);
  if (!el || !info || !m) return;
  const host = pend.host.isConnected ? pend.host : firstInstance(pid);
  const frags = E.fragments(tgtRoot, pid, host);
  const { tokens } = E.readParagraph(frags);
  try {
    X.applyParagraph(el, tokens, info, P.templates);
  } catch (e) {
    console.error(e);
    toast('Could not store that edit: ' + e.message, 'error');
    return;
  }
  m.tgtText = X.paraText(el);
  const wasEdited = m.edited;
  refreshMeta(pid, m);
  P.dirty.add(m.path);
  P.changedSinceSnapshot = true;
  S.journalWrite(P.project.id, pid, new XMLSerializer().serializeToString(el));
  setSaveState('pending');
  scheduleSave();

  // Repeated headers/footers show the edit on every page.
  if (host.closest('header, footer')) {
    for (const other of $$(`p[data-p="${CSS.escape(pid)}"]`, tgtRoot)) if (other !== host) other.innerHTML = host.innerHTML;
    V.numberPages(tgtRoot);
  }
  if (host.querySelector('[data-k="tab"]')) V.refreshTabs(host);
  if (m.edited) proofer.queue(pid);
  else if (wasEdited) { P.issues.delete(pid); paintHighlights(); }
  scheduleOverflow(frags);
  updateStatus();
}

let saveTimer = 0, saveFirstPending = 0, saving = null, saveAgain = false;
function scheduleSave(delay = 900) {
  if (!P) return;
  clearTimeout(saveTimer);
  saveFirstPending ||= Date.now();
  const wait = Math.max(0, Math.min(delay, saveFirstPending + 4000 - Date.now()));
  saveTimer = setTimeout(save, wait);
}

async function save() {
  if (!P || P.readOnly) return;
  if (saving) { saveAgain = true; return saving; }
  clearTimeout(saveTimer);
  saveFirstPending = 0;
  const session = P;
  const seq = S.journalRead(P.project.id).seq;
  const paths = [...P.dirty];
  P.dirty.clear();
  const parts = {};
  for (const path of paths) parts[path] = X.serializeXml(P.tgt[path], P.source.parts[path]);
  P.project.updatedAt = Date.now();
  P.project.stats = totals();
  setSaveState('saving');
  saving = (async () => {
    try {
      await S.saveParts(session.project, parts);
      S.journalCommit(session.project.id, seq);
      if (P === session) setSaveState(P.dirty.size || P.pending.size ? 'pending' : 'saved', Date.now());
    } catch (e) {
      for (const p of paths) session.dirty.add(p);
      console.error(e);
      if (P === session) setSaveState('error', e.message);
    }
  })();
  await saving;
  saving = null;
  if (saveAgain) { saveAgain = false; scheduleSave(200); }
  if (P === session && P.changedSinceSnapshot && Date.now() - P.lastSnapshotAt > 180000) snapshot('auto');
}

function setSaveState(state, detail) {
  const el = $('#saveState');
  el.dataset.state = state;
  el.textContent = state === 'saved' ? `Saved ${clock(detail)}`
    : state === 'saving' ? 'Saving…'
      : state === 'pending' ? 'Saving…'
        : `Not saved — ${detail}`;
  el.title = state === 'error'
    ? 'Edits are still kept in the recovery journal. Download a copy to be safe.'
    : 'Saved in this browser’s storage. Every edit is journalled immediately.';
}

async function snapshot(reason) {
  if (!P) return;
  const session = P;
  flushAll();
  const parts = {};
  for (const path of P.project.textParts) parts[path] = X.serializeXml(P.tgt[path], P.source.parts[path]);
  P.lastSnapshotAt = Date.now();
  P.changedSinceSnapshot = false;
  try { await S.addSnapshot(session.project.id, reason, parts, totals()); }
  catch (e) { console.warn('snapshot failed', e); }
}

addEventListener('keydown', e => {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
    e.preventDefault();
    if (P) { flushAll(); save(); }
  }
  if (e.key === 'Escape') hideIssue();
});
addEventListener('pagehide', () => { flushAll(); save(); });
document.addEventListener('visibilitychange', () => { if (document.hidden) { flushAll(); save(); } });
setInterval(() => { if (P?.changedSinceSnapshot && Date.now() - P.lastSnapshotAt > 180000) snapshot('auto'); }, 60000);

// ── Navigation and focus ───────────────────────────────────────────────────

function setActive(host) {
  if (!P) return;
  const pid = host.dataset.p;
  if (P.active === pid) return;
  if (P.active) flush(P.active);
  P.active = pid;
  for (const el of $$('.b-active')) el.classList.remove('b-active');
  host.classList.add('b-active');
  for (const f of $$(`p[data-p="${CSS.escape(pid)}"]`, srcRoot)) f.classList.add('b-active');
  updateParaStatus();
}

function navigable(dir, from, mode) {
  let list = $$('p[data-p]', tgtRoot).filter(p => p.isContentEditable);
  if (mode !== 'spatial' && mode !== 'adjacent') {
    const seen = new Set();
    list = list.filter(p => {
      if (seen.has(p.dataset.p)) return false;
      seen.add(p.dataset.p);
      return true;
    });
    if (!list.includes(from)) list.splice(0, 0, from);
  }
  const i = list.indexOf(from);
  const ordered = dir > 0 ? list.slice(i + 1) : list.slice(0, Math.max(0, i)).reverse();
  return ordered;
}

function navigate(host, dir, mode, x) {
  let candidates = navigable(dir, host, mode);
  if (mode === 'todo') {
    candidates = candidates.filter(p => {
      const m = P.meta.get(p.dataset.p);
      return m?.translatable && !m.edited && !P.confirmed.has(p.dataset.p);
    });
    if (!candidates.length) { toast(dir > 0 ? 'Nothing left to translate below.' : 'Nothing left to translate above.'); return true; }
  }
  if (mode === 'paragraph') candidates = candidates.filter(p => !p.closest('header, footer') || host.closest('header, footer'));
  const next = candidates[0];
  if (!next) return false;
  if (mode === 'spatial' && x != null) {
    const r = next.getBoundingClientRect();
    next.scrollIntoView({ block: 'nearest' });
    const r2 = next.getBoundingClientRect();
    E.placeCaret(next, { x, y: dir > 0 ? r2.top + 2 : r2.bottom - 2 });
    void r;
  } else {
    E.placeCaret(next, dir > 0 || mode !== 'adjacent' ? 'start' : 'end');
  }
  ensureVisible(next);
  return true;
}

function ensureVisible(el) {
  const sc = $('#tgtScroll');
  const r = el.getBoundingClientRect();
  const v = sc.getBoundingClientRect();
  if (r.top < v.top + 40 || r.bottom > v.bottom - 40) {
    sc.scrollBy({ top: r.top - v.top - v.height * 0.3, behavior: 'smooth' });
  }
}

// Clicking a paragraph in the original jumps to its translation.
srcRoot.addEventListener('click', e => {
  const p = e.target.closest('p[data-p]');
  if (!p || !P) return;
  const pid = p.dataset.p;
  const frags = $$(`p[data-p="${CSS.escape(pid)}"]`, srcRoot);
  const idx = Math.max(0, frags.indexOf(p));
  let target = $$(`p[data-p="${CSS.escape(pid)}"]`, tgtRoot)[idx];
  if (p.closest('header, footer')) {
    const page = V.pages(srcRoot).indexOf(p.closest('section'));
    target = V.pages(tgtRoot)[page]?.querySelector(`p[data-p="${CSS.escape(pid)}"]`) ?? target;
  }
  if (!target) return;
  E.placeCaret(target, 'start');
  ensureVisible(target);
});

// ── Synchronised scrolling ─────────────────────────────────────────────────

let syncing = null;
function anchorAt(root, scroller) {
  const v = scroller.getBoundingClientRect();
  for (const dy of [12, 40, 80, 140, 220]) {
    for (const fx of [0.5, 0.3, 0.7]) {
      const el = document.elementFromPoint(v.left + v.width * fx, v.top + dy);
      if (!el || !root.contains(el)) continue;
      const p = el.closest('p[data-p]');
      const sec = el.closest('section');
      if (p && !p.closest('header, footer')) return { p, sec };
      if (sec) return { sec };
    }
  }
  return null;
}

function syncFrom(fromRoot, fromSc, toRoot, toSc) {
  if (!P || !settings.sync) return;
  const a = anchorAt(fromRoot, fromSc);
  if (!a) return;
  const v = fromSc.getBoundingClientRect();
  let src = a.p, dst = null;
  if (src) {
    const pid = src.dataset.p;
    const idx = $$(`p[data-p="${CSS.escape(pid)}"]`, fromRoot).indexOf(src);
    dst = $$(`p[data-p="${CSS.escape(pid)}"]`, toRoot)[idx];
  }
  if (!dst && a.sec) {
    src = a.sec;
    dst = V.pages(toRoot)[V.pages(fromRoot).indexOf(a.sec)];
  }
  if (!dst) return;
  const rs = src.getBoundingClientRect();
  const frac = rs.height ? (v.top - rs.top) / rs.height : 0;
  const rd = dst.getBoundingClientRect();
  const tv = toSc.getBoundingClientRect();
  const delta = (rd.top + frac * rd.height) - tv.top;
  if (Math.abs(delta) < 1) return;
  syncing = toSc;
  toSc.scrollTop += delta;
  // horizontal position mirrors proportionally
  const fmax = fromSc.scrollWidth - fromSc.clientWidth;
  const tmax = toSc.scrollWidth - toSc.clientWidth;
  if (fmax > 0 && tmax > 0) toSc.scrollLeft = fromSc.scrollLeft / fmax * tmax;
}

for (const [fromRoot, fromSc, toRoot, toSc] of [
  [srcRoot, $('#srcScroll'), tgtRoot, $('#tgtScroll')],
  [tgtRoot, $('#tgtScroll'), srcRoot, $('#srcScroll')],
]) {
  let raf = 0;
  fromSc.addEventListener('scroll', () => {
    if (syncing === fromSc) { syncing = null; return; }
    cancelAnimationFrame(raf);
    raf = requestAnimationFrame(() => syncFrom(fromRoot, fromSc, toRoot, toSc));
    hideIssue();
  }, { passive: true });
}
$('#syncScroll').onchange = e => {
  settings.sync = e.target.checked; saveSettings();
  if (settings.sync) syncFrom(tgtRoot, $('#tgtScroll'), srcRoot, $('#srcScroll'));
};

// ── Zoom ───────────────────────────────────────────────────────────────────

const ZOOMS = [0.5, 0.67, 0.75, 0.85, 1, 1.1, 1.25, 1.5, 1.75, 2];
function fitZoom() {
  const w = $('#tgtScroll').clientWidth - 24;
  return Math.max(0.3, Math.min(2, w / (V.pageWidth(tgtRoot) + 48)));
}
function applyZoom(z) {
  settings.zoom = z; saveSettings();
  const v = z === 'fit' ? fitZoom() : z;
  srcZoom.set(v); tgtZoom.set(v);
  $('#zoomVal').textContent = `${Math.round(v * 100)} %`;
  $('#zoomVal').classList.toggle('on', z === 'fit');
}
const currentZoom = () => (settings.zoom === 'fit' ? fitZoom() : settings.zoom);
$('#zoomIn').onclick = () => applyZoom(ZOOMS.find(z => z > currentZoom() + 0.001) ?? 2);
$('#zoomOut').onclick = () => applyZoom([...ZOOMS].reverse().find(z => z < currentZoom() - 0.001) ?? 0.5);
$('#zoomVal').onclick = () => applyZoom('fit');
addEventListener('resize', () => { if (P && settings.zoom === 'fit') applyZoom('fit'); });
addEventListener('wheel', e => {
  if (!P || !(e.ctrlKey || e.metaKey) || !e.target.closest?.('.scroll')) return;
  e.preventDefault();
  (e.deltaY < 0 ? $('#zoomIn') : $('#zoomOut')).click();
}, { passive: false });

// ── Page overflow ──────────────────────────────────────────────────────────

let overflowTimer = 0;
const overflowQueue = new Set();
function scheduleOverflow(frags) {
  for (const f of frags) { const s = f.closest('section'); if (s) overflowQueue.add(s); }
  clearTimeout(overflowTimer);
  overflowTimer = setTimeout(() => {
    const tp = V.pages(tgtRoot);
    const changed = checkOverflow([...overflowQueue].map(s => tp.indexOf(s)).filter(i => i >= 0));
    overflowQueue.clear();
    if (changed) updateStatus();
  }, 250);
}

function checkOverflow(indexes) {
  const sp = V.pages(srcRoot);
  const tp = V.pages(tgtRoot);
  let changed = false;
  for (const i of indexes ?? tp.keys()) {
    const t = tp[i];
    const over = V.overflowOf(sp[i], t);
    const was = t.classList.contains('b-over');
    if (over > 2) {
      const lines = Math.max(1, Math.round(over / V.lineHeightOf(t)));
      t.classList.add('b-over');
      t.dataset.over = `Overflows by about ${lines} line${lines > 1 ? 's' : ''}`;
    } else {
      t.classList.remove('b-over');
      delete t.dataset.over;
    }
    changed ||= was !== t.classList.contains('b-over');
  }
  return changed;
}

function gotoOverflow() {
  const over = $$('section.b-over', tgtRoot);
  if (!over.length) return;
  const v = $('#tgtScroll').getBoundingClientRect();
  const next = over.find(s => s.getBoundingClientRect().top > v.top + 10) ?? over[0];
  $('#tgtScroll').scrollBy({ top: next.getBoundingClientRect().top - v.top - 12, behavior: 'smooth' });
}

// ── Proofing ───────────────────────────────────────────────────────────────

let proofState = { state: 'idle', detail: '' };
const proofer = new Proofer({
  getText(pid) {
    if (!P) return null;
    const m = P.meta.get(pid);
    const host = firstInstance(pid);
    if (!m?.edited || !host) return null;
    return E.readParagraph(E.fragments(tgtRoot, pid, host)).text;
  },
  onResult(pid, text, matches) {
    if (!P) return;
    const host = firstInstance(pid);
    if (!host) return;
    const { text: now, segs } = E.readParagraph(E.fragments(tgtRoot, pid, host));
    if (now !== text) { proofer.queue(pid, 800); return; }
    const list = [];
    for (const m of matches) {
      const word = text.slice(m.offset, m.offset + m.length);
      if (m.kind === 'spelling' && dictionary.has(word)) continue;
      // Words still in the source language, names and terms shared by both
      // languages: whatever also occurs in the original paragraph is not a typo.
      if (m.kind === 'spelling' && srcWords(pid).has(word.toLowerCase())) continue;
      if (P.ignored.has(`${m.rule}|${word}`)) continue;
      const range = E.rangeFor(segs, m.offset, m.offset + m.length);
      if (range) list.push({ range, m, word, pid });
    }
    if (list.length) P.issues.set(pid, list); else P.issues.delete(pid);
    paintHighlights();
    updateStatus();
  },
  onStatus(state, detail) {
    proofState = { state, detail };
    updateStatus();
  },
});

function srcWords(pid) {
  const text = P.meta.get(pid)?.srcText ?? '';
  return new Set((text.toLowerCase().match(/[\p{L}\p{M}\p{N}'’-]+/gu) ?? []));
}

function configureProofer() {
  const lang = langByCode(P.project.targetLang);
  proofer.configure({
    enabled: settings.proof,
    lang: lang.lt,
    endpoint: settings.endpoint || 'https://api.languagetool.org',
    username: settings.user,
    apiKey: settings.key,
    picky: settings.picky,
  });
  if (!proofer.active) { P.issues.clear(); paintHighlights(); }
}

function issueCounts() {
  const c = { spelling: 0, grammar: 0, style: 0 };
  if (!P) return c;
  for (const list of P.issues.values()) for (const i of list) {
    if (i.m.kind === 'spelling') c.spelling++;
    else if (i.m.kind === 'grammar') c.grammar++;
    else c.style++;
  }
  return c;
}

function paintHighlights() {
  if (!globalThis.Highlight || !CSS.highlights) return;
  const groups = { spelling: [], grammar: [], style: [] };
  for (const list of P?.issues.values() ?? []) for (const i of list) {
    if (i.range.collapsed) continue;
    (groups[i.m.kind] ?? groups.style).push(i.range);
  }
  CSS.highlights.set('b-spelling', new Highlight(...groups.spelling));
  CSS.highlights.set('b-grammar', new Highlight(...groups.grammar));
  CSS.highlights.set('b-style', new Highlight(...groups.style));
}

// Ranges are live and survive edits elsewhere in the paragraph; only the
// issue under the caret goes stale while typing.
function dropIssuesAtCaret(pid) {
  const list = P.issues.get(pid);
  if (!list) return;
  const sel = getSelection();
  if (!sel.rangeCount) return;
  const c = sel.getRangeAt(0);
  const keep = list.filter(i => {
    if (i.range.collapsed) return false;
    try {
      return !(i.range.comparePoint(c.startContainer, c.startOffset) === 0
        || (c.startContainer === i.range.endContainer && c.startOffset === i.range.endOffset + 1));
    } catch { return false; }
  });
  if (keep.length !== list.length) {
    if (keep.length) P.issues.set(pid, keep); else P.issues.delete(pid);
    paintHighlights();
  }
  hideIssue();
}

function issueAtCaret() {
  const sel = getSelection();
  if (!P || !sel.rangeCount) return null;
  const c = sel.getRangeAt(0);
  const host = E.hostOf(c.startContainer);
  if (!host) return null;
  for (const i of P.issues.get(host.dataset.p) ?? []) {
    try {
      if (!i.range.collapsed && i.range.comparePoint(c.startContainer, c.startOffset) === 0 && E.hostOf(i.range.startContainer) === host) return i;
    } catch { /* detached */ }
  }
  return null;
}

tgtRoot.addEventListener('mouseup', () => setTimeout(showIssueAtCaret, 0));
tgtRoot.addEventListener('keyup', e => { if (e.key.startsWith('Arrow') || e.key === 'Home' || e.key === 'End') showIssueAtCaret(); });

function showIssueAtCaret() {
  const sel = getSelection();
  if (!sel.isCollapsed) return hideIssue();
  const i = issueAtCaret();
  if (!i) return hideIssue();
  showIssue(i);
}

function showIssue(i) {
  const pop = $('#issuePop');
  const reps = i.m.replacements.length
    ? i.m.replacements.map((r, k) => `<button class="rep" data-k="${k}">${esc(r) || '<em>delete</em>'}</button>`).join('')
    : '';
  pop.innerHTML = `
    <div class="pop-kind ${i.m.kind}">${esc({ spelling: 'Spelling', grammar: 'Grammar', style: 'Style', typography: 'Typography' }[i.m.kind] ?? 'Note')}</div>
    <p>${esc(i.m.message)}</p>
    ${reps ? `<div class="reps">${reps}</div>` : ''}
    <div class="pop-actions">
      <button class="ghost small" data-act="ignore">Ignore</button>
      ${i.m.kind === 'spelling' ? '<button class="ghost small" data-act="dict">Add to dictionary</button>' : ''}
    </div>`;
  const r = i.range.getBoundingClientRect();
  pop.hidden = false;
  const w = pop.offsetWidth, h = pop.offsetHeight;
  pop.style.left = `${Math.max(8, Math.min(innerWidth - w - 8, r.left))}px`;
  pop.style.top = `${r.bottom + h + 12 > innerHeight ? r.top - h - 6 : r.bottom + 6}px`;
  pop.onmousedown = e => e.preventDefault(); // keep the caret in the text
  pop.onclick = e => {
    const b = e.target.closest('button');
    if (!b) return;
    if (b.classList.contains('rep')) applyReplacement(i, i.m.replacements[+b.dataset.k]);
    else if (b.dataset.act === 'ignore') {
      P.ignored.add(`${i.m.rule}|${i.word}`);
      P.project.ignored = [...P.ignored];
      removeIssue(i); scheduleSave();
    } else if (b.dataset.act === 'dict') {
      dictionary.add(i.word); writeJSON(DICT_KEY, [...dictionary]);
      for (const [pid, list] of P.issues) {
        const keep = list.filter(x => !(x.m.kind === 'spelling' && x.word === i.word));
        if (keep.length) P.issues.set(pid, keep); else P.issues.delete(pid);
      }
      paintHighlights(); updateStatus();
    }
    hideIssue();
  };
}

function hideIssue() { $('#issuePop').hidden = true; $('#issueMenu').hidden = true; }

function removeIssue(i) {
  const list = (P.issues.get(i.pid) ?? []).filter(x => x !== i);
  if (list.length) P.issues.set(i.pid, list); else P.issues.delete(i.pid);
  paintHighlights(); updateStatus();
}

function applyReplacement(i, value) {
  const host = E.hostOf(i.range.startContainer);
  if (!host || P.readOnly) return;
  host.focus({ preventScroll: true });
  const sel = getSelection();
  sel.removeAllRanges();
  sel.addRange(i.range.cloneRange());
  removeIssue(i);
  if (value) document.execCommand('insertText', false, value);
  else document.execCommand('delete');
}

function openIssueMenu(e) {
  const menu = $('#issueMenu');
  const items = [];
  const order = $$('p[data-p]', tgtRoot).map(p => p.dataset.p);
  const seen = new Set();
  for (const pid of order) {
    if (seen.has(pid)) continue;
    seen.add(pid);
    for (const i of P.issues.get(pid) ?? []) items.push(i);
  }
  menu.innerHTML = `<div class="menu-head">${items.length} issue${items.length === 1 ? '' : 's'} in edited paragraphs</div>` + items.slice(0, 200).map((i, k) => {
    const ctx = i.range.startContainer.data ?? '';
    const s = i.range.startOffset;
    return `<button class="menu-item" data-k="${k}"><i class="sw ${i.m.kind === 'spelling' ? 'sp' : i.m.kind === 'grammar' ? 'gr' : 'st'}"></i>
      <span><b>${esc(i.word || '…')}</b> <small>${esc(ctx.slice(Math.max(0, s - 30), s))}<u>${esc(ctx.slice(s, s + i.word.length))}</u>${esc(ctx.slice(s + i.word.length, s + i.word.length + 30))}</small><br><small class="muted">${esc(i.m.message)}</small></span></button>`;
  }).join('');
  const r = e.currentTarget.getBoundingClientRect();
  menu.hidden = false;
  menu.style.left = `${Math.max(8, Math.min(innerWidth - menu.offsetWidth - 8, r.left))}px`;
  menu.style.top = `${r.top - menu.offsetHeight - 6}px`;
  menu.onclick = ev => {
    const b = ev.target.closest('.menu-item');
    if (!b) return;
    const i = items[+b.dataset.k];
    menu.hidden = true;
    const host = E.hostOf(i.range.startContainer);
    if (!host) return;
    host.focus({ preventScroll: true });
    const sel = getSelection();
    sel.removeAllRanges();
    const c = i.range.cloneRange(); c.collapse(true);
    sel.addRange(c);
    ensureVisible(host);
    setTimeout(() => showIssue(i), 350);
  };
  e.stopPropagation();
}
addEventListener('mousedown', e => {
  if (!e.target.closest('#issueMenu, #issueChip, #issuePop')) { $('#issueMenu').hidden = true; }
  if (!e.target.closest('#issuePop') && !e.target.closest('#tgtDoc')) $('#issuePop').hidden = true;
});

// ── Paragraph actions: revert ──────────────────────────────────────────────

tgtRoot.addEventListener('contextmenu', e => {
  const host = E.hostOf(e.target);
  if (!host || !P || P.readOnly) return;
  const pid = host.dataset.p;
  const m = P.meta.get(pid);
  if (!m) return;
  e.preventDefault();
  const menu = $('#issueMenu');
  menu.innerHTML = `
    <button class="menu-item" data-a="confirm"><span>${P.confirmed.has(pid) ? 'Unmark as done' : 'Mark as done'} <small class="muted">Ctrl+Enter</small></span></button>
    <button class="menu-item" data-a="revert" ${m.edited ? '' : 'disabled'}><span>Revert paragraph to the original</span></button>
    <button class="menu-item" data-a="copy"><span>Copy the original text</span></button>`;
  menu.hidden = false;
  menu.style.left = `${Math.min(innerWidth - menu.offsetWidth - 8, e.clientX)}px`;
  menu.style.top = `${Math.min(innerHeight - menu.offsetHeight - 8, e.clientY)}px`;
  menu.onclick = ev => {
    const b = ev.target.closest('.menu-item');
    if (!b || b.disabled) return;
    menu.hidden = true;
    if (b.dataset.a === 'confirm') editorCallbacks.confirm(host);
    if (b.dataset.a === 'revert') revertParagraph(pid);
    if (b.dataset.a === 'copy') navigator.clipboard?.writeText(m.srcText.replace(/\n/g, ' ')).then(() => toast('Original text copied.'));
  };
});

function revertParagraph(pid) {
  flush(pid);
  const src = P.srcEl.get(pid);
  const tgt = P.tgtEl.get(pid);
  if (!src || !tgt) return;
  const before = { xml: tgt.cloneNode(true), html: $$(`p[data-p="${CSS.escape(pid)}"]`, tgtRoot).map(f => f.innerHTML) };
  swapParagraph(pid, src, $$(`p[data-p="${CSS.escape(pid)}"]`, srcRoot).map(f => f.innerHTML));
  toast('Paragraph reverted to the original.', 'info', 8000, {
    label: 'Undo',
    run: () => swapParagraph(pid, before.xml, before.html),
  });
}

function swapParagraph(pid, xmlEl, htmlList) {
  const tgt = P.tgtEl.get(pid);
  const copy = X.replaceParagraph(tgt, xmlEl);
  P.tgtEl.set(pid, copy);
  const frags = $$(`p[data-p="${CSS.escape(pid)}"]`, tgtRoot);
  frags.forEach((f, k) => {
    f.innerHTML = htmlList[Math.min(k, htmlList.length - 1)] ?? '';
    for (const s of f.querySelectorAll('span[data-r]:not([data-k="t"])')) s.contentEditable = 'false';
    for (const a of f.querySelectorAll('a[href]')) a.removeAttribute('href');
  });
  for (const r of copy.querySelectorAll('[data-br]')) P.templates.delete(r.getAttribute('data-br'));
  P.info.set(pid, E.runInfo(E.fragments(tgtRoot, pid, frags[0])));
  const m = P.meta.get(pid);
  m.tgtText = X.paraText(copy);
  refreshMeta(pid, m);
  P.issues.delete(pid);
  paintHighlights();
  P.dirty.add(m.path);
  P.changedSinceSnapshot = true;
  S.journalWrite(P.project.id, pid, new XMLSerializer().serializeToString(copy));
  if (m.edited) proofer.queue(pid, 300);
  scheduleSave(200);
  scheduleOverflow(frags);
  updateStatus();
}

// ── Download ───────────────────────────────────────────────────────────────

const LANG_SUFFIX = code => code.split('-')[0].toUpperCase();

$('#btnDownload').onclick = async () => {
  if (!P) return;
  flushAll();
  const btn = $('#btnDownload');
  btn.disabled = true;
  try {
    const parts = {};
    for (const path of P.project.textParts) parts[path] = X.serializeXml(P.tgt[path], P.source.parts[path]);
    const blob = await X.buildDocx({ bytes: P.source.bytes, partXml: parts, lang: P.project.targetLang });
    saveBlob(blob, `${P.project.name}_${LANG_SUFFIX(P.project.targetLang)}.docx`);
    await save();
    snapshot('download');
  } catch (e) { fatal(e); }
  finally { btn.disabled = false; }
};

async function downloadProject(id, snapshotParts = null, suffix = '') {
  const project = await S.getProject(id);
  const source = await S.getSource(id);
  const stored = snapshotParts ?? await S.getParts(id);
  const parts = {};
  for (const path of project.textParts) parts[path] = stored[path] ?? source.parts[path];
  // Unsaved journal entries belong in the file too.
  if (!snapshotParts) {
    const j = S.journalRead(id);
    if (Object.keys(j.entries).length) {
      for (const path of project.textParts) {
        const doc = X.parseXml(parts[path]);
        let touched = false;
        for (const [pid, e] of Object.entries(j.entries)) {
          if (X.partKey(path) !== pid.split(':')[0]) continue;
          const el = doc.querySelector(`[data-bp="${CSS.escape(pid)}"]`);
          if (el) { el.replaceWith(doc.importNode(parseFragment(doc, e.xml), true)); touched = true; }
        }
        if (touched) parts[path] = X.serializeXml(doc, parts[path]);
      }
    }
  }
  const blob = await X.buildDocx({ bytes: source.bytes, partXml: parts, lang: project.targetLang });
  saveBlob(blob, `${project.name}_${LANG_SUFFIX(project.targetLang)}${suffix}.docx`);
}

function saveBlob(blob, name) {
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(a.href), 60000);
}

// ── Versions ───────────────────────────────────────────────────────────────

$('#btnVersions').onclick = () => { if (P) openVersions(); };
$('#versionSave').onclick = async () => {
  const name = $('#versionName').value.trim() || 'saved by you';
  $('#versionName').value = '';
  await snapshot(name);
  openVersions();
};

async function openVersions() {
  flushAll();
  await save();
  const dlg = $('#versionsDlg');
  const list = $('#versionList');
  const snaps = await S.listSnapshots(P.project.id);
  list.innerHTML = snaps.length ? '' : '<li class="muted">No versions yet. The first one is made after a few minutes of work.</li>';
  for (const s of snaps) {
    const st = s.stats ?? {};
    const li = document.createElement('li');
    li.innerHTML = `
      <span class="v-when"><b>${esc(new Date(s.time).toLocaleString())}</b><small>${esc(s.reason === 'auto' ? 'automatic' : s.reason)}</small></span>
      <span class="v-stats">${st.total ? Math.floor(100 * st.done / st.total) : 0} % · ${nf.format(st.tgtChars ?? 0)} chars</span>
      <button type="button" class="ghost small" data-a="dl">Download</button>
      <button type="button" class="ghost small" data-a="restore">Restore</button>`;
    li.querySelector('[data-a=dl]').onclick = () => downloadProject(P.project.id, s.parts, '_' + new Date(s.time).toISOString().slice(0, 16).replace(/[:T]/g, '-')).catch(fatal);
    li.querySelector('[data-a=restore]').onclick = async () => {
      if (!confirm(`Restore the version from ${new Date(s.time).toLocaleString()}? The current state is kept as a version first.`)) return;
      dlg.close();
      await restoreParts(s.parts, 'before restore');
    };
    list.appendChild(li);
  }
  const info = await S.storageInfo();
  $('#versionStorage').textContent = info ? `Browser storage in use: ${fmtBytes(info.usage)}${info.persisted ? ' (persistent)' : ''}.` : '';
  if (!dlg.open) dlg.showModal();
}

async function restoreParts(parts, reason) {
  const id = P.project.id;
  await snapshot(reason);
  await S.saveParts(P.project, parts);
  S.journalClear(id);
  P.dirty.clear();
  await closeProject();
  await openProject(id);
  toast('Version restored.');
}

// ── Settings ───────────────────────────────────────────────────────────────

$('#btnSettings').onclick = () => {
  $('#setProof').checked = settings.proof;
  $('#setPicky').checked = settings.picky;
  $('#setNative').checked = settings.native;
  $('#setEndpoint').value = settings.endpoint;
  $('#setUser').value = settings.user;
  $('#setKey').value = settings.key;
  $('#setMarks').checked = settings.marks;
  $('#setDict').value = [...dictionary].sort().join('\n');
  const dlg = $('#settingsDlg');
  dlg.returnValue = '';
  dlg.showModal();
  dlg.onclose = () => {
    if (dlg.returnValue !== 'ok') return;
    Object.assign(settings, {
      proof: $('#setProof').checked,
      picky: $('#setPicky').checked,
      native: $('#setNative').checked,
      endpoint: $('#setEndpoint').value.trim() || 'https://api.languagetool.org',
      user: $('#setUser').value.trim(),
      key: $('#setKey').value.trim(),
      marks: $('#setMarks').checked,
    });
    saveSettings();
    dictionary = new Set($('#setDict').value.split('\n').map(s => s.trim()).filter(Boolean));
    writeJSON(DICT_KEY, [...dictionary]);
    if (!P) return;
    setMarks();
    for (const p of $$('p[data-p]', tgtRoot)) p.spellcheck = settings.native;
    configureProofer();
    P.issues.clear(); paintHighlights();
    for (const [pid, m] of P.meta) if (m.edited) proofer.queue(pid, 0);
    updateStatus();
  };
};

function setMarks() { tgtRoot.classList.toggle('b-marks', settings.marks); }

$('#btnHelp').onclick = () => $('#helpDlg').showModal();

$('#langPill').onclick = async () => {
  if (!P) return;
  const lang = await askLanguage({ title: 'Target language', sub: 'Used for spelling and grammar, and set as the language of the downloaded document.', current: P.project.targetLang, ok: 'Change' });
  if (!lang || lang === P.project.targetLang) return;
  P.project.targetLang = lang;
  settings.lastLang = lang; saveSettings();
  tgtRoot.lang = lang;
  updateLangPill();
  configureProofer();
  P.issues.clear(); paintHighlights();
  for (const [pid, m] of P.meta) if (m.edited) proofer.queue(pid, 0);
  S.saveProjectMeta(P.project);
  updateStatus();
};

function updateLangPill() {
  $('#langPill').textContent = `→ ${langByCode(P.project.targetLang).name}`;
}

// ── Toasts ─────────────────────────────────────────────────────────────────

let toastTimer = 0;
function toast(msg, kind = 'info', ms = 4000, action = null) {
  const t = $('#toast');
  t.className = 'toast ' + kind;
  t.innerHTML = `<span>${esc(msg)}</span>`;
  if (action) {
    const b = document.createElement('button');
    b.textContent = action.label;
    b.onclick = () => { action.run(); t.hidden = true; };
    t.appendChild(b);
  }
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { t.hidden = true; }, ms);
}

// ── Formatting helpers ─────────────────────────────────────────────────────

function clock(t) {
  return t ? new Date(t).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit', second: '2-digit' }) : '';
}
function timeAgo(t) {
  const s = (Date.now() - t) / 1000;
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return new Date(t).toLocaleDateString();
}
function fmtBytes(n) {
  if (n == null) return '?';
  return n > 1e9 ? `${(n / 1e9).toFixed(1)} GB` : n > 1e6 ? `${(n / 1e6).toFixed(1)} MB` : `${Math.round(n / 1e3)} kB`;
}

// Test hook (used by the automated browser tests).
globalThis.__babel = { get P() { return P; }, X, save, flushAll, snapshot };
