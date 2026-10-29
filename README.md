# Babel

Side-by-side translation of Word documents.

Open a `.docx`, pick the target language, and the right-hand panel becomes an
editable copy of the original. Type over it paragraph by paragraph and export
a `.docx` that is the original file with the words changed: same styles,
numbering, tables, images, headers, footers, footnotes, fields and section
setup.

Babel comes in three forms, all with the same editor:

| | | Where translations are kept |
|---|---|---|
| **Desktop app** (`babel-desktop`) | A KDE/Kirigami window, Rust + Qt | On the computer; synced with a Babel server when signed in |
| **Server** (`babel-server`) | The web app with accounts | On the server, per user |
| **Static site** (<https://babel.cptjanst.se>) | No build step, nothing uploaded | In the browser (IndexedDB) |

## Architecture

```
crates/babel-core     storage (SQLite) and the paragraph merge
crates/babel-server   HTTP API + the web app (embedded), accounts
crates/babel-sync     syncing a local store with a server account
crates/babel-desktop  the Kirigami app (cxx-qt), runs babel-server's API on localhost
web/                  the editor: HTML/CSS/JS, no build step
```

The editor (`web/`) is one codebase. `web/js/store.js` decides where it keeps
translations: in IndexedDB when it is served as static files, or behind the
HTTP API when a Babel server serves it — `babel-server` for the web, or the
desktop app's own copy of the same server, bound to `127.0.0.1` over the
computer's local database. The desktop app's list of translations, file
dialogs, export dialog, account and sync are native (QML/Kirigami, with the
logic in Rust); the two document panels are the editor in a QtWebEngine view,
because faithful page layout of a Word document is what docx-preview and the
browser engine provide and Qt's text engine does not.

## The model

Babel does not convert the document to HTML and back. The original OOXML stays
the source of truth, and an edit rewrites the text of runs inside it.

1. **Normalise.** Every `w:r` is split so it holds exactly one content element
   (`w:t`, `w:tab`, `w:br`, a drawing, a field character…). Runs with identical
   properties are equivalent in Word, so this is invisible. Every `w:p` and `w:r`
   gets an id (`data-bp`, `data-br`).
2. **Render.** [docx-preview](https://github.com/VolodymyrBaydalka/docxjs)
   lays both documents out. It is vendored with a handful of `BABEL PATCH`
   changes so the ids come through as `data-p` / `data-r` on the HTML, plus a
   `data-k` run kind: `t` text, `tab`, `br`, or `a` (anything else).
3. **Edit.** Every rendered paragraph is its own `contenteditable` host, so the
   browser cannot split or merge paragraphs. Non-text runs are non-editable
   islands; deleting images, footnote references and fields is refused. Tabs
   and line breaks may be deleted, and `Shift+Enter` adds a line break.
4. **Write back.** On input, the paragraph is read back as tokens (text with the
   run it sits in, atoms, breaks) and `applyParagraph` rewrites the paragraph's
   text runs in place. Text typed outside any run borrows the formatting of
   the run before it; `Ctrl+B/I/U` become explicit run properties, inserted in
   schema order. Nothing that was not rendered (deleted revisions, `mc:Fallback`
   copies, field codes) is ever touched.
5. **Export.** The original zip with the translated text parts swapped in. Ids
   and `w:proofErr` are stripped, neighbouring runs with identical formatting
   are merged again, `w:lang` is set to the target language everywhere (and
   on the document defaults), and `w:proofState` is dropped so Word re-checks
   spelling. Every other part is copied byte for byte.

## Layout

- Word records where it last broke pages (`w:lastRenderedPageBreak`); those
  are the page breaks shown. Pages that still overflow (documents never
  paginated by Word, or different font metrics) are split at block level —
  between paragraphs, keeping headings with what follows, or between table
  rows. **The source decides the splits and the target replays them**, so any
  difference in page height between the panels is the translation's doing.
- A target page taller than its source page gets a red edge and a note
  ("Overflows by about 2 lines"); the status bar counts them and jumps to them.
- `fonts/` holds metric-compatible stand-ins for Calibri, Cambria, Arial,
  Helvetica, Times New Roman and Courier New (Carlito, Caladea, Arimo, Tinos,
  Cousine — OFL/Apache, via Fontsource). Each `@font-face` tries `local()`
  first, so the real font wins when installed. Equal advance widths give the
  same line breaks as Word.
- Zoom is a `transform: scale` on a sized wrapper, so layout measurements
  (`offsetHeight`) stay in document pixels.

## Native layout and editing (desktop, in progress)

`crates/babel-pages` lays documents out with WordCraft's engine (the
`inthevidual/wordcraft` fork, whose .docx reader keeps each paragraph's
`data-bp` and each run's `data-br`), and `crates/babel-docx` is the Rust
port of the write-back (`applyParagraph`), checked against the JavaScript on
2,254 generated cases (`tests/parity.rs`). In the desktop app, **Native
Layout** on the editor page shows both documents laid out natively; the
translation can be edited there: click for the caret, type, Backspace and
Delete, arrows, Home/End, Enter for the next paragraph. Edits are written
into the same XML the web editor writes and saved a second after typing
stops. Not yet: selection, copy and paste, undo, formatting keys, Shift+Enter,
input methods beyond plain key text, proofing.

## Recoverability

Three layers, so no keystroke depends on one mechanism:

| Layer | When | What |
|---|---|---|
| Journal | every edit, synchronously | the edited `w:p` in `localStorage` |
| Store | ~1 s after typing stops (max 4 s) | full translated XML parts |
| Versions | every 3 min of work, session start, download, restore | full snapshot, 60 kept |

The store is IndexedDB on the static site and the server's (or the desktop
app's) SQLite database otherwise. On open, the journal is replayed over the
store, so an edit survives a crashed tab (tested by crashing the renderer).
Restoring a version first saves the current state as a version. On the
static site `navigator.storage.persist()` is requested so the browser does not
evict the data. A Web Lock makes sure only one tab edits a translation;
opening it in a second tab pauses the first.

If storage is lost entirely: *Continue from a translated file* on the
start page pairs the original with a downloaded translation (paragraph
structure must match) and carries on.

## Saving, merging and sync

The server numbers every save of a project's translated parts (a revision)
and keeps the last 100; part contents are stored once per distinct text,
zstd-compressed. A save names the revision it started from. If another
device saved since, the server merges **paragraph by paragraph**: Babel never
splits or merges paragraphs, so every copy has the same paragraphs, each with
a stable `data-bp` id. A paragraph changed on one side takes that side; one
changed on both takes the newer save, and the state it overwrote is kept as a
version ("before merging edits from another device"). Relationship parts
(pasted links) merge per relationship. An editor with the translation open
hears about other saves through server-sent events and reloads; unsaved
edits are in its journal and replay on top.

The desktop app syncs its local store with a server account
(`crates/babel-sync`): after local saves, every 20 s, and on *Sync Now*. Each
round uploads and downloads whole projects the other side has never seen,
pushes local revisions (the changed parts, merged on the server as above) or
pulls newer server ones, carries deletions both ways, metadata by most recent
change, and versions as a union. It works offline and catches up when the
server is reachable. The session token for the server is kept in the app's
database, in a directory only the user can read.

## Proofing

Edited paragraphs (target text differs from the source) are checked with
[LanguageTool](https://languagetool.org). Requests are batched, paced for the
free API's limits (20 requests and 75 kB per minute) with back-off on 429.
Results are drawn with the CSS Custom Highlight API, so the editable DOM is
never touched; the ranges are live and survive edits elsewhere in the
paragraph. Clicking an underline shows suggestions, *Ignore* (per
translation) and *Add to dictionary* (per browser, or per desktop app). A Premium account or own
server can be set in Settings. Proofing can be turned off. On the static site
nothing else leaves the browser; otherwise translations go only to the Babel
server serving the editor (and the one the desktop app syncs with).

## Context menu

Right-click in either panel (`js/menu.js`; Shift+right-click gives the
browser's own menu). Items register with a condition, so new actions are
one `menu.add({...})` in `main.js`.

- **Copy link** on a link in either panel. The address is read from the XML,
  so it works for `w:hyperlink` (external or bookmark) and for `HYPERLINK`
  fields, which are not rendered as links.
- **Paste link** (also `Ctrl+K`) on selected text in the translation. Uses
  the link Babel copied, or a web address copied elsewhere. The selection
  becomes a real `w:hyperlink`, with a relationship added to the part's
  `.rels` (journalled like a paragraph), and takes the copied link's character
  style (`Hyperlink`, colour, underline). Pasting onto existing linked text
  changes its address; a selection that straddles a link, field or tracked
  change is refused. The paragraph is then laid out again on its own.
- **Look up in Merriam-Webster** on a selected term in the original (up to
  six words; surrounding punctuation is trimmed). Opens the entry in a new tab.
- Cut / Copy / Paste as plain text, Mark as done, Revert, Copy the original.

On export, a hyperlink whose relationship is missing is unwrapped (text kept)
rather than produce a file Word rejects.

## Counts

"Characters with spaces" follows Word: text of the body, footnotes and
endnotes, without tabs and paragraph marks, deleted revisions or field codes.
The target bar shows the difference from the source; the right end of each
bar shows the active paragraph.

## Files

| | |
|---|---|
| `web/index.html` | markup |
| `web/app.css` | interface; the documents are styled by docx-preview |
| `web/js/main.js` | projects, rendering, write-back, saving, proofing UI, versions, sign-in |
| `web/js/ooxml.js` | package handling, normalisation, write-back, export |
| `web/js/editor.js` | editing hosts, token reading, input guards, keyboard |
| `web/js/view.js` | rendering, pagination, overflow, zoom |
| `web/js/store.js` | picks the backend; `store-idb.js` (IndexedDB), `store-http.js` (Babel API) |
| `web/js/journal.js` | the synchronous localStorage journal |
| `web/js/proof.js` | LanguageTool client |
| `web/js/menu.js` | context menu |
| `web/js/langs.js` | target languages (Word code ↔ LanguageTool code) |
| `web/vendor/docx-preview.mjs` | docx-preview 0.3.7 with `BABEL PATCH` changes |
| `web/vendor/jszip.min.js` | JSZip 3.10.1 |
| `web/fonts/` | metric-compatible fonts + `fonts.css` |
| `crates/babel-core/src/store.rs` | projects, revisions, versions, users, sessions |
| `crates/babel-core/src/merge.rs` | three-way merge of OOXML parts by paragraph |
| `crates/babel-server/src/lib.rs` | the HTTP API, sign-in, events, the embedded web app |
| `crates/babel-sync/src/lib.rs` | the sync engine |
| `crates/babel-desktop/src/` | app start-up and local server (`app.rs`), QML bridge (`bridge.rs`), account (`sync.rs`) |
| `crates/babel-desktop/qml/` | the window: translations list, editor page, account dialog |
| `test/make-fixture.py` | builds `test/fixtures/sample.docx` |
| `test/e2e.mjs` | browser test: edit, format, breaks, guards, headers, overflow, proofing, reload, crash recovery, download, pairing |
| `test/sync.mjs` | two signed-in browsers: live updates and merged simultaneous edits |

## Build and run

Needs Rust, Qt 6 (base, declarative, webengine), Kirigami 6 and, for
installing, `make`. On Arch: `pacman -S rust qt6-webengine kirigami
qqc2-desktop-style`.

```
make install            # babel-desktop into ~/.local (PREFIX=/usr with sudo)
babel-desktop           # or Babel in the application menu; opens .docx from Dolphin
```

The desktop app keeps its data in `~/.local/share/babel/` and serves the
editor on `127.0.0.1:47917` (a fixed port, because the recovery journal lives
in that origin's storage). `babel-desktop FILE.docx` opens a document;
`--project=ID` opens a translation.

Server:

```
cargo build --release -p babel-server
babel-server --db babel.db user add NAME      # asks for the password
babel-server --db babel.db serve --listen 127.0.0.1:8790
```

`deploy/` has a systemd unit and a Caddy config for HTTPS (use
`--secure-cookies` there). Accounts: `user add|passwd|del|list`; a new
password signs the user out everywhere.

Static site: serve `web/` as files (`python3 -m http.server 8790` from `web/`).

## Test

```
cargo test --workspace                 # store, merge, server-backed sync engine
python3 test/make-fixture.py

# the editor on the static site
(cd web && python3 -m http.server 8790) &
node test/e2e.mjs                      # needs Playwright and Chrome (PLAYWRIGHT_DIR, CHROME)

# the editor against babel-server, and two devices on one account
babel-server --db /tmp/t.db user add tester    # password testpass123
babel-server --db /tmp/t.db serve --listen 127.0.0.1:8791 &
BASE=http://localhost:8791/ LOGIN=tester:testpass123 node test/e2e.mjs
BASE=http://localhost:8791/ LOGIN=tester:testpass123 node test/sync.mjs
```

## Limits

- `.docx` only (not `.doc`, `.odt`). Right-to-left targets aren't supported.
- Paragraphs are never split or merged — that is the point, but it means a
  sentence can't move to another paragraph.
- Charts, SmartArt and WordArt render as docx-preview manages; their text is
  not editable. Comments are not shown.
- Pagination is block-level: a paragraph that Word would break across pages
  moves whole to the next page (both panels alike).
- Sync conflicts are resolved per paragraph, newest save wins; the other
  text is in Versions, not shown side by side.
- The settings, versions and help dialogs are still the editor's own (web)
  dialogs inside the desktop window.

## Deploy

The static site: GitHub Pages serves `main`. Since the app moved into `web/`,
Pages needs to publish that folder (a Pages workflow, or the `/docs`
convention) — until then `main` keeps the old layout. DNS is a DNS-only CNAME
in the `cptjanst.se` Cloudflare zone → `inthevidual.github.io`.
