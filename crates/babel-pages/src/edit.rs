//! Editing a translation on the natively laid-out pages.
//!
//! A [`Session`] holds the translation's parts (Babel's normalised XML, the
//! source of truth) and their layout. Positions are WordCraft's: a story, a
//! paragraph path and a byte offset into that paragraph's text.
//!
//! An edit changes three things in step:
//! 1. the paragraph's *pieces* — its text cut up by the run each stretch came
//!    from (WordCraft's `run_sources`), the same model the web editor reads
//!    back from the page;
//! 2. the paragraph's `w:p`, rewritten from those pieces by
//!    `babel_docx::apply_paragraph`, exactly as the web editor would;
//! 3. WordCraft's paragraph, so that it can be laid out again at once (its
//!    layout cache keeps every other paragraph).
//!
//! The concatenated pieces are always the WordCraft paragraph's text, so
//! offsets mean the same in both. Every change is one [`Session::replace`] of
//! a range of one paragraph (paragraphs are never split or merged, as in the
//! web editor), or a formatting change; each can be undone.

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

use babel_docx::para::{find_paragraph, runs, RunKind};
use babel_docx::{apply_paragraph, Info, Templates, Token};
use serde_json::{Map, Value};
use wordcraft_doc::{Document, Paragraph, Path, Pos, StoryRef};
use wordcraft_layout::{DocLayout, LayoutCache, Placed};

use crate::Parts;

const OBJ: char = '\u{FFFC}';

#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    /// Text the translator may change: in run `rid`, or typed outside any run
    /// (then formatted like run `base`), with explicit bold/italic/underline
    /// changes relative to that run.
    Text { rid: Option<String>, base: Option<String>, text: String, fmt: Option<Map<String, Value>> },
    /// A tab or line break of the original that may be deleted.
    Atom { rid: String, text: String },
    /// A line break the translator inserted (Shift+Enter).
    Br { base: Option<String> },
    /// Anything else, kept as it is (`rid`: its run, if it has one).
    Fixed { rid: Option<String>, text: String },
}

impl Piece {
    fn text(&self) -> &str {
        match self {
            Piece::Text { text, .. } | Piece::Atom { text, .. } | Piece::Fixed { text, .. } => text,
            Piece::Br { .. } => "\n",
        }
    }
    fn len(&self) -> usize {
        self.text().len()
    }
}

#[derive(Debug, Clone)]
struct Para {
    part: String,
    pid: String,
    info: Info,
    pieces: Vec<Piece>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum EditError {
    #[error("this paragraph can't be edited here")]
    NotEditable,
    #[error("that would delete something that isn't text (a picture, note or field)")]
    Fixed,
    #[error("select text within one paragraph")]
    AcrossParagraphs,
    #[error("{0}")]
    Internal(String),
}

/// Bold, italic or underline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Bold,
    Italic,
    Underline,
}

impl Style {
    fn key(self) -> &'static str {
        match self {
            Style::Bold => "b",
            Style::Italic => "i",
            Style::Underline => "u",
        }
    }
}

/// A paragraph as it was before an edit: enough to put it back exactly.
#[derive(Clone)]
struct Snapshot {
    pid: String,
    story: StoryRef,
    path: Path,
    xml: String,
    para: Para,
    wc: Paragraph,
    caret: Pos,
    /// Typing continues this step while it goes on at `caret_after`.
    typing: Option<(Instant, Pos)>,
}

pub struct Session {
    original: Vec<u8>,
    /// The translation's text parts, as stored (path → XML).
    pub parts: Parts,
    pub doc: Document,
    pub layout: DocLayout,
    cache: LayoutCache,
    paras: HashMap<String, Para>,
    templates: Templates,
    /// Parts changed since [`Session::take_dirty`].
    dirty: BTreeSet<String>,
    /// Paragraphs changed since [`Session::take_edited`].
    edited: BTreeSet<String>,
    /// Paragraph id → where it is in the document.
    at: HashMap<String, (StoryRef, Path)>,
    /// Per page, what is drawn on it (to tell which pages an edit changed).
    prints: Vec<String>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

fn part_key(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".xml").unwrap_or(name)
}

fn page_print(page: &wordcraft_layout::Page) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    for it in page.header.iter().chain(&page.items).chain(&page.footer) {
        match it {
            Placed::Lines { story, path, para, l0, l1, x, y, .. } => {
                let _ = write!(s, "L{story:?}{:?}:{l0}-{l1}@{x},{y}#{:p};", path.0, std::sync::Arc::as_ptr(para));
            }
            other => {
                let d = format!("{other:?}");
                let _ = write!(s, "{};", &d[..d.len().min(200)]);
            }
        }
    }
    let _ = write!(s, "n{}", page.number);
    s
}

fn prev_char(t: &str, off: usize) -> usize {
    t[..off].char_indices().next_back().map(|(i, _)| i).unwrap_or(0)
}

fn next_char(t: &str, off: usize) -> usize {
    t[off..].char_indices().nth(1).map(|(i, _)| off + i).unwrap_or(t.len())
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '’' || c == '-' || c == '‑'
}

impl Session {
    pub fn open(original: Vec<u8>, parts: Parts) -> anyhow::Result<Session> {
        let doc = wordcraft_docx::read(&crate::renderable(&original, &parts)?)?;
        let mut cache = LayoutCache::default();
        let layout = wordcraft_layout::layout(&doc, &mut cache, &crate::options());
        let mut at = HashMap::new();
        for (story, path) in layout.index.keys() {
            if let Some(id) = doc.para(*story, path).and_then(|p| p.source.as_deref()) {
                at.insert(id.to_owned(), (*story, path.clone()));
            }
        }
        let prints = layout.pages.iter().map(page_print).collect();
        Ok(Session {
            original,
            parts,
            doc,
            layout,
            cache,
            paras: HashMap::new(),
            templates: Templates::new(),
            dirty: BTreeSet::new(),
            edited: BTreeSet::new(),
            at,
            prints,
            undo: vec![],
            redo: vec![],
        })
    }

    pub fn original(&self) -> &[u8] {
        &self.original
    }

    /// The parts changed since the last call, with their XML.
    pub fn take_dirty(&mut self) -> Parts {
        std::mem::take(&mut self.dirty).into_iter().filter_map(|p| self.parts.get(&p).map(|x| (p, x.clone()))).collect()
    }

    /// The paragraphs changed since the last call.
    pub fn take_edited(&mut self) -> BTreeSet<String> {
        std::mem::take(&mut self.edited)
    }

    pub fn page_count(&self) -> usize {
        self.layout.pages.len()
    }

    /// Page `i` as straight-alpha RGBA, `scale` pixels per point.
    pub fn render(&self, i: usize, scale: f32) -> (u32, u32, Vec<u8>) {
        let r = wordcraft_render::render_page(&self.doc, &self.layout.pages[i], scale, &crate::render_options());
        (r.width, r.height, r.to_straight())
    }

    // ── Positions ─────────────────────────────────────────────────────────

    /// The text position nearest to a point on a page (points from its top left).
    pub fn hit(&self, page: usize, x: f32, y: f32) -> Option<Pos> {
        let story = match self.layout.header_footer_at(page, y) {
            Some((s, _)) => s,
            None => self.layout.story_at(page, x, y).unwrap_or(StoryRef::Body),
        };
        self.layout.hit(page, x, y, story).or_else(|| self.layout.hit(page, x, y, StoryRef::Body))
    }

    pub fn caret(&self, pos: &Pos, page_hint: usize) -> Option<wordcraft_layout::hit::Caret> {
        self.layout.caret_on(pos, page_hint)
    }

    /// Highlight rectangles (page, rect in points) for a range of one paragraph.
    pub fn selection_rects(&self, a: &Pos, b: &Pos, page_hint: usize) -> Vec<(usize, [f32; 4])> {
        self.layout.selection_rects(&self.doc, a, b, page_hint).into_iter().map(|(p, r)| (p, [r.x, r.y, r.w, r.h])).collect()
    }

    /// The Babel paragraph id at a position, if it has one.
    pub fn paragraph_id(&self, pos: &Pos) -> Option<&str> {
        self.doc.para(pos.story, &pos.path)?.source.as_deref()
    }

    pub fn text_of(&self, pos: &Pos) -> Option<&str> {
        self.doc.para(pos.story, &pos.path).map(|p| p.text.as_str())
    }

    /// One character left (`-1`) or right, within the paragraph, or to the
    /// end/start of the one before/after.
    pub fn step(&self, pos: &Pos, dir: i32, page_hint: usize) -> Pos {
        if let Some(t) = self.text_of(pos) {
            if dir < 0 && pos.off > 0 {
                return Pos::new(pos.story, pos.path.clone(), prev_char(t, pos.off));
            }
            if dir > 0 && pos.off < t.len() {
                return Pos::new(pos.story, pos.path.clone(), next_char(t, pos.off));
            }
        }
        match self.layout.vertical(pos, None, dir, page_hint) {
            Some((p, _)) => {
                let len = self.text_of(&p).map(str::len).unwrap_or(0);
                Pos::new(p.story, p.path, if dir < 0 { len } else { 0 })
            }
            None => pos.clone(),
        }
    }

    /// One character left or right, never leaving the paragraph (for extending a selection).
    pub fn step_within(&self, pos: &Pos, dir: i32) -> Pos {
        let Some(t) = self.text_of(pos) else { return pos.clone() };
        let off = if dir < 0 { if pos.off > 0 { prev_char(t, pos.off) } else { 0 } } else if pos.off < t.len() { next_char(t, pos.off) } else { t.len() };
        Pos::new(pos.story, pos.path.clone(), off)
    }

    /// The word around a position (for double-click).
    pub fn word_at(&self, pos: &Pos) -> (Pos, Pos) {
        let Some(t) = self.text_of(pos) else { return (pos.clone(), pos.clone()) };
        let mut a = pos.off.min(t.len());
        let mut b = a;
        while a > 0 && t[..a].chars().next_back().is_some_and(word_char) {
            a = prev_char(t, a);
        }
        while b < t.len() && t[b..].chars().next().is_some_and(word_char) {
            b = next_char(t, b);
        }
        (Pos::new(pos.story, pos.path.clone(), a), Pos::new(pos.story, pos.path.clone(), b))
    }

    /// The position a line up (`-1`) or down, keeping `goal_x`.
    pub fn line(&self, pos: &Pos, goal_x: Option<f32>, dir: i32, page_hint: usize) -> Option<(Pos, f32)> {
        self.layout.vertical(pos, goal_x, dir, page_hint)
    }

    /// The start or end of the visual line a position is on.
    pub fn line_end(&self, pos: &Pos, end: bool, page_hint: usize) -> Pos {
        match self.layout.line_bounds(pos, page_hint) {
            Some((a, b)) => if end { b } else { a },
            None => pos.clone(),
        }
    }

    /// Where a paragraph is.
    pub fn locate(&self, pid: &str) -> Option<Pos> {
        self.at.get(pid).map(|(s, p)| Pos::new(*s, p.clone(), 0))
    }

    /// The plain text of a range of one paragraph (objects left out).
    pub fn text_between(&self, a: &Pos, b: &Pos) -> String {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        if a.story != b.story || a.path != b.path {
            return String::new();
        }
        self.text_of(a).map(|t| t[a.off.min(t.len())..b.off.min(t.len())].chars().filter(|c| *c != OBJ).collect()).unwrap_or_default()
    }

    // ── The paragraph model ───────────────────────────────────────────────

    fn para_state(&mut self, pos: &Pos) -> Result<&mut Para, EditError> {
        let pid = self.paragraph_id(pos).ok_or(EditError::NotEditable)?.to_owned();
        if !self.paras.contains_key(&pid) {
            let key = pid.split(':').next().unwrap_or("");
            let part = self.parts.keys().find(|p| part_key(p) == key).cloned().ok_or(EditError::NotEditable)?;
            let xml = &self.parts[&part];
            let range = find_paragraph(xml, &pid).ok_or(EditError::NotEditable)?;
            let r = runs(&xml[range]).map_err(|e| EditError::Internal(e.to_string()))?;
            let wp = self.doc.para(pos.story, &pos.path).ok_or(EditError::NotEditable)?;
            let mut pieces = vec![];
            let mut at = 0usize;
            let mut sources: Vec<_> = wp.run_sources.iter().map(|(s, e, id)| (*s as usize, *e as usize, id.to_string())).collect();
            sources.sort_by_key(|s| s.0);
            for (s, e, rid) in sources {
                if s < at || e > wp.text.len() {
                    return Err(EditError::NotEditable);
                }
                if s > at {
                    pieces.push(Piece::Fixed { rid: None, text: wp.text[at..s].to_owned() });
                }
                let text = wp.text[s..e].to_owned();
                pieces.push(match r.kinds.get(&rid) {
                    // The page must show exactly the run's text, or writing it back would change it.
                    Some(RunKind::Text) if r.texts.get(&rid) == Some(&text) => {
                        Piece::Text { rid: Some(rid.clone()), base: Some(rid), text, fmt: None }
                    }
                    Some(RunKind::Text) => return Err(EditError::NotEditable),
                    Some(RunKind::Atom) => Piece::Atom { rid, text },
                    _ => Piece::Fixed { rid: Some(rid), text },
                });
                at = e;
            }
            if at < wp.text.len() {
                pieces.push(Piece::Fixed { rid: None, text: wp.text[at..].to_owned() });
            }
            // Only runs shown here as pieces may be rewritten or removed: a run the web
            // editor would show but WordCraft draws differently (a field's result, which it
            // computes itself) would otherwise read as emptied and lose its text.
            let mut info = r.info;
            let shown: std::collections::HashSet<&str> = pieces
                .iter()
                .filter_map(|p| match p {
                    Piece::Text { rid: Some(r), .. } | Piece::Atom { rid: r, .. } => Some(r.as_str()),
                    _ => None,
                })
                .collect();
            info.text_runs.retain(|r| shown.contains(r.as_str()));
            info.atoms.retain(|r| shown.contains(r.as_str()));
            self.paras.insert(pid.clone(), Para { part, pid: pid.clone(), info, pieces });
        }
        Ok(self.paras.get_mut(&pid).expect("just inserted"))
    }

    /// Whether the text at a position can be edited.
    pub fn editable(&mut self, pos: &Pos) -> bool {
        self.para_state(pos).is_ok()
    }

    /// The pieces of a paragraph, for tests.
    pub fn pieces(&mut self, pos: &Pos) -> Result<Vec<Piece>, EditError> {
        Ok(self.para_state(pos)?.pieces.clone())
    }

    // ── Edits ─────────────────────────────────────────────────────────────

    /// Types `s` at `pos`; returns where the caret goes and the pages to draw again.
    pub fn insert(&mut self, pos: &Pos, s: &str) -> Result<(Pos, Vec<usize>), EditError> {
        self.replace(pos, pos, s)
    }

    /// A line break the translator adds (Shift+Enter).
    pub fn line_break(&mut self, pos: &Pos) -> Result<(Pos, Vec<usize>), EditError> {
        self.replace(pos, pos, "\n")
    }

    /// Deletes the character before (`-1`) or after the caret.
    pub fn delete(&mut self, pos: &Pos, dir: i32) -> Result<(Pos, Vec<usize>), EditError> {
        let t = self.text_of(pos).ok_or(EditError::NotEditable)?;
        let (a, b) = if dir < 0 {
            if pos.off == 0 {
                return Ok((pos.clone(), vec![])); // paragraphs are never merged
            }
            (prev_char(t, pos.off), pos.off)
        } else {
            if pos.off >= t.len() {
                return Ok((pos.clone(), vec![]));
            }
            (pos.off, next_char(t, pos.off))
        };
        let a = Pos::new(pos.story, pos.path.clone(), a);
        let b = Pos::new(pos.story, pos.path.clone(), b);
        self.replace(&a, &b, "")
    }

    /// Replaces the text between `a` and `b` (one paragraph) with `s`; a
    /// newline in `s` becomes a line break. Returns the caret after the new
    /// text and the pages to draw again.
    pub fn replace(&mut self, a: &Pos, b: &Pos, s: &str) -> Result<(Pos, Vec<usize>), EditError> {
        let (a, b) = if a <= b { (a.clone(), b.clone()) } else { (b.clone(), a.clone()) };
        if a.story != b.story || a.path != b.path {
            return Err(EditError::AcrossParagraphs);
        }
        let s: String = s.replace("\r\n", "\n").replace('\r', "\n").chars().filter(|c| !c.is_control() || *c == '\t' || *c == '\n').filter(|c| *c != OBJ).collect();
        if a.off == b.off && s.is_empty() {
            return Ok((a, vec![]));
        }
        let snap = self.snapshot(&a)?;
        let typing = a.off == b.off && s.chars().count() == 1 && s != "\n";
        let para = self.para_state(&a)?;
        let mut pieces = para.pieces.clone();
        remove_range(&mut pieces, a.off, b.off)?;
        let mut off = a.off;
        let mut props_at = None;
        for (i, line) in s.split('\n').enumerate() {
            if i > 0 {
                insert_break(&mut pieces, off);
                off += 1;
            }
            if !line.is_empty() {
                let p = insert_text(&mut pieces, off, line);
                props_at.get_or_insert(p);
                off += line.len();
            }
        }
        para.pieces = pieces;
        let pid = para.pid.clone();
        self.write_back(&pid)?;
        // The same change in WordCraft's paragraph, the new text formatted
        // like the text the pieces put it with.
        let wp = self.doc.para_mut(a.story, &a.path).map_err(|e| EditError::Internal(e.to_string()))?;
        if b.off > a.off {
            wp.delete(a.off, b.off).map_err(|e| EditError::Internal(e.to_string()))?;
        }
        let props = match wp.text.len() {
            0 => wp.mark.clone(),
            n => wp.props_of_char(props_at.unwrap_or(a.off.saturating_sub(1)).min(n - 1)).clone(),
        };
        if !s.is_empty() {
            wp.insert_text(a.off, &s, &props).map_err(|e| EditError::Internal(e.to_string()))?;
        }
        let caret = Pos::new(a.story, a.path.clone(), off);
        self.record(snap, typing, &caret);
        let pages = self.relayout();
        Ok((caret, pages))
    }

    /// Toggles bold, italic or underline over a range of one paragraph
    /// (off if the start of the range has it, else on).
    pub fn toggle(&mut self, a: &Pos, b: &Pos, style: Style) -> Result<Vec<usize>, EditError> {
        let (a, b) = if a <= b { (a.clone(), b.clone()) } else { (b.clone(), a.clone()) };
        if a.story != b.story || a.path != b.path {
            return Err(EditError::AcrossParagraphs);
        }
        if a.off == b.off {
            return Ok(vec![]);
        }
        let on = !self.has_style(&a, style);
        let snap = self.snapshot(&a)?;
        let para = self.para_state(&a)?;
        let mut out = vec![];
        let mut start = 0usize;
        for p in para.pieces.drain(..) {
            let end = start + p.len();
            match p {
                Piece::Text { rid, base, text, fmt } if start < b.off && a.off < end && !text.is_empty() => {
                    let (x, y) = (a.off.max(start) - start, b.off.min(end) - start);
                    let piece = |t: &str, f: Option<Map<String, Value>>| Piece::Text { rid: rid.clone(), base: base.clone(), text: t.to_owned(), fmt: f };
                    if x > 0 {
                        out.push(piece(&text[..x], fmt.clone()));
                    }
                    let mut f = fmt.clone().unwrap_or_default();
                    f.insert(style.key().to_owned(), Value::Bool(on));
                    out.push(piece(&text[x..y], Some(f)));
                    if y < text.len() {
                        out.push(piece(&text[y..], fmt));
                    }
                }
                p => out.push(p),
            }
            start = end;
        }
        para.pieces = out;
        let pid = para.pid.clone();
        self.write_back(&pid)?;
        let wp = self.doc.para_mut(a.story, &a.path).map_err(|e| EditError::Internal(e.to_string()))?;
        let set: &dyn Fn(&mut wordcraft_doc::CharProps) = match style {
            Style::Bold => &|p| p.bold = Some(on),
            Style::Italic => &|p| p.italic = Some(on),
            Style::Underline => &|p| p.underline = Some(if on { wordcraft_doc::props::Underline::Single } else { wordcraft_doc::props::Underline::None }),
        };
        wp.format(a.off, b.off, set).map_err(|e| EditError::Internal(e.to_string()))?;
        self.record(snap, false, &b);
        Ok(self.relayout())
    }

    /// Whether the text at a position is bold, italic or underlined.
    pub fn has_style(&self, pos: &Pos, style: Style) -> bool {
        let Some(p) = self.doc.para(pos.story, &pos.path) else { return false };
        if p.text.is_empty() {
            return false;
        }
        let cp = p.props_of_char(pos.off.min(p.text.len() - 1));
        let r = self.doc.styles.resolve_char(p.props.style.as_deref(), cp);
        match style {
            Style::Bold => r.bold,
            Style::Italic => r.italic,
            Style::Underline => r.underline != wordcraft_doc::props::Underline::None,
        }
    }

    // ── Undo ──────────────────────────────────────────────────────────────

    fn snapshot(&mut self, pos: &Pos) -> Result<Snapshot, EditError> {
        let para = self.para_state(pos)?.clone();
        let xml = &self.parts[&para.part];
        let range = find_paragraph(xml, &para.pid).ok_or(EditError::NotEditable)?;
        let wc = self.doc.para(pos.story, &pos.path).ok_or(EditError::NotEditable)?.clone();
        Ok(Snapshot { pid: para.pid.clone(), story: pos.story, path: pos.path.clone(), xml: xml[range].to_owned(), para, wc, caret: pos.clone(), typing: None })
    }

    fn record(&mut self, snap: Snapshot, typing: bool, after: &Pos) {
        self.redo.clear();
        // Typing goes on in the same step while it continues where it left off.
        if typing {
            if let Some(last) = self.undo.last_mut() {
                if let Some((t, at)) = &last.typing {
                    if last.pid == snap.pid && at == &snap.caret && t.elapsed() < Duration::from_secs(3) {
                        last.typing = Some((Instant::now(), after.clone()));
                        return;
                    }
                }
            }
        }
        let mut snap = snap;
        snap.typing = typing.then(|| (Instant::now(), after.clone()));
        self.undo.push(snap);
        if self.undo.len() > 500 {
            self.undo.remove(0);
        }
    }

    fn restore(&mut self, snap: Snapshot) -> Result<(Snapshot, Vec<usize>), EditError> {
        let pos = Pos::new(snap.story, snap.path.clone(), snap.caret.off);
        let mut current = self.snapshot(&pos)?;
        current.caret = snap.typing.as_ref().map(|(_, p)| p.clone()).unwrap_or_else(|| snap.caret.clone());
        let xml = self.parts.get_mut(&snap.para.part).ok_or(EditError::NotEditable)?;
        let range = find_paragraph(xml, &snap.pid).ok_or(EditError::NotEditable)?;
        xml.replace_range(range, &snap.xml);
        self.dirty.insert(snap.para.part.clone());
        self.edited.insert(snap.pid.clone());
        self.paras.insert(snap.pid.clone(), snap.para.clone());
        let wp = self.doc.para_mut(snap.story, &snap.path).map_err(|e| EditError::Internal(e.to_string()))?;
        *wp = snap.wc.clone();
        wp.touch();
        Ok((current, self.relayout()))
    }

    /// Undoes the last edit; returns the caret and the pages to draw again.
    pub fn undo(&mut self) -> Option<Result<(Pos, Vec<usize>), EditError>> {
        let snap = self.undo.pop()?;
        let caret = snap.caret.clone();
        Some(self.restore(snap).map(|(current, pages)| {
            self.redo.push(current);
            (caret, pages)
        }))
    }

    pub fn redo(&mut self) -> Option<Result<(Pos, Vec<usize>), EditError>> {
        let snap = self.redo.pop()?;
        let caret = snap.caret.clone();
        Some(self.restore(snap).map(|(mut current, pages)| {
            current.typing = None;
            self.undo.push(current);
            (caret, pages)
        }))
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    // ── Writing back ──────────────────────────────────────────────────────

    /// Rewrites the paragraph's `w:p` from its pieces.
    fn write_back(&mut self, pid: &str) -> Result<(), EditError> {
        let para = self.paras.get_mut(pid).ok_or(EditError::NotEditable)?;
        let mut tokens = vec![];
        let mut last_rid: Option<String> = None;
        for p in &para.pieces {
            match p {
                Piece::Text { rid, base, text, fmt } => {
                    if !text.is_empty() {
                        tokens.push(Token::Text { rid: rid.clone(), base: base.clone().or_else(|| last_rid.clone()), text: text.clone(), fmt: fmt.clone() });
                    }
                    if rid.is_some() {
                        last_rid = rid.clone();
                    }
                }
                Piece::Atom { rid, .. } => tokens.push(Token::Atom { rid: rid.clone() }),
                Piece::Br { base } => tokens.push(Token::Br { base: base.clone().or_else(|| last_rid.clone()) }),
                Piece::Fixed { rid: Some(rid), .. } => tokens.push(Token::Atom { rid: rid.clone() }),
                Piece::Fixed { rid: None, .. } => {}
            }
        }
        let xml = self.parts.get(&para.part).ok_or(EditError::NotEditable)?;
        let range = find_paragraph(xml, pid).ok_or(EditError::NotEditable)?;
        let new = apply_paragraph(&xml[range.clone()], &tokens, &mut para.info, &mut self.templates)
            .map_err(|e| EditError::Internal(e.to_string()))?;
        let part = para.part.clone();
        let xml = self.parts.get_mut(&part).expect("found above");
        xml.replace_range(range, &new);
        self.dirty.insert(part);
        self.edited.insert(pid.to_owned());
        Ok(())
    }

    /// Lays the document out again (unchanged paragraphs come from the cache)
    /// and returns the pages that look different.
    fn relayout(&mut self) -> Vec<usize> {
        self.layout = wordcraft_layout::layout(&self.doc, &mut self.cache, &crate::options());
        let prints: Vec<String> = self.layout.pages.iter().map(page_print).collect();
        let changed = (0..prints.len()).filter(|&i| self.prints.get(i) != Some(&prints[i])).collect();
        self.prints = prints;
        changed
    }
}

/// Removes `a..b` from the pieces; refuses if that would remove something fixed.
fn remove_range(pieces: &mut Vec<Piece>, a: usize, b: usize) -> Result<(), EditError> {
    if a == b {
        return Ok(());
    }
    let mut start = 0usize;
    for p in pieces.iter() {
        let end = start + p.len();
        if start < b && a < end && matches!(p, Piece::Fixed { .. }) {
            return Err(EditError::Fixed);
        }
        start = end;
    }
    let mut out = Vec::with_capacity(pieces.len());
    let mut start = 0usize;
    for p in pieces.drain(..) {
        let end = start + p.len();
        if end <= a || start >= b {
            out.push(p);
        } else {
            match p {
                Piece::Text { rid, base, text, fmt } => {
                    let (x, y) = (a.max(start) - start, b.min(end) - start);
                    let mut t = text;
                    t.replace_range(x..y, "");
                    // Text of an original run stays (emptied, it writes back as an empty run).
                    if !t.is_empty() || rid.is_some() {
                        out.push(Piece::Text { rid, base, text: t, fmt });
                    }
                }
                Piece::Atom { .. } | Piece::Br { .. } => {}
                Piece::Fixed { .. } => unreachable!("refused above"),
            }
        }
        start = end;
    }
    *pieces = out;
    Ok(())
}

/// Inserts text at `off`: into the text piece the offset is in; at a boundary,
/// the text before it (as a browser types), else the text after it; else a
/// new piece formatted like the last text before it. Returns the offset whose
/// formatting the new text takes in WordCraft's paragraph.
fn insert_text(pieces: &mut Vec<Piece>, off: usize, s: &str) -> usize {
    // An emptied piece only takes text when nothing else is there: a browser
    // drops a span once its text is gone and types into the one before.
    let (mut inside, mut before, mut after, mut empty) = (None, None, None, None);
    let mut last_rid: Option<String> = None;
    let mut start = 0usize;
    for (i, p) in pieces.iter().enumerate() {
        let end = start + p.len();
        if let Piece::Text { rid, base, .. } = p {
            if start < off && off < end {
                inside = Some((i, start));
            } else if start == end && start == off {
                empty.get_or_insert((i, start));
            } else if end == off {
                before = Some((i, start));
            } else if start == off && after.is_none() {
                after = Some((i, start));
            }
            if end <= off {
                last_rid = rid.clone().or_else(|| base.clone()).or(last_rid);
            }
        }
        start = end;
    }
    match inside.or(before).or(after).or(empty) {
        Some((i, start)) => {
            let Piece::Text { text, .. } = &mut pieces[i] else { unreachable!("only text pieces are targets") };
            text.insert_str(off - start, s);
            if off > start { off - 1 } else { start }
        }
        None => {
            let idx = index_at(pieces, off);
            pieces.insert(idx, Piece::Text { rid: None, base: last_rid, text: s.to_owned(), fmt: None });
            off.saturating_sub(1)
        }
    }
}

/// Inserts a line break at `off`, splitting a text piece if it is inside one.
fn insert_break(pieces: &mut Vec<Piece>, off: usize) {
    let mut start = 0usize;
    let mut last_rid: Option<String> = None;
    for i in 0..pieces.len() {
        let end = start + pieces[i].len();
        if let Piece::Text { rid, base, text, fmt } = &pieces[i] {
            if start < off && off < end {
                let (rid, base, fmt) = (rid.clone(), base.clone(), fmt.clone());
                let tail = text[off - start..].to_owned();
                let Piece::Text { text, .. } = &mut pieces[i] else { unreachable!() };
                text.truncate(off - start);
                pieces.insert(i + 1, Piece::Br { base: rid.clone().or_else(|| base.clone()) });
                pieces.insert(i + 2, Piece::Text { rid, base, text: tail, fmt });
                return;
            }
            if end <= off {
                last_rid = rid.clone().or_else(|| base.clone()).or(last_rid);
            }
        }
        start = end;
    }
    let idx = index_at(pieces, off);
    pieces.insert(idx, Piece::Br { base: last_rid });
}

/// The index of the first piece starting at or after `off`.
fn index_at(pieces: &[Piece], off: usize) -> usize {
    let mut start = 0usize;
    for (i, p) in pieces.iter().enumerate() {
        if start >= off {
            return i;
        }
        start += p.len();
    }
    pieces.len()
}
