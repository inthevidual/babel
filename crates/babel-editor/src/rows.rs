//! The text view: the document as rows of segments, the original beside the
//! translation, for a grid editor (as in CAT tools) on the desktop and the web.
//!
//! A row is one paragraph with something to translate. Its two sides are
//! spans of text with their bold, italic, underline and link, and *chips*
//! for what can't be typed (footnote marks, pictures, fields, symbols):
//! numbered as a segment file numbers them, so ① in the original is ① in the
//! translation. A chip is one character of the view's text (a circled
//! number), whatever it stands for.
//!
//! Positions in the view are UTF-16 code units of that text (what QML and the
//! browser count); the editor turns them into its own. The text view edits
//! the same translation as the pages, through the same session: typing, its
//! undo, formatting, done marks and every right-click action.

use std::collections::HashMap;

use serde_json::{Value, json};
use wordcraft_doc::{Pos, StoryRef};

use crate::segments::{self, Item, Tok};
use crate::{Editor, Piece, numbered_pieces};
use babel_docx::ooxml::{self, Part};
use babel_docx::para::find_paragraph;
use babel_docx::tree::Tree;

/// The character a chip is shown as: ① to ⑳, then ◆; nothing to see (a
/// zero-width space) for what a segment file doesn't number (a tracked
/// deletion, a content control's edge), which stays put all the same.
pub fn chip_char(n: usize) -> char {
    match n {
        0 => '\u{200B}',
        1..=20 => char::from_u32(0x2460 + n as u32 - 1).unwrap(),
        _ => '◆',
    }
}

pub fn is_chip_char(c: char) -> bool {
    ('\u{2460}'..='\u{2473}').contains(&c) || c == '\u{200B}' || c == '◆'
}

/// A paragraph of the translation as the view shows it: each unit of the
/// view's text and the bytes of the paragraph it stands for.
pub(crate) struct Display {
    /// (start, end) in the paragraph, length in the view (UTF-16).
    units: Vec<(usize, usize, usize)>,
}

impl Display {
    pub(crate) fn to_model(&self, d: usize) -> usize {
        let mut acc = 0;
        for &(a, _, len) in &self.units {
            if d < acc + len.max(1) || (len == 0 && d == acc) {
                return a;
            }
            acc += len;
        }
        self.units.last().map(|u| u.1).unwrap_or(0)
    }

    pub(crate) fn to_view(&self, m: usize) -> usize {
        let mut acc = 0;
        for &(a, b, len) in &self.units {
            if m <= a || m < b {
                return acc;
            }
            acc += len;
        }
        acc
    }
}

fn span(t: &str, b: bool, i: bool, u: bool, l: bool) -> Value {
    json!({ "t": t, "b": b, "i": i, "u": u, "l": l })
}

fn chip(n: usize, title: &str) -> Value {
    json!({ "t": chip_char(n).to_string(), "chip": n, "title": title })
}

/// Adjacent spans with the same look as one.
fn merge(spans: Vec<Value>) -> Vec<Value> {
    let mut out: Vec<Value> = vec![];
    for s in spans {
        if let Some(last) = out.last_mut() {
            let same = last.get("chip").is_none() && s.get("chip").is_none() && ["b", "i", "u", "l"].iter().all(|k| last[k] == s[k]);
            if same {
                let t = format!("{}{}", last["t"].as_str().unwrap_or(""), s["t"].as_str().unwrap_or(""));
                last["t"] = json!(t);
                continue;
            }
        }
        out.push(s);
    }
    out
}

fn has_text(items: &[Item]) -> bool {
    items.iter().any(|it| matches!(it, Item::Text { s, .. } if !s.trim().is_empty()))
}

/// What a part is called beside its rows ("" for the body).
fn part_label(pid: &str) -> &'static str {
    let key = pid.split(':').next().unwrap_or("");
    if key.starts_with("header") {
        "Header"
    } else if key.starts_with("footer") {
        "Footer"
    } else if key.starts_with("footnotes") {
        "Footnote"
    } else if key.starts_with("endnotes") {
        "Endnote"
    } else {
        ""
    }
}

/// The original's paragraph as spans: from its walk, so the chips are numbered
/// as in a segment file; bold and italic when it is mostly so (a heading).
fn source_spans(items: &[Item], usual: (bool, bool)) -> Vec<Value> {
    let mut out = vec![];
    let mut n = 0;
    for it in items {
        out.push(match it {
            Item::Text { s, b, i, l } => span(s, *b || usual.0, *i || usual.1, false, *l),
            Item::Tab => span("\t", false, false, false, false),
            Item::Break => span("\n", false, false, false, false),
            Item::Obj(_) => {
                n += 1;
                chip(n, "")
            }
        });
    }
    merge(out)
}

impl Editor {
    /// Every row of the text view, in document order.
    pub fn rows(&mut self) -> Value {
        let mut out = vec![];
        let mut n = 0;
        self.row_n.clear();
        let usual_src = self.source_usual();
        for path in self.text_parts.clone() {
            let Some(src_xml) = self.source_parts.get(&path) else { continue };
            let Ok(src) = Part::parse(src_xml) else { continue };
            // The translation's paragraphs of this part, walked once.
            let tgt: HashMap<String, Vec<Item>> = self
                .session
                .parts
                .get(&path)
                .and_then(|x| Part::parse(x).ok())
                .map(|t| ooxml::paragraphs(&t.tree).into_iter().filter_map(|p| Some((t.tree.attr(p, "data-bp")?.to_owned(), segments::walk(&t.tree, p)))).collect())
                .unwrap_or_default();
            for p in ooxml::paragraphs(&src.tree) {
                let Some(pid) = src.tree.attr(p, "data-bp").map(str::to_owned) else { continue };
                let items = segments::walk(&src.tree, p);
                if !has_text(&items) {
                    continue;
                }
                n += 1;
                self.row_n.insert(pid.clone(), n);
                let usual = usual_src.get(&pid).copied().unwrap_or_default();
                out.push(self.row_json(&pid, n, &items, usual, tgt.get(&pid).map(Vec::as_slice)));
            }
        }
        Value::Array(out)
    }

    /// One row, as [`Editor::rows`] has it.
    pub fn row(&mut self, pid: &str) -> Option<Value> {
        let n = *self.row_n.get(pid)?;
        let path = self.meta.get(pid)?.path.clone();
        let xml = self.source_parts.get(&path)?;
        let range = find_paragraph(xml, pid)?;
        let t = Tree::parse(&xml[range]).ok()?;
        let items = segments::walk(&t, t.root);
        let usual = self.source_usual_of(pid);
        Some(self.row_json(pid, n, &items, usual, None))
    }

    /// The rows changed since the last call (a paragraph edited, marked done,
    /// Claude's translation attached): `{all, rows}`; all of them after a
    /// change that touches every row.
    pub fn take_row_changes(&mut self) -> Value {
        if std::mem::take(&mut self.rows_all) {
            self.row_changes.clear();
            return json!({ "all": true, "rows": self.rows() });
        }
        let pids: Vec<String> = std::mem::take(&mut self.row_changes).into_iter().collect();
        let rows: Vec<Value> = pids.iter().filter_map(|pid| self.row(pid)).collect();
        json!({ "all": false, "rows": rows })
    }

    fn row_json(&mut self, pid: &str, n: usize, src: &[Item], usual: (bool, bool), tgt_items: Option<&[Item]>) -> Value {
        let tgt = self.target_spans(pid, tgt_items).map(|(_, s)| s);
        let (done, edited) = match self.meta.get(pid) {
            Some(m) => (self.confirmed.contains(pid), m.tgt_text != m.src_text),
            None => (false, false),
        };
        json!({
            "pid": pid,
            "n": n,
            "part": part_label(pid),
            "src": source_spans(src, usual),
            "tgt": tgt.as_ref().ok(),
            "editable": tgt.is_ok(),
            "done": done,
            "edited": edited,
            "claude": self.claude.as_ref().is_some_and(|c| c.by_pid.contains_key(pid)),
        })
    }

    /// The translation's paragraph as the view shows it, and how its text maps.
    pub(crate) fn target_spans(&mut self, pid: &str, items: Option<&[Item]>) -> Result<(Display, Vec<Value>), String> {
        let pos = self.session.locate(pid).ok_or("This paragraph isn't in the document.")?;
        let pieces = self.session.pieces(&pos).map_err(|e| e.to_string())?;
        let walked;
        let items = match items {
            Some(i) => i,
            None => {
                walked = self.target_walk(pid)?.0;
                &walked
            }
        };
        let numbered = numbered_pieces(&pieces, items);
        let wp = self.session.doc.para(pos.story, &pos.path).ok_or("This paragraph isn't in the document.")?;
        // Each run's look: bold, italic, underline, link.
        let mut looks = vec![];
        let mut at = 0;
        for r in &wp.runs {
            let c = self.session.doc.styles.resolve_char(wp.props.style.as_deref(), &r.props);
            looks.push((at, at + r.len, c.bold, c.italic, c.underline != wordcraft_doc::props::Underline::None, r.props.link.is_some()));
            at += r.len;
        }
        let look_at = |o: usize| looks.iter().find(|l| o >= l.0 && o < l.1).map(|l| (l.2, l.3, l.4, l.5)).unwrap_or_default();
        let mut units = vec![];
        let mut spans = vec![];
        let mut off = 0;
        let mut n = 0;
        for (k, p) in pieces.iter().enumerate() {
            let end = off + p.len();
            if p.is_object() {
                units.push((off, end, 1));
                let label = if numbered[k] {
                    n += 1;
                    n
                } else {
                    0
                };
                let shown: String = p.text().chars().filter(|&c| c != '\u{FFFC}').collect();
                spans.push(chip(label, &shown));
            } else if matches!(p, Piece::Fixed { .. }) {
                units.push((off, off, 0));
            } else {
                for (i, c) in p.text().char_indices() {
                    let o = off + i;
                    units.push((o, o + c.len_utf8(), c.len_utf16()));
                    let (b, it, u, l) = look_at(o);
                    spans.push(span(&c.to_string(), b, it, u, l));
                }
            }
            off = end;
        }
        Ok((Display { units }, merge(spans)))
    }

    /// Paragraph id → whether the original's paragraph is mostly bold, italic.
    fn source_usual(&self) -> HashMap<String, (bool, bool)> {
        let doc = &self.source.doc;
        let mut out = HashMap::new();
        let stories = std::iter::once(StoryRef::Body).chain(doc.parts.keys().map(|&k| StoryRef::Part(k)));
        for story in stories {
            for path in doc.para_paths(story) {
                let Some(p) = doc.para(story, &path) else { continue };
                if let Some(id) = p.source.as_deref() {
                    out.insert(id.to_owned(), usual_of(doc, p));
                }
            }
        }
        out
    }

    fn source_usual_of(&self, pid: &str) -> (bool, bool) {
        let doc = &self.source.doc;
        let stories = std::iter::once(StoryRef::Body).chain(doc.parts.keys().map(|&k| StoryRef::Part(k)));
        for story in stories {
            for path in doc.para_paths(story) {
                if let Some(p) = doc.para(story, &path).filter(|p| p.source.as_deref() == Some(pid)) {
                    return usual_of(doc, p);
                }
            }
        }
        (false, false)
    }

    /// Puts the selection in a row (its view positions): anchor to caret.
    /// The right-click actions, keys and formatting then act there.
    pub fn text_select(&mut self, pid: &str, anchor: usize, caret: usize) -> Value {
        let res = (|| {
            let pos = self.session.locate(pid).ok_or("This paragraph isn't in the document.")?;
            let (d, _) = self.target_spans(pid, None)?;
            self.anchor = Some(Pos::new(pos.story, pos.path.clone(), d.to_model(anchor)));
            self.caret = Some(Pos::new(pos.story, pos.path.clone(), d.to_model(caret)));
            self.goal_x = None;
            Ok::<_, String>(())
        })();
        let mut v = self.state();
        if let Err(e) = res {
            v["error"] = json!(e);
        }
        v
    }

    /// Typing in a row: replaces its view's `from..to` with `text` (one step
    /// of undo while typing goes on). Refused (and the row to be shown again)
    /// when it would remove what can't be typed over.
    pub fn text_edit(&mut self, pid: &str, from: usize, to: usize, text: &str) -> Value {
        let res = (|| {
            let pos = self.session.locate(pid).ok_or("This paragraph isn't in the document.")?;
            let (d, _) = self.target_spans(pid, None)?;
            let (a, b) = (d.to_model(from.min(to)), d.to_model(from.max(to)));
            let clean: String = text.chars().filter(|&c| !is_chip_char(c)).map(|c| if matches!(c, '\u{2028}' | '\u{2029}' | '\r') { '\n' } else { c }).collect();
            let at = |o| Pos::new(pos.story, pos.path.clone(), o);
            let (caret, pages) = self.session.replace(&at(a), &at(b), &clean).map_err(|e| e.to_string())?;
            self.edited(pages);
            self.anchor = Some(caret.clone());
            self.caret = Some(caret);
            Ok::<_, String>(())
        })();
        let mut v = self.state();
        if let Err(e) = res {
            v["error"] = json!(e);
            self.row_changes.insert(pid.to_owned());
        }
        v
    }

    /// Where the caret and anchor are in the text view: `{pid, at, anchor}`.
    pub(crate) fn text_caret(&mut self) -> Value {
        let Some(c) = self.caret.clone() else { return Value::Null };
        let Some(pid) = self.pid_at(&c) else { return Value::Null };
        let Ok((d, _)) = self.target_spans(&pid, None) else { return Value::Null };
        let anchor = self.anchor.as_ref().filter(|a| a.story == c.story && a.path == c.path).map(|a| d.to_view(a.off)).unwrap_or(d.to_view(c.off));
        json!({ "pid": pid, "at": d.to_view(c.off), "anchor": anchor })
    }

    /// Claude's translation of a row, as spans (chips numbered as written), for
    /// a panel beside the grid; null when there is none.
    pub fn claude_spans(&self, pid: &str) -> Value {
        let Some(c) = &self.claude else { return Value::Null };
        let Some(&si) = c.by_pid.get(pid) else { return Value::Null };
        let spans: Vec<Value> = segments::tokens(&c.segments[si].target)
            .into_iter()
            .map(|t| match t {
                Tok::Text { s, b, i, l } => span(&s, b, i, false, l),
                Tok::Obj(n) => chip(n, ""),
                Tok::Tab => span("\t", false, false, false, false),
            })
            .collect();
        Value::Array(merge(spans))
    }
}

/// Whether a paragraph is mostly bold, italic (as its runs resolve).
fn usual_of(doc: &wordcraft_doc::Document, p: &wordcraft_doc::Paragraph) -> (bool, bool) {
    let (mut b, mut i, mut all) = (0usize, 0usize, 0usize);
    let mut at = 0;
    for r in &p.runs {
        let text = p.text.get(at..at + r.len).unwrap_or("");
        let n = text.chars().filter(|c| !c.is_whitespace() && *c != '\u{FFFC}').count();
        if n > 0 {
            let c = doc.styles.resolve_char(p.props.style.as_deref(), &r.props);
            all += n;
            b += if c.bold { n } else { 0 };
            i += if c.italic { n } else { 0 };
        }
        at += r.len;
    }
    (all > 0 && b * 2 > all, all > 0 && i * 2 > all)
}
