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
//! offsets mean the same in both.

use std::collections::{BTreeSet, HashMap};

use babel_docx::para::{find_paragraph, runs, RunKind};
use babel_docx::{apply_paragraph, Info, Templates, Token};
use wordcraft_doc::{Document, Path, Pos, StoryRef};
use wordcraft_layout::{DocLayout, LayoutCache, Placed};

use crate::Parts;

#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    /// Text the translator may change: in run `rid`, or typed outside any run
    /// (then formatted like run `base`).
    Text { rid: Option<String>, base: Option<String>, text: String },
    /// A tab or line break that may be deleted.
    Atom { rid: String, text: String },
    /// Anything else, kept as it is (`rid`: its run, if it has one).
    Fixed { rid: Option<String>, text: String },
}

impl Piece {
    fn text(&self) -> &str {
        match self {
            Piece::Text { text, .. } | Piece::Atom { text, .. } | Piece::Fixed { text, .. } => text,
        }
    }
}

#[derive(Debug)]
struct Para {
    part: String,
    pid: String,
    info: Info,
    pieces: Vec<Piece>,
}

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("this paragraph can't be edited here")]
    NotEditable,
    #[error("that would delete something that isn't text (a picture, note or field)")]
    Fixed,
    #[error("{0}")]
    Internal(String),
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
    /// Paragraph id → where it is in the document.
    at: HashMap<String, (StoryRef, Path)>,
    /// Per page, what is drawn on it (to tell which pages an edit changed).
    prints: Vec<String>,
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
        Ok(Session { original, parts, doc, layout, cache, paras: HashMap::new(), templates: Templates::new(), dirty: BTreeSet::new(), at, prints })
    }

    pub fn original(&self) -> &[u8] {
        &self.original
    }

    /// The parts changed since the last call, with their XML.
    pub fn take_dirty(&mut self) -> Parts {
        std::mem::take(&mut self.dirty).into_iter().filter_map(|p| self.parts.get(&p).map(|x| (p, x.clone()))).collect()
    }

    pub fn page_count(&self) -> usize {
        self.layout.pages.len()
    }

    /// Page `i` as straight-alpha RGBA, `scale` pixels per point.
    pub fn render(&self, i: usize, scale: f32) -> (u32, u32, Vec<u8>) {
        let r = wordcraft_render::render_page(&self.doc, &self.layout.pages[i], scale, &crate::render_options());
        (r.width, r.height, r.to_straight())
    }

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

    /// The Babel paragraph id at a position, if it has one.
    pub fn paragraph_id(&self, pos: &Pos) -> Option<&str> {
        self.doc.para(pos.story, &pos.path)?.source.as_deref()
    }

    /// One character left (`-1`) or right, within the paragraph, or to the
    /// end/start of the one before/after.
    pub fn step(&self, pos: &Pos, dir: i32, page_hint: usize) -> Pos {
        if let Some(p) = self.doc.para(pos.story, &pos.path) {
            let t = &p.text;
            if dir < 0 && pos.off > 0 {
                let off = t[..pos.off].char_indices().next_back().map(|(i, _)| i).unwrap_or(0);
                return Pos::new(pos.story, pos.path.clone(), off);
            }
            if dir > 0 && pos.off < t.len() {
                let off = t[pos.off..].char_indices().nth(1).map(|(i, _)| pos.off + i).unwrap_or(t.len());
                return Pos::new(pos.story, pos.path.clone(), off);
            }
        }
        // Across a paragraph boundary: the line above or below, at its end or start.
        match self.layout.vertical(pos, None, dir, page_hint) {
            Some((p, _)) => {
                let len = self.doc.para(p.story, &p.path).map(|x| x.text.len()).unwrap_or(0);
                Pos::new(p.story, p.path, if dir < 0 { len } else { 0 })
            }
            None => pos.clone(),
        }
    }

    /// The position a line up (`-1`) or down, keeping `goal_x`.
    pub fn line(&self, pos: &Pos, goal_x: Option<f32>, dir: i32, page_hint: usize) -> Option<(Pos, f32)> {
        self.layout.vertical(pos, goal_x, dir, page_hint)
    }

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
                    Some(RunKind::Text) if r.texts.get(&rid) == Some(&text) => Piece::Text { rid: Some(rid.clone()), base: Some(rid), text },
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

    /// Types `s` at `pos`; returns where the caret goes and the pages to draw again.
    pub fn insert(&mut self, pos: &Pos, s: &str) -> Result<(Pos, Vec<usize>), EditError> {
        let s: String = s.chars().filter(|c| !c.is_control() || *c == '\t').collect();
        if s.is_empty() {
            return Ok((pos.clone(), vec![]));
        }
        let para = self.para_state(pos)?;
        let off = pos.off;
        // Into the text piece the caret is in; at a boundary, the text before
        // the caret (as a browser types), else the text after it; else a new
        // piece formatted like the last text before the caret.
        let (mut inside, mut before, mut after) = (None, None, None);
        let mut last_rid: Option<String> = None;
        let mut start = 0usize;
        for (i, p) in para.pieces.iter().enumerate() {
            let end = start + p.text().len();
            if let Piece::Text { rid, base, .. } = p {
                if start < off && off < end {
                    inside = Some((i, start));
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
        let props_from;
        match inside.or(before).or(after) {
            Some((i, start)) => {
                let Piece::Text { text, .. } = &mut para.pieces[i] else { unreachable!("only text pieces are targets") };
                text.insert_str(off - start, &s);
                props_from = if off > start { off - 1 } else { start };
            }
            None => {
                let mut start = 0usize;
                let mut idx = para.pieces.len();
                for (i, p) in para.pieces.iter().enumerate() {
                    if start >= off {
                        idx = i;
                        break;
                    }
                    start += p.text().len();
                }
                para.pieces.insert(idx, Piece::Text { rid: None, base: last_rid, text: s.clone() });
                props_from = off.saturating_sub(1);
            }
        }
        let pid = para.pid.clone();
        self.write_back(&pid)?;
        // The same text in WordCraft's paragraph, formatted like the text before it.
        let wp = self.doc.para_mut(pos.story, &pos.path).map_err(|e| EditError::Internal(e.to_string()))?;
        let props = wp.props_at(props_from.min(wp.text.len().saturating_sub(1))).clone();
        let n = wp.insert_text(pos.off, &s, &props).map_err(|e| EditError::Internal(e.to_string()))?;
        let pages = self.relayout();
        Ok((Pos::new(pos.story, pos.path.clone(), pos.off + n), pages))
    }

    /// Deletes the character before (`-1`) or after the caret.
    pub fn delete(&mut self, pos: &Pos, dir: i32) -> Result<(Pos, Vec<usize>), EditError> {
        let wp = self.doc.para(pos.story, &pos.path).ok_or(EditError::NotEditable)?;
        let t = &wp.text;
        let (a, b) = if dir < 0 {
            if pos.off == 0 {
                return Ok((pos.clone(), vec![])); // paragraphs are never merged
            }
            (t[..pos.off].char_indices().next_back().map(|(i, _)| i).unwrap_or(0), pos.off)
        } else {
            if pos.off >= t.len() {
                return Ok((pos.clone(), vec![]));
            }
            (pos.off, t[pos.off..].char_indices().nth(1).map(|(i, _)| pos.off + i).unwrap_or(t.len()))
        };
        let para = self.para_state(pos)?;
        let mut start = 0usize;
        let mut hit = None;
        for (i, p) in para.pieces.iter().enumerate() {
            let end = start + p.text().len();
            if start <= a && b <= end {
                hit = Some((i, start));
                break;
            }
            start = end;
        }
        let (i, start) = hit.ok_or(EditError::Fixed)?;
        match &mut para.pieces[i] {
            Piece::Text { text, .. } => {
                text.replace_range(a - start..b - start, "");
            }
            Piece::Atom { .. } => {
                para.pieces.remove(i);
            }
            Piece::Fixed { .. } => return Err(EditError::Fixed),
        }
        let pid = para.pid.clone();
        self.write_back(&pid)?;
        let wp = self.doc.para_mut(pos.story, &pos.path).map_err(|e| EditError::Internal(e.to_string()))?;
        wp.delete(a, b).map_err(|e| EditError::Internal(e.to_string()))?;
        let pages = self.relayout();
        Ok((Pos::new(pos.story, pos.path.clone(), a), pages))
    }

    /// Rewrites the paragraph's `w:p` from its pieces.
    fn write_back(&mut self, pid: &str) -> Result<(), EditError> {
        let para = self.paras.get_mut(pid).ok_or(EditError::NotEditable)?;
        let mut tokens = vec![];
        let mut last_rid: Option<String> = None;
        for p in &para.pieces {
            match p {
                Piece::Text { rid, base, text } => {
                    if !text.is_empty() {
                        tokens.push(Token::Text { rid: rid.clone(), base: base.clone().or_else(|| last_rid.clone()), text: text.clone(), fmt: None });
                    }
                    if rid.is_some() {
                        last_rid = rid.clone();
                    }
                }
                Piece::Atom { rid, .. } => tokens.push(Token::Atom { rid: rid.clone() }),
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

    /// The pieces of a paragraph, for tests.
    pub fn pieces(&mut self, pos: &Pos) -> Result<Vec<Piece>, EditError> {
        Ok(self.para_state(pos)?.pieces.clone())
    }

    /// Where a paragraph is.
    pub fn locate(&self, pid: &str) -> Option<Pos> {
        self.at.get(pid).map(|(s, p)| Pos::new(*s, p.clone(), 0))
    }
}
