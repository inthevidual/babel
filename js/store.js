// Persistence. Three layers, so that no keystroke depends on one mechanism:
//   1. a synchronous localStorage journal of every paragraph as it is edited,
//   2. IndexedDB with the full translated XML parts (debounced),
//   3. IndexedDB snapshots — rolling versions that can be restored or downloaded.

const DB_NAME = 'babel';
const DB_VERSION = 1;
let dbPromise = null;

function db() {
  dbPromise ??= new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, DB_VERSION);
    req.onupgradeneeded = () => {
      const d = req.result;
      d.createObjectStore('projects', { keyPath: 'id' });
      d.createObjectStore('sources', { keyPath: 'id' });
      d.createObjectStore('parts', { keyPath: 'key' });
      const snaps = d.createObjectStore('snapshots', { keyPath: 'sid', autoIncrement: true });
      snaps.createIndex('byProject', 'id');
    };
    req.onsuccess = () => {
      req.result.onversionchange = () => req.result.close();
      resolve(req.result);
    };
    req.onerror = () => reject(req.error);
    req.onblocked = () => reject(new Error('The database is blocked by another Babel tab. Close other tabs and reload.'));
  });
  return dbPromise;
}

const done = tx => new Promise((resolve, reject) => {
  tx.oncomplete = () => resolve();
  tx.onerror = () => reject(tx.error);
  tx.onabort = () => reject(tx.error ?? new Error('Transaction aborted'));
});
const result = req => new Promise((resolve, reject) => {
  req.onsuccess = () => resolve(req.result);
  req.onerror = () => reject(req.error);
});

export async function requestPersistence() {
  try {
    if (navigator.storage?.persist && !(await navigator.storage.persisted())) await navigator.storage.persist();
  } catch { /* not fatal */ }
}

export async function storageInfo() {
  try {
    const [est, persisted] = await Promise.all([navigator.storage.estimate(), navigator.storage.persisted()]);
    return { usage: est.usage, quota: est.quota, persisted };
  } catch { return null; }
}

export async function listProjects() {
  const tx = (await db()).transaction('projects');
  const all = await result(tx.objectStore('projects').getAll());
  return all.sort((a, b) => b.updatedAt - a.updatedAt);
}

export async function getProject(id) {
  return result((await db()).transaction('projects').objectStore('projects').get(id));
}

export async function createProject(project, source) {
  const tx = (await db()).transaction(['projects', 'sources'], 'readwrite');
  tx.objectStore('projects').put(project);
  tx.objectStore('sources').put(source);
  await done(tx);
}

export async function getSource(id) {
  return result((await db()).transaction('sources').objectStore('sources').get(id));
}

export async function getParts(id) {
  const tx = (await db()).transaction('parts');
  const range = IDBKeyRange.bound(id + '|', id + '|￿');
  const rows = await result(tx.objectStore('parts').getAll(range));
  return Object.fromEntries(rows.map(r => [r.path, r.xml]));
}

export async function saveParts(project, parts) {
  const tx = (await db()).transaction(['projects', 'parts'], 'readwrite');
  for (const [path, xml] of Object.entries(parts))
    tx.objectStore('parts').put({ key: `${project.id}|${path}`, id: project.id, path, xml });
  tx.objectStore('projects').put(project);
  await done(tx);
}

export async function saveProjectMeta(project) {
  const tx = (await db()).transaction('projects', 'readwrite');
  tx.objectStore('projects').put(project);
  await done(tx);
}

export async function deleteProject(id) {
  const d = await db();
  const tx = d.transaction(['projects', 'sources', 'parts', 'snapshots'], 'readwrite');
  tx.objectStore('projects').delete(id);
  tx.objectStore('sources').delete(id);
  tx.objectStore('parts').delete(IDBKeyRange.bound(id + '|', id + '|￿'));
  const idx = tx.objectStore('snapshots').index('byProject');
  idx.openKeyCursor(IDBKeyRange.only(id)).onsuccess = e => {
    const c = e.target.result;
    if (c) { tx.objectStore('snapshots').delete(c.primaryKey); c.continue(); }
  };
  await done(tx);
  journalClear(id);
}

// ── Snapshots ──────────────────────────────────────────────────────────────

const KEEP_SNAPSHOTS = 60;

export async function addSnapshot(id, reason, parts, stats) {
  const d = await db();
  const tx = d.transaction('snapshots', 'readwrite');
  const store = tx.objectStore('snapshots');
  store.add({ id, time: Date.now(), reason, parts, stats });
  // Prune the oldest automatic snapshots beyond the limit; named ones stay.
  const all = await result(store.index('byProject').getAll(IDBKeyRange.only(id)));
  const auto = all.filter(s => s.reason === 'auto').sort((a, b) => a.time - b.time);
  const excess = all.length + 1 - KEEP_SNAPSHOTS;
  for (const s of auto.slice(0, Math.max(0, excess))) store.delete(s.sid);
  await done(tx);
}

export async function listSnapshots(id) {
  const tx = (await db()).transaction('snapshots');
  const all = await result(tx.objectStore('snapshots').index('byProject').getAll(IDBKeyRange.only(id)));
  return all.sort((a, b) => b.time - a.time);
}

export async function getSnapshot(sid) {
  return result((await db()).transaction('snapshots').objectStore('snapshots').get(sid));
}

// ── Journal ────────────────────────────────────────────────────────────────
// localStorage is synchronous, so an edit is on disk before the next frame
// even if the tab is killed. Entries are cleared once IndexedDB has them.

const jkey = id => `babel.journal.${id}`;

export function journalRead(id) {
  try { return JSON.parse(localStorage.getItem(jkey(id)) ?? 'null') ?? { seq: 0, entries: {} }; }
  catch { return { seq: 0, entries: {} }; }
}

export function journalWrite(id, pid, xml) {
  try {
    const j = journalRead(id);
    j.seq++;
    j.entries[pid] = { seq: j.seq, xml };
    localStorage.setItem(jkey(id), JSON.stringify(j));
    return j.seq;
  } catch {
    return -1; // quota exceeded: IndexedDB still has it shortly
  }
}

export function journalCommit(id, upToSeq) {
  try {
    const j = journalRead(id);
    for (const [pid, e] of Object.entries(j.entries)) if (e.seq <= upToSeq) delete j.entries[pid];
    localStorage.setItem(jkey(id), JSON.stringify(j)); // keep seq monotonic
  } catch { /* ignore */ }
}

export function journalClear(id) {
  try { localStorage.removeItem(jkey(id)); } catch { /* ignore */ }
}
