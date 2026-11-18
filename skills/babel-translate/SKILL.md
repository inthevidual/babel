---
name: babel-translate
description: Translates a Word document (.docx) segment by segment into a Babel segment file (Markdown), which the translator attaches to the document in Babel to insert Claude's translation of any paragraph with a right-click. Use when the user asks to translate a .docx (or a Babel "segments" .md file) for Babel, or asks for a "segment file", "Babel translation" or "Claude translation to attach".
---

# Babel segment translation

Babel is a two-panel translation editor for Word documents. The translator works
paragraph by paragraph; this skill gives them a translation of every paragraph by
Claude, in a file they attach to the document in Babel. When a paragraph is tricky,
or they want Claude's version, they right-click it and choose **Insert Claude's
Translation**. So each segment must stand on its own as a good, finished
translation of exactly that paragraph.

## What you get

Either:

- a **.docx**: make the segment file from it with the script (step 1), or
- a **segment file** exported from Babel (Markdown starting with `babel-segments: 1`,
  headings like `## 12 · document:4`): translate it as it is; skip step 1.

Ask for the target language if the user hasn't given it (and it isn't in the file's
`target-language:` line). Use a language tag such as `sv-SE`, `de-DE`, `en-GB`.

## Steps

1. **Extract** (for a .docx):

   ```
   python3 scripts/babel_segments.py extract "DOCUMENT.docx" --lang sv-SE -o "WORK.md"
   ```

   `scripts/` is in this skill's folder. Each segment is one paragraph of the
   document that has something to translate:

   ```
   ## 12
   > This report is **confidential**.{1} See the table below.

   ```

2. **Translate** every segment: write the translation on the line(s) after the quote,
   with one blank line between the quote and the translation. Change nothing else:
   not the headings, not the quotes, not the front matter.

   ```
   ## 12
   > This report is **confidential**.{1} See the table below.

   Den här rapporten är **konfidentiell**.{1} Se tabellen nedan.
   ```

   Work through a long document in order, a few dozen segments at a time, writing
   them into the file as you go (edit the file in place; don't rewrite it whole
   from memory). Every segment needs a translation, including short ones such as
   table cells and captions. Copy text that stays the same (names, figures, codes,
   addresses) as it is.

3. **Check** the file, fix what it reports, and check again until it is clean:

   ```
   python3 scripts/babel_segments.py check "WORK.md"
   ```

4. **Deliver** the file as `<document name>.<language>.babel.md` (for example
   `Annual report.sv-SE.babel.md`), in the outputs folder if there is one, and tell
   the user how to use it (below).

## The rules of a segment

- **One segment, one paragraph.** Never merge, split, reorder or renumber segments.
  A translation that reads better as two sentences stays in its one paragraph.
- **`{1}`, `{2}`… are things in the paragraph that aren't text**: footnote and endnote
  marks, pictures, fields such as dates and page numbers, symbols. Each must appear
  exactly once in the translation, where it belongs in the translated sentence: a
  footnote mark after the word or sentence it refers to, a date field where the date
  goes. Keep them in the original's order unless the target language truly needs
  otherwise.
- **`{tab}`** is a tab: keep the same number, between the same parts (table-like
  lines such as `Item{tab}Amount`).
- **Lines**: each line of the quote is a line break within the paragraph. Give the
  translation the same lines (an address keeps its lines). An empty quoted line is
  an empty line; keep it.
- **`**bold**`, `*italic*`, `***both***`** mark the original's direct formatting.
  Mark the words in the translation that correspond. Don't add emphasis the original
  doesn't have. A paragraph that is all bold or all italic (a heading) may be quoted
  without marks: it keeps its look anyway.
- **Escapes**: write a literal `*` as `\*` and a literal `{` as `\{`; a translation
  line that starts with `#` or `>` gets a backslash in front (`\# 1`).
- **No notes in the file.** If a segment needs a comment (an ambiguity, a term you
  weren't sure of), say so in your reply, naming the segment number.
- Links (web addresses) stay as they are in the original; Babel keeps the link
  around the translated words.

## Translating well

Translate for the document's readers, in the register of the original: a report
stays formal, marketing copy stays lively. Follow the target language's own
conventions for punctuation, quotation marks, number and date formats where the
document is clearly meant for that language's readers; keep figures themselves
unchanged. Keep terminology consistent across segments: the same term for the same
thing throughout. Where the user has given a glossary or style notes, they win.

## Telling the user how to use it

In Babel, with the document open:

- **Desktop**: the ⋮ menu › **Attach Claude's Translation…**, and choose the file.
- **Web**: right-click in the translation › **Attach Claude's translation…**.

Then right-click any paragraph of the translation › **Insert Claude's Translation**
(one undo takes it back). Babel says how many paragraphs have a translation; a
paragraph without one has the item greyed out. The file is kept with the translation
and synced, so it is there on the other computers and the web too.

Babel can also export a document's segments itself (⋮ › **Export Segments for
Claude…**); such a file can be given to you instead of the .docx.
