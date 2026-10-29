// Translations kept behind Babel's HTTP API: babel-server, or the desktop
// app's local server. Same functions as store-idb.js.
//
// The server numbers every save of a project's parts (its revision). A save
// names the revision it was based on; if another device saved in between,
// the server merges paragraph by paragraph and says so, and the open
// translation is reloaded from the merged state.

import { journalClear } from './journal.js';

const CLIENT = crypto.randomUUID(); // tells this tab's own saves apart in the event stream
const revs = new Map();             // project id -> revision the parts in this tab are based on
let remoteChange = () => {};

export function onRemoteChange(fn) { remoteChange = fn; }

export class AuthError extends Error {}

async function api(method, path, body, { raw = false } = {}) {
  const headers = { 'x-babel-client': CLIENT };
  if (body !== undefined) headers['content-type'] = 'application/json';
  const res = await fetch('api/' + path, {
    method, headers, credentials: 'same-origin', cache: 'no-store',
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (res.status === 401) throw new AuthError('You are signed out. Sign in again to continue.');
  if (!res.ok) throw new Error((await res.text()) || `${res.status} ${res.statusText}`);
  if (raw) return res.arrayBuffer();
  return /json/.test(res.headers.get('content-type') ?? '') ? res.json() : null;
}

// The configuration if this page is served by a Babel server, else null.
export async function probe() {
  try {
    const res = await fetch('api/config', { cache: 'no-store', credentials: 'same-origin' });
    if (!res.ok || !/json/.test(res.headers.get('content-type') ?? '')) return null;
    const c = await res.json();
    return c?.babel ? c : null;
  } catch { return null; }
}

export const login = (username, password) => api('POST', 'login', { username, password });
export const logout = () => api('POST', 'logout');

export async function requestPersistence() {}

export async function storageInfo() { return null; }

export const listProjects = () => api('GET', 'projects');

export async function getProject(id) {
  try { return await api('GET', `projects/${id}`); }
  catch (e) { if (/not found/i.test(e.message)) return undefined; throw e; }
}

export async function createProject(project, source) {
  await api('POST', 'projects', { project, source: { parts: source.parts, bytes: toBase64(source.bytes) } });
  revs.set(project.id, 0);
}

export async function getSource(id) {
  const [parts, bytes] = await Promise.all([
    api('GET', `projects/${id}/source`),
    api('GET', `projects/${id}/source.docx`, undefined, { raw: true }),
  ]);
  return { id, bytes, parts };
}

export async function getParts(id) {
  const r = await api('GET', `projects/${id}/parts`);
  revs.set(id, r.rev);
  return r.parts;
}

export async function saveParts(project, parts) {
  const r = await api('POST', `projects/${project.id}/parts`, { baseRev: revs.get(project.id) ?? 0, project, parts });
  revs.set(project.id, r.rev);
  if (r.merged) remoteChange(project.id, 'merged');
}

export const saveProjectMeta = project => api('PUT', `projects/${project.id}/meta`, project);

export async function deleteProject(id) {
  await api('DELETE', `projects/${id}`);
  revs.delete(id);
  journalClear(id);
}

export const addSnapshot = (id, reason, parts, stats) =>
  api('POST', `projects/${id}/versions`, { time: Date.now(), reason, parts, stats });

// Without parts; getSnapshot fetches them.
export const listSnapshots = id => api('GET', `projects/${id}/versions`);

export const getSnapshot = sid => api('GET', `versions/${sid}`);

// Saves from other devices and tabs. Returns a function that stops listening.
export function watch(id) {
  if (!window.EventSource) return () => {};
  const es = new EventSource(`api/projects/${id}/events`);
  es.addEventListener('rev', e => {
    const { rev, origin } = JSON.parse(e.data);
    if (origin !== CLIENT && rev > (revs.get(id) ?? 0)) remoteChange(id, 'saved');
  });
  es.addEventListener('deleted', () => remoteChange(id, 'deleted'));
  return () => es.close();
}

function toBase64(buf) {
  const bytes = new Uint8Array(buf);
  let s = '';
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
  return btoa(s);
}
