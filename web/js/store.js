// Persistence. Three layers, so that no keystroke depends on one mechanism:
//   1. a synchronous localStorage journal of every paragraph as it is edited
//      (journal.js),
//   2. the full translated XML parts (debounced),
//   3. snapshots — rolling versions that can be restored or downloaded.
// Layers 2 and 3 live in this browser's IndexedDB on the static site, or
// behind Babel's HTTP API when a Babel server (or the desktop app) serves
// the page.

import * as idb from './store-idb.js';
import * as http from './store-http.js';

export * from './journal.js';

const config = await http.probe();
const B = config ? http : idb;

// { kind: 'idb' } or { kind: 'http', mode: 'server' | 'desktop', user, … }
export const backend = config ? { kind: 'http', ...config } : { kind: 'idb' };

export const {
  requestPersistence, storageInfo, listProjects, getProject, createProject, getSource,
  getParts, saveParts, saveProjectMeta, deleteProject, addSnapshot, listSnapshots, getSnapshot,
} = B;

export const watch = B.watch ?? (() => () => {});
export const onRemoteChange = B.onRemoteChange ?? (() => {});
export const { login, logout, AuthError } = http;
