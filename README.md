# Babel

Side-by-side translation of Word documents: <https://babel.cptjanst.se>

Open a `.docx`, pick the target language, and the right-hand panel becomes an
editable copy of the original. Type over it paragraph by paragraph and download
a `.docx` that is the original file with the words changed: same styles,
numbering, tables, images, headers, footers, footnotes, fields and section
setup. Static site, no build step, nothing is uploaded anywhere.

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

## Recoverability

Three layers, so no keystroke depends on one mechanism:

| Layer | When | What |
|---|---|---|
| Journal | every edit, synchronously | the edited `w:p` in `localStorage` |
| IndexedDB | ~1 s after typing stops (max 4 s) | full translated XML parts |
| Versions | every 3 min of work, session start, download, restore | full snapshot, 60 kept |

On open, the journal is replayed over IndexedDB, so an edit survives a crashed
tab (tested by crashing the renderer). Restoring a version first saves the
current state as a version. `navigator.storage.persist()` is requested so the
browser does not evict the data. A Web Lock makes sure only one tab edits a
translation; opening it in a second tab pauses the first.

If browser storage is lost entirely: *Continue from a translated file* on the
start page pairs the original with a downloaded translation (paragraph
structure must match) and carries on.

## Proofing

Edited paragraphs (target text differs from the source) are checked with
[LanguageTool](https://languagetool.org). Requests are batched, paced for the
free API's limits (20 requests and 75 kB per minute) with back-off on 429.
Results are drawn with the CSS Custom Highlight API, so the editable DOM is
never touched; the ranges are live and survive edits elsewhere in the
paragraph. Clicking an underline shows suggestions, *Ignore* (per
translation) and *Add to dictionary* (per browser). A Premium account or own
server can be set in Settings. Proofing can be turned off; nothing else leaves
the browser.

## Counts

"Characters with spaces" follows Word: text of the body, footnotes and
endnotes, without tabs and paragraph marks, deleted revisions or field codes.
The target bar shows the difference from the source; the right end of each
bar shows the active paragraph.

## Files

| | |
|---|---|
| `index.html` | markup |
| `app.css` | interface; the documents are styled by docx-preview |
| `js/main.js` | projects, rendering, write-back, saving, proofing UI, versions |
| `js/ooxml.js` | package handling, normalisation, write-back, export |
| `js/editor.js` | editing hosts, token reading, input guards, keyboard |
| `js/view.js` | rendering, pagination, overflow, zoom |
| `js/store.js` | IndexedDB, snapshots, journal |
| `js/proof.js` | LanguageTool client |
| `js/langs.js` | target languages (Word code ↔ LanguageTool code) |
| `vendor/docx-preview.mjs` | docx-preview 0.3.7 with `BABEL PATCH` changes |
| `vendor/jszip.min.js` | JSZip 3.10.1 |
| `fonts/` | metric-compatible fonts + `fonts.css` |
| `test/make-fixture.py` | builds `test/fixtures/sample.docx` |
| `test/e2e.mjs` | browser test: edit, format, breaks, guards, headers, overflow, proofing, reload, crash recovery, download, pairing |

## Run and test

```
python3 -m http.server 8790
python3 test/make-fixture.py
node test/e2e.mjs        # needs Playwright and Chrome
```

## Limits

- `.docx` only (not `.doc`, `.odt`). Right-to-left targets aren't supported.
- Paragraphs are never split or merged — that is the point, but it means a
  sentence can't move to another paragraph.
- Charts, SmartArt and WordArt render as docx-preview manages; their text is
  not editable. Comments are not shown.
- Pagination is block-level: a paragraph that Word would break across pages
  moves whole to the next page (both panels alike).

## Deploy

Push to `main`; GitHub Pages serves the root. DNS is a DNS-only CNAME in the
`cptjanst.se` Cloudflare zone → `inthevidual.github.io`.
