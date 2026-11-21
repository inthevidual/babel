//! Babel's editor, without a platform: the translation laid out on pages with
//! a caret, selection, keys, formatting, undo, links, done marks, reverting,
//! proofing and statistics, and the original beside it to click, select and
//! copy from. The desktop app and the browser (WebAssembly) wrap it; they
//! draw the pages it says changed, store what it says to save and make the
//! LanguageTool requests it asks for.
//!
//! Answers are JSON, the same shape for both: see [`Editor::state`].

pub mod proof;
pub mod rows;
pub mod segments;

use std::collections::{BTreeSet, HashMap};

use babel_docx::ooxml::{self, Link, LinkTarget, Part, StyleBit};
use babel_docx::para::find_paragraph;
use babel_docx::tree::Tree;
use babel_pages::edit::{EditError, Piece, Session, Style};
use babel_pages::{Laid, Parts};
use serde_json::{json, Value};
use std::time::Duration;

use web_time::Instant;
use wordcraft_doc::Pos;

use proof::Proofing;

/// A paragraph is checked this long after typing in it stops.
pub const PROOF_DELAY: Duration = Duration::from_millis(1500);

fn same_para(a: &Pos, b: &Pos) -> bool {
    a.story == b.story && a.path == b.path
}

fn part_key(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".xml").unwrap_or(name)
}

/// Paragraph bookkeeping for statistics and marks (as main.js computeStats).
#[derive(Debug, Clone)]
struct Meta {
    path: String,
    src_text: String,
    tgt_text: String,
    translatable: bool,
    counted: bool,
}

/// What a text part is: the body and notes count towards characters.
fn counted(path: &str) -> bool {
    let k = part_key(path);
    !(k.starts_with("header") || k.starts_with("footer"))
}

/// Paragraph id → its text, for every paragraph of the parts.
fn texts(parts: &Parts, paths: &[String]) -> HashMap<String, (String, String)> {
    let mut out = HashMap::new();
    for path in paths {
        let Some(xml) = parts.get(path) else { continue };
        let Ok(part) = Part::parse(xml) else { continue };
        for p in ooxml::paragraphs(&part.tree) {
            if let Some(pid) = part.tree.attr(p, "data-bp") {
                out.insert(pid.to_owned(), (path.clone(), ooxml::para_text(&part.tree, p)));
            }
        }
    }
    out
}

fn paragraph_text_of(parts: &Parts, path: &str, pid: &str) -> Option<String> {
    let xml = parts.get(path)?;
    let r = find_paragraph(xml, pid)?;
    let t = Tree::parse(&xml[r]).ok()?;
    Some(ooxml::para_text(&t, t.root))
}

/// The look-up term in a selection (as main.js lookupTerm): up to six words.
pub fn lookup_term(text: &str) -> Option<String> {
    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = t.trim_matches(|c: char| !c.is_alphanumeric());
    (!t.is_empty() && t.chars().count() <= 80 && t.split(' ').count() <= 6).then(|| t.to_owned())
}

/// A link copied from the original, to paste onto the translation.
#[derive(Debug, Clone)]
struct Copied {
    link: Link,
    style: Vec<StyleBit>,
    clip: String,
}

pub enum Key {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Backspace,
    Delete,
    Enter,
    LineBreak,
    SelectAll,
    Undo,
    Redo,
    Done,
    Toggle(Style),
    Text(String),
}

impl Key {
    /// From a key code and modifiers as the platforms send them: Qt's key
    /// codes (the browser layer maps its keys to these) and Qt's modifier bits.
    pub fn from_qt(key: i32, text: &str, modifiers: i32) -> Option<Key> {
        const SHIFT: i32 = 0x0200_0000;
        const CTRL: i32 = 0x0400_0000;
        let (shift, ctrl) = (modifiers & SHIFT != 0, modifiers & CTRL != 0);
        Some(match (key, ctrl) {
            (0x5A, true) if shift => Key::Redo,
            (0x5A, true) => Key::Undo,
            (0x59, true) => Key::Redo,
            (0x42, true) => Key::Toggle(Style::Bold),
            (0x49, true) => Key::Toggle(Style::Italic),
            (0x55, true) => Key::Toggle(Style::Underline),
            (0x41, true) => Key::SelectAll,
            (0x0100_0004 | 0x0100_0005, true) => Key::Done,
            (_, true) => return None,
            (0x0100_0012, _) => Key::Left,
            (0x0100_0014, _) => Key::Right,
            (0x0100_0013, _) => Key::Up,
            (0x0100_0015, _) => Key::Down,
            (0x0100_0010, _) => Key::Home,
            (0x0100_0011, _) => Key::End,
            (0x0100_0003, _) => Key::Backspace,
            (0x0100_0007, _) => Key::Delete,
            (0x0100_0004 | 0x0100_0005, _) if shift => Key::LineBreak,
            (0x0100_0004 | 0x0100_0005, _) => Key::Enter,
            _ => {
                if text.is_empty() || text.chars().any(|c| c.is_control() && c != '\t') {
                    return None;
                }
                Key::Text(text.to_owned())
            }
        })
    }
}

pub struct Editor {
    pub session: Session,
    source: Laid,
    source_parts: Parts,
    text_parts: Vec<String>,
    meta: HashMap<String, Meta>,
    caret: Option<Pos>,
    anchor: Option<Pos>,
    goal_x: Option<f32>,
    page: usize,
    src_caret: Option<Pos>,
    src_anchor: Option<Pos>,
    pub proof: Proofing,
    /// When the translation was last edited (None: not in this session).
    last_edit: Option<Instant>,
    redraw: BTreeSet<usize>,
    redraw_all: bool,
    /// How many pages the view was last told of.
    drawn_pages: usize,
    src_redraw: bool,
    pub confirmed: BTreeSet<String>,
    /// Project metadata changed (done marks, ignored issues): to save.
    pub meta_dirty: bool,
    copied: Option<Copied>,
    /// Paragraphs and relationship parts changed since [`Editor::take_journal`].
    journal: BTreeSet<String>,
    /// The link under the last right-click on the original.
    copied_candidate: Option<Copied>,
    styles_xml: Option<String>,
    /// Claude's translation, segment by segment, when one is attached.
    claude: Option<Claude>,
    /// The text view: rows changed since [`Editor::take_row_changes`], all of
    /// them, and each row's number.
    row_changes: BTreeSet<String>,
    rows_all: bool,
    row_n: HashMap<String, usize>,
}

/// A segment file attached to the translation (see [`segments`]).
struct Claude {
    md: String,
    segments: Vec<segments::Segment>,
    /// Paragraph id → its segment.
    by_pid: HashMap<String, usize>,
    /// Paragraphs that are segments.
    total: usize,
}

impl Editor {
    /// Opens a translation: the original package, its normalised text parts,
    /// the translated parts stored (path → XML, over the originals'), and the
    /// project's metadata (`targetLang`, `confirmed`, `ignored`).
    pub fn open(original: Vec<u8>, source_parts: Parts, translated: Parts, project: &Value) -> anyhow::Result<Editor> {
        // The package's order (the body, then headers, footers and notes as it lists them).
        let mut text_parts: Vec<String> = babel_docx::package::open_package(&original)
            .map(|p| p.text_parts.into_iter().filter(|x| source_parts.contains_key(x)).collect())
            .unwrap_or_default();
        for k in source_parts.keys() {
            if !text_parts.contains(k) {
                text_parts.push(k.clone());
            }
        }
        let mut target = source_parts.clone();
        target.extend(translated);
        // Runs Babel generated in an earlier session, or that a link split off
        // without an id, become first-class runs (ensureIds).
        let mut adopted = vec![];
        for path in &text_parts {
            let Some(xml) = target.get(path) else { continue };
            if !xml.contains("data-bgen") && !ooxml::lacks_run_id(xml) {
                continue;
            }
            let mut part = Part::parse(xml)?;
            ooxml::ensure_ids(&mut part.tree, part_key(path));
            let new = part.serialize();
            if &new != xml {
                target.insert(path.clone(), new);
                adopted.push(path.clone());
            }
        }
        let mut session = Session::open(original.clone(), target)?;
        for p in &adopted {
            session.mark_dirty(p);
        }
        let source = babel_pages::lay_out(&babel_pages::renderable(&original, &source_parts)?)?;
        let styles_xml = text_parts.first().and_then(|p| {
            let dir = &p[..p.rfind('/').map(|i| i + 1).unwrap_or(0)];
            babel_pages::read_part(&original, &format!("{dir}styles.xml"))
        });

        let src = texts(&source_parts, &text_parts);
        let tgt = texts(&session.parts, &text_parts);
        let meta = src
            .into_iter()
            .filter_map(|(pid, (path, src_text))| {
                let tgt_text = tgt.get(&pid)?.1.clone();
                let translatable = src_text.chars().any(char::is_alphabetic);
                Some((pid, Meta { counted: counted(&path), path, src_text, tgt_text, translatable }))
            })
            .collect::<HashMap<_, _>>();

        let mut proof = Proofing { lang: project["targetLang"].as_str().and_then(proof::lt_code), ..Default::default() };
        proof.ignored = project["ignored"].as_array().map(|v| v.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default();
        for (pid, m) in &meta {
            proof.source.insert(pid.clone(), m.src_text.clone());
            if m.tgt_text != m.src_text {
                proof.queue.insert(pid.clone());
            }
        }
        let confirmed = project["confirmed"].as_array().map(|v| v.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default();
        let mut editor = Editor {
            session,
            source,
            source_parts,
            text_parts,
            meta,
            caret: None,
            anchor: None,
            goal_x: None,
            page: 0,
            src_caret: None,
            src_anchor: None,
            proof,
            last_edit: None,
            redraw: BTreeSet::new(),
            redraw_all: true,
            drawn_pages: 0,
            src_redraw: true,
            confirmed,
            meta_dirty: false,
            copied: None,
            journal: BTreeSet::new(),
            copied_candidate: None,
            styles_xml,
            claude: None,
            row_changes: BTreeSet::new(),
            rows_all: false,
            row_n: HashMap::new(),
        };
        if let Some(md) = project["claudeSegments"].as_str() {
            editor.claude = editor.index_segments(md).ok();
        }
        Ok(editor)
    }

    /// Settings for proofing: picky mode and the personal dictionary.
    pub fn set_proofing(&mut self, picky: bool, dictionary: impl IntoIterator<Item = String>) {
        self.proof.picky = picky;
        self.proof.dictionary = dictionary.into_iter().collect();
    }

    // ── Pages ─────────────────────────────────────────────────────────────

    pub fn page_count(&self) -> usize {
        self.session.page_count()
    }

    pub fn source_page_count(&self) -> usize {
        self.source.layout.pages.len()
    }

    /// Page sizes in points: (translation, original).
    pub fn page_sizes(&self) -> Value {
        let size = |p: &wordcraft_layout::Page| json!({ "w": p.w, "h": p.h });
        json!({
            "target": self.session.layout.pages.iter().map(size).collect::<Vec<_>>(),
            "source": self.source.layout.pages.iter().map(size).collect::<Vec<_>>(),
        })
    }

    /// The translation's pages to draw again (all, if their number changed).
    /// Whether there is anything to draw again (an edit since the last
    /// [`Editor::take_redraw`]).
    pub fn has_pending_redraw(&self) -> bool {
        self.redraw_all || !self.redraw.is_empty()
    }

    /// The pages to draw again; all of them when there are more or fewer
    /// than before (the view's list of pages changes), and never one that is
    /// gone (an edit may add a page and take it away again).
    pub fn take_redraw(&mut self) -> (bool, Vec<usize>) {
        let n = self.page_count();
        let all = std::mem::take(&mut self.redraw_all) || n != self.drawn_pages;
        self.drawn_pages = n;
        let pages: Vec<usize> = std::mem::take(&mut self.redraw).into_iter().filter(|&p| p < n).collect();
        if all { (true, (0..n).collect()) } else { (false, pages) }
    }

    /// Whether the original's pages must be drawn (once, at first).
    pub fn take_source_redraw(&mut self) -> bool {
        std::mem::take(&mut self.src_redraw)
    }

    pub fn render(&self, page: usize, scale: f32) -> (u32, u32, Vec<u8>) {
        self.session.render(page, scale)
    }

    pub fn render_source(&self, page: usize, scale: f32) -> (u32, u32, Vec<u8>) {
        self.source.render(page, scale)
    }

    // ── State ─────────────────────────────────────────────────────────────

    fn selection(&self) -> Option<(Pos, Pos)> {
        let (c, a) = (self.caret.as_ref()?, self.anchor.as_ref()?);
        if !same_para(a, c) || a.off == c.off {
            return None;
        }
        Some(if a.off < c.off { (a.clone(), c.clone()) } else { (c.clone(), a.clone()) })
    }

    fn src_selection(&self) -> Option<(Pos, Pos)> {
        let (c, a) = (self.src_caret.as_ref()?, self.src_anchor.as_ref()?);
        if !same_para(a, c) || a.off == c.off {
            return None;
        }
        Some(if a.off < c.off { (a.clone(), c.clone()) } else { (c.clone(), a.clone()) })
    }

    fn rects(list: Vec<(usize, [f32; 4])>) -> Vec<Value> {
        list.into_iter().map(|(p, [x, y, w, h])| json!({ "page": p, "x": x, "y": y, "w": w, "h": h })).collect()
    }

    fn pid_at(&self, pos: &Pos) -> Option<String> {
        self.session.paragraph_id(pos).map(str::to_owned)
    }

    /// Everything the view shows besides the pages: caret, selections,
    /// formatting at the caret, undo state, the issue under the caret, the
    /// paragraph's status, statistics and marks.
    pub fn state(&mut self) -> Value {
        let text_caret = self.text_caret();
        let caret = match self.caret.clone() {
            Some(pos) => {
                let editable = self.session.editable(&pos);
                match self.session.caret(&pos, self.page) {
                    Some(c) => {
                        self.page = c.page;
                        json!({ "page": c.page, "x": c.x, "top": c.top, "height": c.height, "editable": editable })
                    }
                    None => Value::Null,
                }
            }
            None => Value::Null,
        };
        let selection = self.selection().map(|(a, b)| Self::rects(self.session.selection_rects(&a, &b, self.page))).unwrap_or_default();
        let src_selection = self
            .src_selection()
            .map(|(a, b)| Self::rects(self.source.layout.selection_rects(&self.source.doc, &a, &b, 0).into_iter().map(|(p, r)| (p, [r.x, r.y, r.w, r.h])).collect()))
            .unwrap_or_default();
        let styles = match &self.caret {
            Some(pos) => json!({
                "b": self.session.has_style(pos, Style::Bold),
                "i": self.session.has_style(pos, Style::Italic),
                "u": self.session.has_style(pos, Style::Underline),
            }),
            None => json!({ "b": false, "i": false, "u": false }),
        };
        let para = self.caret.as_ref().and_then(|p| self.pid_at(p)).and_then(|pid| {
            let m = self.meta.get(&pid)?;
            Some(json!({
                "pid": pid,
                "done": self.confirmed.contains(&pid),
                "edited": m.tgt_text != m.src_text,
                "srcChars": ooxml::count_chars(&m.src_text),
                "tgtChars": ooxml::count_chars(&m.tgt_text),
                "claude": self.claude.as_ref().is_some_and(|c| c.by_pid.contains_key(&pid)),
            }))
        });
        json!({
            "caret": caret,
            "selection": selection,
            "sourceSelection": src_selection,
            "styles": styles,
            "canUndo": self.session.can_undo(),
            "canRedo": self.session.can_redo(),
            "issue": self.issue_at_caret(),
            "paragraph": para,
            "textCaret": text_caret,
            "claude": self.claude.as_ref().map(|c| json!({ "matched": c.by_pid.len(), "total": c.total })),
            "stats": self.stats(),
        })
    }

    /// As main.js totals(): characters and words of both, paragraphs done.
    pub fn stats(&self) -> Value {
        let (mut sc, mut tc, mut sw, mut tw, mut total, mut done) = (0, 0, 0, 0, 0, 0);
        for (pid, m) in &self.meta {
            if m.counted {
                sc += ooxml::count_chars(&m.src_text);
                tc += ooxml::count_chars(&m.tgt_text);
                sw += ooxml::count_words(&m.src_text);
                tw += ooxml::count_words(&m.tgt_text);
            }
            if m.translatable {
                total += 1;
                if m.tgt_text != m.src_text || self.confirmed.contains(pid) {
                    done += 1;
                }
            }
        }
        json!({ "srcChars": sc, "tgtChars": tc, "srcWords": sw, "tgtWords": tw, "total": total, "done": done,
                "srcPages": self.source_page_count(), "tgtPages": self.page_count() })
    }

    /// Bars beside paragraphs: edited (changed, not marked done) and done.
    pub fn marks(&self) -> Value {
        let mut out = vec![];
        for (pid, m) in &self.meta {
            let kind = if self.confirmed.contains(pid) {
                "done"
            } else if m.tgt_text != m.src_text {
                "edited"
            } else {
                continue;
            };
            let Some(p) = self.session.locate(pid) else { continue };
            let len = self.session.text_of(&p).map(str::len).unwrap_or(0);
            let rects = self.session.selection_rects(&p, &Pos::new(p.story, p.path.clone(), len), 0);
            let mut by_page: HashMap<usize, (f32, f32, f32)> = HashMap::new();
            for (page, [x, y, _, h]) in rects {
                let e = by_page.entry(page).or_insert((x, y, y + h));
                e.0 = e.0.min(x);
                e.1 = e.1.min(y);
                e.2 = e.2.max(y + h);
            }
            for (page, (x, y0, y1)) in by_page {
                out.push(json!({ "page": page, "x": x, "y": y0, "h": y1 - y0, "kind": kind }));
            }
        }
        Value::Array(out)
    }

    fn issue_at_caret(&self) -> Value {
        if self.selection().is_some() {
            return Value::Null;
        }
        let Some(pos) = &self.caret else { return Value::Null };
        let Some(pid) = self.session.paragraph_id(pos) else { return Value::Null };
        let Some(list) = self.proof.issues.get(pid) else { return Value::Null };
        match list.iter().position(|i| i.a <= pos.off && pos.off <= i.b && i.b > i.a) {
            Some(n) => {
                let i = &list[n];
                json!({ "pid": pid, "n": n, "kind": i.kind, "message": i.message, "short": i.short, "replacements": i.replacements, "word": i.word })
            }
            None => Value::Null,
        }
    }

    /// Caret geometry of a position (paragraph id, byte offset): for jumping
    /// to a paragraph, and for tests to click exactly.
    pub fn caret_at(&self, pid: &str, off: usize) -> Value {
        let Some(p) = self.session.locate(pid) else { return Value::Null };
        match self.session.caret(&Pos::new(p.story, p.path, off), 0) {
            Some(c) => json!({ "page": c.page, "x": c.x, "top": c.top, "height": c.height }),
            None => Value::Null,
        }
    }

    /// Underlines for every proofing issue: {underlines: [{page, x, y, w, kind}], count, status}.
    pub fn issues(&self) -> Value {
        let mut out = vec![];
        let mut count = 0;
        for (pid, list) in &self.proof.issues {
            let Some(p) = self.session.locate(pid) else { continue };
            for i in list {
                count += 1;
                let a = Pos::new(p.story, p.path.clone(), i.a);
                let b = Pos::new(p.story, p.path.clone(), i.b);
                for (page, [x, y, w, h]) in self.session.selection_rects(&a, &b, self.page) {
                    out.push(json!({ "page": page, "x": x, "y": y + h - 1.5, "w": w, "kind": i.kind }));
                }
            }
        }
        json!({ "underlines": out, "count": count, "status": self.proof.status })
    }

    // ── After edits ───────────────────────────────────────────────────────

    fn edited(&mut self, pages: Vec<usize>) {
        self.redraw.extend(pages);
        for pid in self.session.take_edited() {
            self.row_changes.insert(pid.clone());
            self.journal.insert(pid.clone());
            self.proof.issues.remove(&pid);
            self.proof.queue.insert(pid.clone());
            if let Some(m) = self.meta.get_mut(&pid) {
                if let Some(t) = paragraph_text_of(&self.session.parts, &m.path, &pid) {
                    m.tgt_text = t;
                }
            }
        }
        self.last_edit = Some(Instant::now());
    }

    /// What changed since the last call, for a crash-safe journal: each edited
    /// paragraph's `w:p` by id, and changed relationship parts as `rels|path`.
    pub fn take_journal(&mut self) -> Value {
        let mut out = serde_json::Map::new();
        for key in std::mem::take(&mut self.journal) {
            if let Some(path) = key.strip_prefix("rels|") {
                if let Some(x) = self.session.parts.get(path) {
                    out.insert(key.clone(), json!(x));
                }
            } else if let Some(m) = self.meta.get(&key) {
                if let Some(xml) = self.session.parts.get(&m.path) {
                    if let Some(r) = find_paragraph(xml, &key) {
                        out.insert(key.clone(), json!(&xml[r]));
                    }
                }
            }
        }
        Value::Object(out)
    }

    /// The target language changed: proofing follows it.
    pub fn set_language(&mut self, word_code: &str) {
        self.proof.lang = proof::lt_code(word_code);
        self.proof.issues.clear();
        let ids: Vec<String> = self.meta.iter().filter(|(_, m)| m.tgt_text != m.src_text).map(|(p, _)| p.clone()).collect();
        self.proof.queue.extend(ids);
    }

    /// Parts changed since the last call (to save).
    pub fn take_dirty(&mut self) -> Parts {
        self.session.take_dirty()
    }

    /// The project metadata with this session's done marks, ignored issues and statistics.
    pub fn project_meta(&self, project: &Value) -> Value {
        let mut m = project.clone();
        m["confirmed"] = json!(self.confirmed.iter().collect::<Vec<_>>());
        let mut ignored: Vec<&String> = self.proof.ignored.iter().collect();
        ignored.sort();
        m["ignored"] = json!(ignored);
        m["stats"] = self.stats();
        match &self.claude {
            Some(c) => m["claudeSegments"] = json!(c.md),
            None => {
                if let Some(o) = m.as_object_mut() {
                    o.remove("claudeSegments");
                }
            }
        }
        m
    }

    /// The stored state changed elsewhere: carry on from `translated`.
    pub fn replace_translation(&mut self, translated: Parts) -> anyhow::Result<()> {
        let mut target = self.source_parts.clone();
        target.extend(translated);
        self.session.replace_parts(target).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.rows_all = true;
        self.anchor = None;
        if let Some(c) = &self.caret {
            if self.session.text_of(c).is_none_or(|t| c.off > t.len()) {
                self.caret = None;
            }
        }
        let tgt = texts(&self.session.parts, &self.text_parts);
        for (pid, m) in self.meta.iter_mut() {
            if let Some((_, t)) = tgt.get(pid) {
                if &m.tgt_text != t {
                    m.tgt_text = t.clone();
                    self.proof.issues.remove(pid);
                    self.proof.queue.insert(pid.clone());
                }
            }
        }
        self.redraw_all = true;
        Ok(())
    }

    // ── Mouse ─────────────────────────────────────────────────────────────

    /// A click on the translation; `extend` keeps the selection's other end.
    pub fn click(&mut self, page: usize, x: f32, y: f32, extend: bool) -> Value {
        if let Some(hit) = self.session.hit(page, x, y) {
            self.page = page;
            self.goal_x = None;
            if extend {
                if let Some(a) = self.anchor.clone().or_else(|| self.caret.clone()) {
                    let c = if same_para(&a, &hit) {
                        hit
                    } else {
                        let len = self.session.text_of(&a).map(str::len).unwrap_or(0);
                        let after = (hit.story, &hit.path) > (a.story, &a.path);
                        Pos::new(a.story, a.path.clone(), if after { len } else { 0 })
                    };
                    self.anchor = Some(a);
                    self.caret = Some(c);
                    return self.state();
                }
            }
            self.anchor = Some(hit.clone());
            self.caret = Some(hit);
        }
        self.state()
    }

    pub fn select_word(&mut self, page: usize, x: f32, y: f32) -> Value {
        if let Some(hit) = self.session.hit(page, x, y) {
            let (a, b) = self.session.word_at(&hit);
            self.anchor = Some(a);
            self.caret = Some(b);
        }
        self.state()
    }

    /// Triple-click: selects the paragraph.
    pub fn select_paragraph(&mut self, page: usize, x: f32, y: f32) -> Value {
        if let Some(hit) = self.session.hit(page, x, y) {
            let len = self.session.text_of(&hit).map(str::len).unwrap_or(0);
            self.anchor = Some(Pos::new(hit.story, hit.path.clone(), 0));
            self.caret = Some(Pos::new(hit.story, hit.path, len));
        }
        self.state()
    }

    /// Triple-click on the original: selects its paragraph.
    pub fn source_select_paragraph(&mut self, page: usize, x: f32, y: f32) -> Value {
        if let Some(hit) = self.source.layout.hit(page, x, y, wordcraft_doc::StoryRef::Body) {
            let len = self.source.doc.para(hit.story, &hit.path).map(|p| p.text.len()).unwrap_or(0);
            self.src_anchor = Some(Pos::new(hit.story, hit.path.clone(), 0));
            self.src_caret = Some(Pos::new(hit.story, hit.path, len));
        }
        self.state()
    }

    /// A click on the original: selects there (`extend`: dragging or Shift);
    /// a plain click also puts the caret at the start of the translation's
    /// paragraph (as the web editor's click in the original does).
    pub fn source_click(&mut self, page: usize, x: f32, y: f32, extend: bool) -> Value {
        let Some(hit) = self.source.layout.hit(page, x, y, wordcraft_doc::StoryRef::Body) else { return self.state() };
        if extend {
            if let Some(a) = self.src_anchor.clone() {
                let c = if same_para(&a, &hit) {
                    hit
                } else {
                    let len = self.source.doc.para(a.story, &a.path).map(|p| p.text.len()).unwrap_or(0);
                    Pos::new(a.story, a.path.clone(), if (hit.story, &hit.path) > (a.story, &a.path) { len } else { 0 })
                };
                self.src_caret = Some(c);
                return self.state();
            }
        }
        self.src_anchor = Some(hit.clone());
        self.src_caret = Some(hit.clone());
        let pid = self.source.doc.para(hit.story, &hit.path).and_then(|p| p.source.as_deref()).map(str::to_owned);
        if let Some(p) = pid.and_then(|pid| self.session.locate(&pid)) {
            self.anchor = Some(p.clone());
            self.caret = Some(p);
        }
        self.state()
    }

    pub fn source_word(&mut self, page: usize, x: f32, y: f32) -> Value {
        if let Some(hit) = self.source.layout.hit(page, x, y, wordcraft_doc::StoryRef::Body) {
            let t = self.source.doc.para(hit.story, &hit.path).map(|p| p.text.clone()).unwrap_or_default();
            let (mut a, mut b) = (hit.off.min(t.len()), hit.off.min(t.len()));
            let word = |c: char| c.is_alphanumeric() || c == '\'' || c == '’' || c == '-';
            while a > 0 && t[..a].chars().next_back().is_some_and(word) {
                a = t[..a].char_indices().next_back().map(|(i, _)| i).unwrap_or(0);
            }
            while b < t.len() && t[b..].chars().next().is_some_and(word) {
                b += t[b..].chars().next().map(char::len_utf8).unwrap_or(1);
            }
            self.src_anchor = Some(Pos::new(hit.story, hit.path.clone(), a));
            self.src_caret = Some(Pos::new(hit.story, hit.path, b));
        }
        self.state()
    }

    /// What a right-click on the original offers: {text (selected), lookup,
    /// link (label), paragraph (its text)}.
    pub fn source_menu(&mut self, page: usize, x: f32, y: f32) -> Value {
        let hit = self.source.layout.hit(page, x, y, wordcraft_doc::StoryRef::Body);
        let selected = self.src_selection().map(|(a, b)| {
            let t = self.source.doc.para(a.story, &a.path).map(|p| p.text.clone()).unwrap_or_default();
            t[a.off.min(t.len())..b.off.min(t.len())].chars().filter(|c| *c != '\u{FFFC}').collect::<String>()
        });
        let link = hit.as_ref().and_then(|h| self.source_link_at(h));
        let paragraph = hit.as_ref().and_then(|h| self.source.doc.para(h.story, &h.path)).and_then(|p| p.source.as_deref()).and_then(|pid| self.meta.get(pid)).map(|m| m.src_text.clone());
        self.copied_candidate = link.clone();
        json!({
            "text": selected.clone().unwrap_or_default(),
            "lookup": selected.as_deref().and_then(lookup_term),
            "link": link.map(|c| c.clip),
            "paragraph": paragraph,
        })
    }

    /// The link at a position of the original, read from its XML.
    fn source_link_at(&self, at: &Pos) -> Option<Copied> {
        let p = self.source.doc.para(at.story, &at.path)?;
        let rid = p.run_sources.iter().find(|(s, e, _)| (*s as usize) <= at.off && at.off < *e as usize).map(|(_, _, id)| id.to_string())?;
        let pid = p.source.as_deref()?;
        let path = &self.meta.get(pid)?.path;
        let xml = self.source_parts.get(path)?;
        let range = find_paragraph(xml, pid)?;
        let t = Tree::parse(&xml[range]).ok()?;
        let run = ooxml::find_run(&t, &rid)?;
        let rels_xml = babel_pages::read_part(&self.session_original(), &ooxml::rels_path_for(path));
        let rels = ooxml::rels_map(rels_xml.as_deref());
        let link = ooxml::link_of_run(&t, run, &rels)?;
        let style = ooxml::link_style_of(&t, run);
        Some(Copied { clip: link.label(), link, style })
    }

    fn session_original(&self) -> Vec<u8> {
        self.session.original().to_vec()
    }

    /// Copy link (from the last right-click on the original): the text for the clipboard.
    pub fn copy_source_link(&mut self) -> Option<String> {
        let c = self.copied_candidate.take()?;
        let clip = c.clip.clone();
        self.copied = Some(c);
        Some(clip)
    }

    /// The original's selected text, for copying.
    pub fn source_selected_text(&self) -> String {
        self.src_selection()
            .map(|(a, b)| {
                let t = self.source.doc.para(a.story, &a.path).map(|p| p.text.clone()).unwrap_or_default();
                t[a.off.min(t.len())..b.off.min(t.len())].chars().filter(|c| *c != '\u{FFFC}').collect()
            })
            .unwrap_or_default()
    }

    // ── Keys ──────────────────────────────────────────────────────────────

    /// A key; `extend` is Shift held (for moving keys). Answers with the state
    /// and any refusal as `error`.
    pub fn key(&mut self, k: Key, extend: bool) -> Value {
        let Some(pos) = self.caret.clone() else { return json!({ "caret": Value::Null }) };
        let before = self.session.page_count();
        let sel = self.selection();
        let moving = matches!(k, Key::Left | Key::Right | Key::Up | Key::Down | Key::Home | Key::End);
        let keep_goal = matches!(k, Key::Up | Key::Down);
        let mut error = Value::Null;

        let result: Result<Pos, String> = match &k {
            Key::Left if !extend && sel.is_some() => Ok(sel.clone().unwrap().0),
            Key::Right if !extend && sel.is_some() => Ok(sel.clone().unwrap().1),
            Key::Left | Key::Right => {
                let dir = if matches!(k, Key::Left) { -1 } else { 1 };
                Ok(if extend { self.session.step_within(&pos, dir) } else { self.session.step(&pos, dir, self.page) })
            }
            Key::Up | Key::Down => {
                let dir = if matches!(k, Key::Up) { -1 } else { 1 };
                match self.session.line(&pos, self.goal_x, dir, self.page) {
                    Some((p, x)) => {
                        self.goal_x = Some(x);
                        if extend && !same_para(&p, &pos) {
                            let len = self.session.text_of(&pos).map(str::len).unwrap_or(0);
                            Ok(Pos::new(pos.story, pos.path.clone(), if dir < 0 { 0 } else { len }))
                        } else {
                            Ok(p)
                        }
                    }
                    None => Ok(pos.clone()),
                }
            }
            Key::Home | Key::End => Ok(self.session.line_end(&pos, matches!(k, Key::End), self.page)),
            Key::Enter => Ok(self.next_paragraph(&pos).unwrap_or(pos.clone())),
            Key::SelectAll => {
                let len = self.session.text_of(&pos).map(str::len).unwrap_or(0);
                self.anchor = Some(Pos::new(pos.story, pos.path.clone(), 0));
                Ok(Pos::new(pos.story, pos.path.clone(), len))
            }
            Key::Done => {
                // Mark as done (or not) and go on to the next paragraph, as Ctrl+Enter does on the web.
                match self.pid_at(&pos) {
                    Some(pid) => {
                        self.row_changes.insert(pid.clone());
                        let done = !self.confirmed.remove(&pid);
                        if done {
                            self.confirmed.insert(pid);
                        }
                        self.meta_dirty = true;
                        Ok(if done { self.next_paragraph(&pos).unwrap_or(pos.clone()) } else { pos.clone() })
                    }
                    None => Err("This paragraph can't be marked.".into()),
                }
            }
            Key::Backspace | Key::Delete if sel.is_some() => {
                let (a, b) = sel.clone().unwrap();
                let r = self.session.replace(&a, &b, "");
                self.after(r)
            }
            Key::Backspace | Key::Delete => {
                let r = self.session.delete(&pos, if matches!(k, Key::Backspace) { -1 } else { 1 });
                self.after(r)
            }
            Key::LineBreak | Key::Text(_) => {
                let t = match &k {
                    Key::Text(t) => t.clone(),
                    _ => "\n".into(),
                };
                let (a, b) = sel.clone().unwrap_or((pos.clone(), pos.clone()));
                let r = self.session.replace(&a, &b, &t);
                self.after(r)
            }
            Key::Toggle(style) => match sel.clone() {
                Some((a, b)) => {
                    let r = self.session.toggle(&a, &b, *style).map(|pages| (pos.clone(), pages));
                    self.after(r)
                }
                None => Err("Select some text first.".into()),
            },
            Key::Undo | Key::Redo => {
                let r = if matches!(k, Key::Undo) { self.session.undo() } else { self.session.redo() };
                match r {
                    Some(r) => self.after(r),
                    None => Ok(pos.clone()),
                }
            }
        };
        let keeps_selection = matches!(k, Key::SelectAll | Key::Toggle(_));
        match result {
            Ok(p) => {
                if !(moving && extend) && !keeps_selection {
                    self.anchor = Some(p.clone());
                }
                self.caret = Some(p);
            }
            Err(e) => error = json!(e),
        }
        if !keep_goal {
            self.goal_x = None;
        }
        if self.session.page_count() != before {
            self.redraw_all = true;
        }
        let mut v = self.state();
        v["error"] = error;
        v
    }

    fn after(&mut self, r: Result<(Pos, Vec<usize>), EditError>) -> Result<Pos, String> {
        let (p, pages) = r.map_err(|e| e.to_string())?;
        self.edited(pages);
        Ok(p)
    }

    /// The start of the paragraph after the one at `pos`.
    fn next_paragraph(&self, pos: &Pos) -> Option<Pos> {
        let len = self.session.text_of(pos).map(str::len).unwrap_or(0);
        let end = Pos::new(pos.story, pos.path.clone(), len);
        match self.session.line(&end, None, 1, self.page) {
            Some((p, _)) if !same_para(&p, pos) => Some(Pos::new(p.story, p.path, 0)),
            _ => None,
        }
    }

    /// The translation's selected text, for copying.
    pub fn selected_text(&self) -> String {
        self.selection().map(|(a, b)| self.session.text_between(&a, &b)).unwrap_or_default()
    }

    // ── Paragraph actions ─────────────────────────────────────────────────

    /// Puts the original's paragraph back in place of the translation's (undoable).
    pub fn revert_paragraph(&mut self) -> Value {
        let Some(pos) = self.caret.clone() else { return self.state() };
        let res = self.revert_at(&pos);
        self.caret = Some(Pos::new(pos.story, pos.path.clone(), 0));
        self.anchor = self.caret.clone();
        let mut v = self.state();
        if let Err(e) = res {
            v["error"] = json!(e);
        }
        v
    }

    fn revert_at(&mut self, pos: &Pos) -> Result<(), String> {
        let pid = self.pid_at(pos).ok_or("This paragraph can't be reverted.")?;
        let path = self.meta.get(&pid).map(|m| m.path.clone()).ok_or("This paragraph can't be reverted.")?;
        let xml = self.source_parts.get(&path).ok_or("The original is missing.")?;
        let range = find_paragraph(xml, &pid).ok_or("The original paragraph is missing.")?;
        let src = xml[range].to_owned();
        let pages = self.session.rewrite_paragraph(pos, &src, Parts::new()).map_err(|e| e.to_string())?;
        self.edited(pages);
        Ok(())
    }

    // ── Several paragraphs at once (rows picked in the text view) ─────────

    /// Does `f` for each of the paragraphs `pids` (at its start), all of it one
    /// step of undo; then the caret is at the first. Returns how many it did,
    /// and the errors and notes by row number.
    fn each_paragraph(&mut self, pids: &[String], mut f: impl FnMut(&mut Self, &str) -> Result<Option<String>, String>) -> (usize, Vec<(usize, String)>, Vec<(usize, String)>) {
        let mark = self.session.undo_mark();
        let (mut did, mut errors, mut notes) = (0, vec![], vec![]);
        for pid in pids {
            let Some(pos) = self.session.locate(pid) else { continue };
            let n = self.row_n.get(pid).copied().unwrap_or(0);
            self.caret = Some(Pos::new(pos.story, pos.path.clone(), 0));
            self.anchor = self.caret.clone();
            match f(self, pid) {
                Ok(note) => {
                    did += 1;
                    if let Some(note) = note {
                        notes.push((n, note));
                    }
                }
                Err(e) => errors.push((n, e)),
            }
        }
        self.session.join_since(mark);
        if let Some(pos) = pids.first().and_then(|p| self.session.locate(p)) {
            self.caret = Some(Pos::new(pos.story, pos.path.clone(), 0));
            self.anchor = self.caret.clone();
        }
        (did, errors, notes)
    }

    /// "Segments 3, 5 and 9".
    fn rows_named(list: &[(usize, String)]) -> String {
        let ns: Vec<String> = list.iter().filter(|(n, _)| *n > 0).map(|(n, _)| n.to_string()).collect();
        match ns.len() {
            0 => String::new(),
            1 => format!("segment {}", ns[0]),
            k => format!("segments {} and {}", ns[..k - 1].join(", "), ns[k - 1]),
        }
    }

    /// An action on several rows: "claude", "done", "undone" or "revert".
    pub fn rows_action(&mut self, action: &str, pids: &[String]) -> Value {
        match action {
            "claude" => self.insert_claude_rows(pids),
            "done" | "undone" => self.set_done_rows(pids, action == "done"),
            "revert" => self.revert_rows(pids),
            _ => {
                let mut v = self.state();
                v["error"] = json!(format!("Unknown action {action}."));
                v
            }
        }
    }

    /// Claude's translation in each of the paragraphs `pids` that it has (one
    /// step of undo).
    pub fn insert_claude_rows(&mut self, pids: &[String]) -> Value {
        if self.claude.is_none() {
            let mut v = self.state();
            v["error"] = json!("No translation by Claude is attached to this document.");
            return v;
        }
        let has: Vec<String> = pids.iter().filter(|p| self.claude.as_ref().is_some_and(|c| c.by_pid.contains_key(*p))).cloned().collect();
        let without = pids.len() - has.len();
        let (did, errors, notes) = self.each_paragraph(&has, |e, _| e.insert_claude_inner());
        let mut msg = format!("Inserted Claude's translation in {did} of {} segments.", pids.len());
        if without > 0 {
            msg += &format!(" {without} have none in Claude's translation.");
        }
        if !notes.is_empty() {
            msg += &format!(" Check {}: the marks, tabs or links may not have lined up.", Self::rows_named(&notes));
        }
        if !errors.is_empty() {
            msg += &format!(" Couldn't insert in {}: {}", Self::rows_named(&errors), errors[0].1);
        }
        let mut v = self.state();
        if did == 0 && !errors.is_empty() {
            v["error"] = json!(msg);
        } else {
            v["message"] = json!(msg);
        }
        v
    }

    /// Marks the paragraphs `pids` done, or not done.
    pub fn set_done_rows(&mut self, pids: &[String], done: bool) -> Value {
        for pid in pids {
            if !self.meta.contains_key(pid) {
                continue;
            }
            let changed = if done { self.confirmed.insert(pid.clone()) } else { self.confirmed.remove(pid) };
            if changed {
                self.row_changes.insert(pid.clone());
                self.meta_dirty = true;
            }
        }
        self.state()
    }

    /// The original's text back in each of the paragraphs `pids` (one step of undo).
    pub fn revert_rows(&mut self, pids: &[String]) -> Value {
        let (did, errors, _) = self.each_paragraph(pids, |e, _| {
            let pos = e.caret.clone().ok_or("The paragraph is missing.")?;
            e.revert_at(&pos).map(|_| None)
        });
        let mut v = self.state();
        if !errors.is_empty() {
            v["error"] = json!(format!("Reverted {did} of {} segments. Couldn't revert {}: {}", pids.len(), Self::rows_named(&errors), errors[0].1));
        }
        v
    }

    // ── Claude's translation (a segment file) ─────────────────────────────

    /// The original's paragraphs that are segments, in order, walked.
    fn source_segments(&self) -> Vec<(String, Vec<segments::Item>)> {
        segments::of_parts(&self.source_parts, &self.text_parts)
    }

    /// Matches a segment file's segments to this document's paragraphs: by
    /// id where its text agrees, else by text (in order, for repeated text).
    fn index_segments(&self, md: &str) -> Result<Claude, String> {
        let segs = segments::parse(md)?;
        let paras = self.source_segments();
        let keys: Vec<String> = paras.iter().map(|(_, items)| segments::key_of_items(items)).collect();
        let agree = |a: &str, b: &str| a == b || segments::tight(a) == segments::tight(b);
        let at: HashMap<&str, usize> = paras.iter().enumerate().map(|(k, (pid, _))| (pid.as_str(), k)).collect();
        let mut by_pid: HashMap<String, usize> = HashMap::new();
        let mut taken = vec![false; paras.len()];
        let mut pending = vec![];
        for (si, s) in segs.iter().enumerate() {
            if s.target.trim().is_empty() {
                continue;
            }
            let key = segments::key_of_markup(&s.source);
            match s.id.as_deref().and_then(|id| at.get(id)) {
                Some(&k) if !taken[k] && agree(&keys[k], &key) => {
                    taken[k] = true;
                    by_pid.insert(paras[k].0.clone(), si);
                }
                _ => pending.push((si, key)),
            }
        }
        // By text: the next paragraph with that text, in order.
        let mut first_for_key: HashMap<String, usize> = HashMap::new();
        for (si, key) in &pending {
            first_for_key.entry(segments::tight(key)).or_insert(*si);
            let found = (0..paras.len()).find(|&k| !taken[k] && keys[k] == *key).or_else(|| (0..paras.len()).find(|&k| !taken[k] && agree(&keys[k], key)));
            if let Some(k) = found {
                taken[k] = true;
                by_pid.insert(paras[k].0.clone(), *si);
            }
        }
        // More paragraphs with the same text than segments: the same translation.
        for (k, (pid, _)) in paras.iter().enumerate() {
            if !taken[k] {
                if let Some(&si) = first_for_key.get(&segments::tight(&keys[k])) {
                    by_pid.insert(pid.clone(), si);
                }
            }
        }
        Ok(Claude { md: md.to_owned(), segments: segs, by_pid, total: paras.len() })
    }

    /// Attaches a segment file (Claude's translation) to the translation.
    pub fn attach_segments(&mut self, md: &str) -> Value {
        let res = self.index_segments(md);
        let mut msg = None;
        let err = match res {
            Ok(c) if c.by_pid.is_empty() => Some("None of that file's translated segments match this document's paragraphs. Was it made from this document?".to_owned()),
            Ok(c) => {
                msg = Some(format!("Claude's translation attached: {} of {} paragraphs have one. Right-click a paragraph and choose Insert Claude's Translation.", c.by_pid.len(), c.total));
                self.claude = Some(c);
                self.meta_dirty = true;
                self.rows_all = true;
                None
            }
            Err(e) => Some(e),
        };
        let mut v = self.state();
        if let Some(m) = msg {
            v["message"] = json!(m);
        }
        if let Some(e) = err {
            v["error"] = json!(e);
        }
        v
    }

    /// Detaches Claude's translation.
    pub fn remove_segments(&mut self) -> Value {
        if self.claude.take().is_some() {
            self.meta_dirty = true;
            self.rows_all = true;
        }
        self.state()
    }

    /// The document as a segment file to translate: every paragraph with
    /// something to translate, the original quoted, the translation empty.
    pub fn export_segments(&self, document: &str, target_lang: &str) -> String {
        let mut s = segments::header(document, target_lang);
        for (n, (pid, items)) in self.source_segments().iter().enumerate() {
            s.push_str(&segments::write_segment(n + 1, Some(pid), &segments::markup(items), ""));
        }
        s
    }

    /// Replaces the paragraph at the caret with Claude's translation of it
    /// (one step of undo).
    pub fn insert_claude(&mut self) -> Value {
        let res = self.insert_claude_inner();
        let mut v = self.state();
        match res {
            Ok(Some(note)) => v["message"] = json!(note),
            Ok(None) => {}
            Err(e) => v["error"] = json!(e),
        }
        v
    }

    fn insert_claude_inner(&mut self) -> Result<Option<String>, String> {
        let pos = self.caret.clone().ok_or("Click in a paragraph of the translation first.")?;
        let pid = self.pid_at(&pos).ok_or("This paragraph can't be edited.")?;
        let c = self.claude.as_ref().ok_or("No translation by Claude is attached to this document.")?;
        let si = *c.by_pid.get(&pid).ok_or("Claude's translation has no segment for this paragraph.")?;
        let toks = segments::tokens(&c.segments[si].target);
        let start = Pos::new(pos.story, pos.path.clone(), 0);
        let pieces = self.session.pieces(&start).map_err(|e| e.to_string())?;
        let text = self.session.text_of(&start).unwrap_or("").to_owned();

        // The runs that make the objects the segment file numbers {1}, {2}…,
        // and those that are a link's text.
        let (items, links) = self.target_walk(&pid)?;
        let numbered_piece = numbered_pieces(&pieces, &items);

        // The paragraph cut at its numbered objects and tabs (which stay put)
        // into stretches, each a row of regions: plain text, or a link's text.
        // A region may be cut further by what the file doesn't number (field
        // markers, content controls), which stays put too.
        let mut groups: Vec<Vec<Region>> = vec![vec![Region { link: false, subs: vec![] }]];
        let mut marks = String::new();
        let (mut off, mut from) = (0usize, 0usize);
        fn close(groups: &mut [Vec<Region>], from: usize, to: usize) {
            groups.last_mut().unwrap().last_mut().unwrap().subs.push((from, to));
        }
        for (pi, p) in pieces.iter().enumerate() {
            let end = off + p.len();
            let tab = matches!(p, Piece::Atom { text, .. } if text == "\t");
            let object = numbered_piece[pi];
            if object || tab {
                close(&mut groups, from, off);
                marks.push(if tab { 't' } else { 'o' });
                groups.push(vec![Region { link: false, subs: vec![] }]);
                from = end;
            } else if matches!(p, Piece::Fixed { .. }) {
                close(&mut groups, from, off);
                from = end;
            } else if !p.is_empty() {
                let in_link = match p {
                    Piece::Text { rid: Some(r), .. } | Piece::Atom { rid: r, .. } => links.contains(r),
                    Piece::Text { rid: None, base, .. } => base.as_ref().is_some_and(|b| links.contains(b)),
                    _ => groups.last().unwrap().last().unwrap().link,
                };
                if in_link != groups.last().unwrap().last().unwrap().link {
                    close(&mut groups, from, off);
                    from = off;
                    groups.last_mut().unwrap().push(Region { link: in_link, subs: vec![] });
                }
            }
            off = end;
        }
        close(&mut groups, from, off);
        // Only what can be typed over counts as a stretch's text.
        let typed = |a: usize, b: usize| -> String {
            let mut o = String::new();
            let mut at = 0;
            for p in &pieces {
                let end = at + p.len();
                if !matches!(p, Piece::Fixed { .. }) && end > a && at < b {
                    o.push_str(&text[at.max(a)..end.min(b)]);
                }
                at = end;
            }
            o
        };

        // Claude's text between its placeholders.
        let mut chunks: Vec<Vec<Span>> = vec![vec![]];
        let mut order = vec![];
        let mut claude_marks = String::new();
        for t in toks {
            match t {
                segments::Tok::Obj(n) => {
                    order.push(n);
                    claude_marks.push('o');
                    chunks.push(vec![]);
                }
                segments::Tok::Tab => {
                    claude_marks.push('t');
                    chunks.push(vec![]);
                }
                segments::Tok::Text { s, b, i, l } => chunks.last_mut().unwrap().push((s, b, i, l)),
            }
        }
        while chunks.len() > groups.len() {
            let last = chunks.pop().unwrap();
            chunks.last_mut().unwrap().extend(last);
        }
        chunks.resize(groups.len(), vec![]);

        // Unmarked text takes the paragraph's usual look (a heading stays bold).
        let usual = |s: &Session, style: Style| {
            let (mut with, mut all) = (0usize, 0usize);
            let mut o = 0;
            for p in &pieces {
                if let Piece::Text { text: t, .. } = p {
                    for (k, ch) in t.char_indices() {
                        if !ch.is_whitespace() {
                            all += 1;
                            with += s.has_style(&Pos::new(pos.story, pos.path.clone(), o + k), style) as usize;
                        }
                    }
                }
                o += p.len();
            }
            all > 0 && with * 2 > all
        };
        let (usual_b, usual_i) = (usual(&self.session, Style::Bold), usual(&self.session, Style::Italic));

        // Each stretch's new text and its formatting.
        let mut plan: Vec<(usize, usize, String, Vec<(usize, bool, bool)>)> = vec![];
        let mut links_off = false;
        for (regions, spans) in groups.iter().zip(chunks) {
            let original: String = regions.iter().flat_map(|r| r.subs.iter()).map(|&(a, b)| typed(a, b)).collect();
            let spans = edges_like(spans, &original);
            // Regions with text, neighbours of a kind together; the same for Claude's.
            let mut shape: Vec<(bool, Vec<(usize, usize)>)> = vec![];
            for r in regions {
                if r.subs.iter().all(|&(a, b)| typed(a, b).is_empty()) {
                    if let Some(last) = shape.last_mut() {
                        last.1.extend(r.subs.iter().copied());
                    } else {
                        shape.push((r.link, r.subs.clone()));
                    }
                    continue;
                }
                match shape.last_mut() {
                    Some(last) if last.0 == r.link || last.1.iter().all(|&(a, b)| typed(a, b).is_empty()) => {
                        last.0 = r.link;
                        last.1.extend(r.subs.iter().copied());
                    }
                    _ => shape.push((r.link, r.subs.clone())),
                }
            }
            let mut parts: Vec<(bool, Vec<Span>)> = vec![];
            for sp in spans {
                match parts.last_mut() {
                    Some(last) if last.0 == sp.3 => last.1.push(sp),
                    _ => parts.push((sp.3, vec![sp])),
                }
            }
            let kinds = |v: &[(bool, Vec<Span>)]| v.iter().map(|p| p.0).collect::<Vec<_>>();
            let fits = shape.len() == parts.len() && shape.iter().map(|r| r.0).collect::<Vec<_>>() == kinds(&parts);
            let assignment: Vec<(Vec<(usize, usize)>, Vec<Span>)> = if fits || (shape.len() <= 1 && parts.iter().all(|p| !p.0)) {
                shape.into_iter().map(|r| r.1).zip(parts.into_iter().map(|p| p.1).chain(std::iter::repeat(vec![]))).collect()
            } else {
                // The links don't line up: all of it over the stretch, in proportion.
                links_off |= parts.iter().any(|p| p.0) || shape.iter().any(|r| r.0);
                vec![(shape.into_iter().flat_map(|r| r.1).collect(), parts.into_iter().flat_map(|p| p.1).collect())]
            };
            for (subs, spans) in assignment {
                let originals: Vec<String> = subs.iter().map(|&(a, b)| typed(a, b)).collect();
                let original: String = originals.concat();
                let all: String = spans.iter().map(|x| x.0.as_str()).collect();
                let cuts = cut_like(&all, &original, &originals);
                let mut prev = 0;
                for (k, &(a, b)) in subs.iter().enumerate() {
                    let cut = cuts[k];
                    plan.push((a, b, all[prev..cut].to_owned(), slice_spans(&spans, prev, cut)));
                    prev = cut;
                }
            }
        }
        let (story, path) = (pos.story, pos.path.clone());
        let at = move |o: usize| Pos::new(story, path.clone(), o);
        let pages = self
            .session
            .group(|s| {
                let mut pages = vec![];
                for (a, b, t, _) in plan.iter().rev() {
                    let (a, b) = (*a, *b);
                    if a == b || t.is_empty() {
                        if a != b || !t.is_empty() {
                            pages.extend(s.replace(&at(a), &at(b), t)?.1);
                        }
                        continue;
                    }
                    // Into the stretch's own run: after its first character (at a run's
                    // edge, new text would join the run before, as typing does), then
                    // the old text around it goes.
                    let now = s.text_of(&at(0)).unwrap_or("").to_owned();
                    let c1 = a + now[a..].chars().next().map(char::len_utf8).unwrap_or(0);
                    pages.extend(s.replace(&at(c1), &at(c1), t)?.1);
                    let n = t.len();
                    if b > c1 {
                        pages.extend(s.replace(&at(c1 + n), &at(b + n), "")?.1);
                    }
                    pages.extend(s.replace(&at(a), &at(c1), "")?.1);
                }
                // Bold and italic as Claude marked them.
                let mut ranges = vec![];
                let mut shift = 0isize;
                for (a, b, t, spans) in &plan {
                    let mut o = (*a as isize + shift) as usize;
                    for &(len, bb, ii) in spans {
                        ranges.push((o, o + len, bb || usual_b, ii || usual_i));
                        o += len;
                    }
                    shift += t.len() as isize - (*b as isize - *a as isize);
                }
                let now = s.text_of(&at(0)).unwrap_or("").to_owned();
                for style in [Style::Bold, Style::Italic] {
                    for &(x, y, bb, ii) in &ranges {
                        let want = if style == Style::Bold { bb } else { ii };
                        let offs: Vec<usize> = now.char_indices().map(|(k, _)| k).filter(|&k| k >= x && k < y).collect();
                        let mut k = 0;
                        while k < offs.len() {
                            if s.has_style(&at(offs[k]), style) == want {
                                k += 1;
                                continue;
                            }
                            let mut e = k;
                            while e < offs.len() && s.has_style(&at(offs[e]), style) != want {
                                e += 1;
                            }
                            let end = offs.get(e).copied().unwrap_or(y);
                            pages.extend(s.set_style(&at(offs[k]), &at(end), style, want)?);
                            k = e;
                        }
                    }
                }
                Ok(pages)
            })
            .map_err(|e| e.to_string())?;
        self.edited(pages);
        let len = self.session.text_of(&start).map(str::len).unwrap_or(0);
        self.caret = Some(Pos::new(pos.story, pos.path.clone(), len));
        self.anchor = self.caret.clone();
        let in_order = order.windows(2).all(|w| w[0] < w[1]);
        let count = |m: &str, c: char| m.chars().filter(|&x| x == c).count();
        Ok(if claude_marks != marks {
            let (o, t) = (count(&marks, 'o'), count(&marks, 't'));
            let (co, ct) = (count(&claude_marks, 'o'), count(&claude_marks, 't'));
            Some(format!(
                "Inserted, but check it: this paragraph has {o} footnote mark(s), picture(s) or field(s) and {t} tab(s), and Claude's translation marks {co} and {ct}; they keep their order, so check where they ended up."
            ))
        } else if links_off {
            Some("Inserted, but check the links: Claude's translation marks its link text ([…]) differently from this paragraph's links.".to_owned())
        } else if !in_order {
            Some("Inserted. Claude's translation puts the marks in another order; they keep the original's order.".to_owned())
        } else {
            None
        })
    }

    /// The translation's paragraph `pid` walked (as a segment file sees it),
    /// and the runs that are a link's text.
    pub(crate) fn target_walk(&self, pid: &str) -> Result<(Vec<segments::Item>, std::collections::HashSet<String>), String> {
        let path = self.meta.get(pid).map(|m| m.path.clone()).unwrap_or_default();
        let xml = self.session.parts.get(&path).ok_or("The paragraph is missing.")?;
        let range = find_paragraph(xml, pid).ok_or("The paragraph is missing.")?;
        let t = Tree::parse(&xml[range]).map_err(|e| e.to_string())?;
        Ok(segments::walk_with_links(&t, t.root))
    }

    /// The original text of the paragraph at the caret.
    pub fn original_text(&self) -> Option<String> {
        let pid = self.pid_at(self.caret.as_ref()?)?;
        self.meta.get(&pid).map(|m| m.src_text.clone())
    }

    /// Makes the selection a link (Ctrl+K): to the link copied from the
    /// original, else to a web address or `#bookmark` on the clipboard.
    pub fn paste_link(&mut self, clipboard: &str) -> Value {
        let res = self.paste_link_inner(clipboard.trim());
        let mut v = self.state();
        match res {
            Ok(label) => v["message"] = json!(format!("Linked to {label}")),
            Err(e) => v["error"] = json!(e),
        }
        v
    }

    fn paste_link_inner(&mut self, clip: &str) -> Result<String, String> {
        let (a, b) = self.selection().ok_or("Select the text to link first.")?;
        let urlish = |s: &str| {
            let l = s.to_ascii_lowercase();
            (l.starts_with("http://") || l.starts_with("https://") || l.starts_with("mailto:") || l.starts_with("www.")) && !s.contains(char::is_whitespace)
        };
        let copied = match &self.copied {
            Some(c) if clip.is_empty() || clip == c.clip => c.clone(),
            _ if urlish(clip) => Copied {
                link: Link { url: Some(if clip.to_ascii_lowercase().starts_with("www.") { format!("https://{clip}") } else { clip.to_owned() }), anchor: None },
                style: vec![],
                clip: clip.to_owned(),
            },
            _ if clip.starts_with('#') && clip.len() > 1 && !clip.contains(char::is_whitespace) => {
                Copied { link: Link { url: None, anchor: Some(clip[1..].to_owned()) }, style: vec![], clip: clip.to_owned() }
            }
            Some(c) => c.clone(),
            None => return Err("Nothing to paste: right-click a link in the original and choose Copy link, or copy a web address.".into()),
        };
        let pid = self.pid_at(&a).ok_or("This paragraph can't be linked.")?;
        let path = self.meta.get(&pid).map(|m| m.path.clone()).ok_or("This paragraph can't be linked.")?;
        let xml = self.session.parts.get(&path).ok_or("The part is missing.")?.clone();
        let range = find_paragraph(&xml, &pid).ok_or("The paragraph is missing.")?;
        let mut t = Tree::parse(&xml[range]).map_err(|e| e.to_string())?;
        // Offsets: the session's text has objects in it, the run text doesn't.
        let shown = self.session.text_of(&a).unwrap_or("").to_owned();
        let to_run = |off: usize| shown[..off.min(shown.len())].chars().filter(|c| *c != '\u{FFFC}').map(char::len_utf8).sum::<usize>();
        let run_text = ooxml::run_text(&t, t.root);
        if run_text != shown.replace('\u{FFFC}', "") {
            return Err("Babel could not match the selection to the document, so nothing was changed.".into());
        }
        let mut extra = Parts::new();
        let target = match &copied.link {
            Link { url: Some(url), .. } => {
                let rp = ooxml::rels_path_for(&path);
                let rels_xml = self.session.parts.get(&rp).cloned().or_else(|| babel_pages::read_part(self.session.original(), &rp)).unwrap_or_else(ooxml::empty_rels);
                let mut rels = Part::parse(&rels_xml).map_err(|e| e.to_string())?;
                let id = ooxml::add_link_rel(&mut rels, url);
                self.journal.insert(format!("rels|{rp}"));
                extra.insert(rp, rels.serialize());
                LinkTarget::Rel(id)
            }
            Link { anchor: Some(an), .. } => LinkTarget::Anchor(an.clone()),
            _ => return Err("That link has no address.".into()),
        };
        let style = if copied.style.is_empty() { ooxml::default_link_style(self.styles_xml.as_deref()) } else { copied.style.clone() };
        let root = t.root;
        if let Some(problem) = ooxml::link_range(&mut t, root, to_run(a.off), to_run(b.off), &target, &style) {
            return Err(problem.into());
        }
        let pages = self.session.rewrite_paragraph(&a, &t.serialize(), extra).map_err(|e| e.to_string())?;
        self.edited(pages);
        self.anchor = Some(a);
        self.caret = Some(b);
        Ok(copied.link.label())
    }

    // ── Proofing ──────────────────────────────────────────────────────────

    /// The next batch to check, if it's time (`now` paced by the caller).
    pub fn proof_batch(&mut self) -> Option<proof::Batch> {
        if self.last_edit.is_some_and(|t| t.elapsed() < PROOF_DELAY) {
            return None;
        }
        let texts: HashMap<String, String> = self.proof.queue.iter().filter_map(|pid| self.text_of(pid).map(|t| (pid.clone(), t))).collect();
        self.proof.next_batch(|pid| texts.get(pid).cloned())
    }

    /// Results for a batch (LanguageTool's `matches`).
    pub fn proof_result(&mut self, batch: &proof::Batch, matches: &[Value]) {
        let texts: HashMap<String, String> = batch.items.iter().filter_map(|(pid, _)| self.text_of(pid).map(|t| (pid.clone(), t))).collect();
        self.proof.accept(batch, matches, |pid| texts.get(pid).cloned());
        self.proof.status.clear();
    }

    /// A batch failed: check it again later.
    pub fn proof_failed(&mut self, batch: &proof::Batch, status: &str) {
        for (pid, _) in &batch.items {
            self.proof.queue.insert(pid.clone());
        }
        self.proof.status = status.to_owned();
    }

    fn text_of(&self, pid: &str) -> Option<String> {
        let p = self.session.locate(pid)?;
        self.session.text_of(&p).map(str::to_owned)
    }

    pub fn apply_suggestion(&mut self, pid: &str, n: usize, value: &str) -> Value {
        let Some(i) = self.proof.issues.get(pid).and_then(|l| l.get(n)).cloned() else { return self.state() };
        let Some(p) = self.session.locate(pid) else { return self.state() };
        self.anchor = Some(Pos::new(p.story, p.path.clone(), i.a));
        self.caret = Some(Pos::new(p.story, p.path, i.b));
        self.key(Key::Text(value.to_owned()), false)
    }

    /// Ignores an issue in this translation (and every one like it).
    pub fn ignore(&mut self, pid: &str, n: usize) {
        let Some(i) = self.proof.issues.get_mut(pid).and_then(|l| (n < l.len()).then(|| l.remove(n))) else { return };
        self.proof.ignored.insert(format!("{}|{}", i.rule, i.word));
        for list in self.proof.issues.values_mut() {
            list.retain(|x| !(x.rule == i.rule && x.word == i.word));
        }
        self.meta_dirty = true;
    }

    /// Adds an issue's word to the personal dictionary; returns the word.
    pub fn add_to_dictionary(&mut self, pid: &str, n: usize) -> Option<String> {
        let word = self.proof.issues.get(pid)?.get(n)?.word.clone();
        self.proof.dictionary.insert(word.clone());
        for list in self.proof.issues.values_mut() {
            list.retain(|x| !(x.kind == "spelling" && x.word == word));
        }
        Some(word)
    }
}

/// Which of a paragraph's pieces are the objects a segment file numbers
/// {1}, {2}… (from its walk): by their runs; or, when that misses some (a page
/// number the layout draws itself has no run), all of them in order, if
/// there are just as many.
pub(crate) fn numbered_pieces(pieces: &[Piece], items: &[segments::Item]) -> Vec<bool> {
    let numbered: std::collections::HashSet<&String> = items
        .iter()
        .flat_map(|it| match it {
            segments::Item::Obj(rids) => rids.iter().collect(),
            _ => vec![],
        })
        .collect();
    let count = items.iter().filter(|it| matches!(it, segments::Item::Obj(_))).count();
    let by_run: Vec<bool> = pieces.iter().map(|p| p.is_object() && matches!(p, Piece::Fixed { rid: Some(r), .. } if numbered.contains(r))).collect();
    let all_objects = pieces.iter().filter(|p| p.is_object()).count();
    if by_run.iter().filter(|&&x| x).count() != count && all_objects == count {
        pieces.iter().map(|p| p.is_object()).collect()
    } else {
        by_run
    }
}

/// Claude's text: the text, bold, italic, in a link.
type Span = (String, bool, bool, bool);

/// Plain text or a link's text, in stretches between what stays put.
struct Region {
    link: bool,
    subs: Vec<(usize, usize)>,
}

/// Claude's text for a stretch with the original's spaces at its edges (the
/// space before a footnote mark, after a field), and a stretch of only
/// spaces kept as it was.
fn edges_like(mut spans: Vec<Span>, original: &str) -> Vec<Span> {
    let text: String = spans.iter().map(|x| x.0.as_str()).collect();
    if text.trim().is_empty() {
        return if original.trim().is_empty() && !original.is_empty() { vec![(original.to_owned(), false, false, false)] } else { vec![] };
    }
    if let Some(first) = spans.iter_mut().find(|x| !x.0.trim().is_empty()) {
        first.0 = first.0.trim_start().to_owned();
    }
    if let Some(last) = spans.iter_mut().rev().find(|x| !x.0.trim().is_empty()) {
        last.0 = last.0.trim_end().to_owned();
    }
    let first_text = spans.iter().position(|x| !x.0.trim().is_empty()).unwrap_or(0);
    let last_text = spans.iter().rposition(|x| !x.0.trim().is_empty()).unwrap_or(0);
    let mut out: Vec<Span> = spans.drain(first_text..=last_text).collect();
    let lead = &original[..original.len() - original.trim_start().len()];
    let trail = &original[original.trim_end().len()..];
    // The edge spaces go with the text beside them (inside a link if it starts one).
    if !lead.is_empty() {
        let l = out.first().is_some_and(|x| x.3);
        out.insert(0, (lead.to_owned(), false, false, l));
    }
    if !trail.is_empty() {
        let l = out.last().is_some_and(|x| x.3);
        out.push((trail.to_owned(), false, false, l));
    }
    out
}

/// Where to cut `text` among stretches that held `parts` of `original`:
/// in proportion to their lengths, at a word boundary near there (exactly
/// where they were when the text is the original's).
fn cut_like(text: &str, original: &str, parts: &[String]) -> Vec<usize> {
    let n = parts.len();
    let total: usize = parts.iter().map(String::len).sum();
    if n == 1 || total == 0 {
        let mut cuts = vec![text.len(); n];
        cuts[0] = text.len();
        return cuts;
    }
    let mut cuts = Vec::with_capacity(n);
    let mut cum = 0;
    let mut prev = 0;
    for (k, p) in parts.iter().enumerate() {
        if k == n - 1 {
            cuts.push(text.len());
            break;
        }
        cum += p.len();
        let mut cut = if text == original { cum } else { cum * text.len() / total };
        if text != original {
            let window = (text.len() / 8).max(3);
            let boundaries: Vec<usize> = text.char_indices().filter(|(i, _)| *i > 0 && text[..*i].ends_with(char::is_whitespace)).map(|(i, _)| i).collect();
            if let Some(&best) = boundaries.iter().filter(|&&b| b.abs_diff(cut) <= window).min_by_key(|&&b| b.abs_diff(cut)) {
                cut = best;
            }
        }
        while !text.is_char_boundary(cut) {
            cut += 1;
        }
        cut = cut.clamp(prev, text.len());
        cuts.push(cut);
        prev = cut;
    }
    cuts
}

/// The formatting of `spans` between byte offsets `a` and `b` of their text.
fn slice_spans(spans: &[Span], a: usize, b: usize) -> Vec<(usize, bool, bool)> {
    let mut out = vec![];
    let mut at = 0;
    for (t, bb, ii, _) in spans {
        let end = at + t.len();
        let (x, y) = (at.max(a), end.min(b));
        if x < y {
            out.push((y - x, *bb, *ii));
        }
        at = end;
    }
    out
}
