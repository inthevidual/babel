// The first persistence layer, whatever the backend: a synchronous
// localStorage journal of every paragraph as it is edited.

// localStorage is synchronous, so an edit is on disk before the next frame
// even if the tab is killed. Entries are cleared once the backend has them.

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
    return -1; // quota exceeded: the backend still has it shortly
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
