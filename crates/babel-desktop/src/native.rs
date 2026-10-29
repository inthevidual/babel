//! Editing the translation on Babel's own pages (babel-pages::edit) from the
//! native view: one open session at a time, caret, selection and keys from
//! QML, pages drawn to PNG in the runtime directory (memory-backed) as they
//! change, saves into the local store a moment after typing stops, and
//! proofing in the background (proof.rs).

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use babel_pages::edit::{EditError, Session, Style};
use serde_json::{json, Value};
use wordcraft_doc::Pos;

use crate::app::app;
use crate::proof::{self, Proofing};

/// Pixels per point the pages are drawn at.
const SCALE: f32 = 2.0;
/// A paragraph is checked this long after typing in it stops.
const PROOF_DELAY: Duration = Duration::from_millis(1500);

struct Native {
    project: String,
    session: Session,
    /// The store's revision the session's parts are based on.
    rev: i64,
    caret: Option<Pos>,
    /// The other end of the selection, in the caret's paragraph.
    anchor: Option<Pos>,
    goal_x: Option<f32>,
    page: usize,
    /// Drawing generation per page, so QML reloads only what changed.
    versions: Vec<u32>,
    dir: PathBuf,
    unsaved_since: Option<Instant>,
    /// Every page must be sent again (the session was reopened).
    redraw_all: bool,
    proof: Proofing,
    last_edit: Instant,
}

static NATIVE: Mutex<Option<Native>> = Mutex::new(None);

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("babel-pages")
}

fn same_para(a: &Pos, b: &Pos) -> bool {
    a.story == b.story && a.path == b.path
}

impl Native {
    fn draw(&mut self, page: usize) -> anyhow::Result<()> {
        let (w, h, px) = self.session.render(page, SCALE);
        if self.versions.len() <= page {
            self.versions.resize(page + 1, 0);
        }
        self.versions[page] += 1;
        let file = self.file(page);
        let mut out = std::io::BufWriter::new(std::fs::File::create(&file)?);
        use image::ImageEncoder;
        image::codecs::png::PngEncoder::new_with_quality(&mut out, image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::NoFilter)
            .write_image(&px, w, h, image::ExtendedColorType::Rgba8)?;
        if self.versions[page] > 1 {
            let _ = std::fs::remove_file(self.dir.join(format!("p{page}-v{}.png", self.versions[page] - 1)));
        }
        Ok(())
    }

    fn file(&self, page: usize) -> PathBuf {
        self.dir.join(format!("p{page}-v{}.png", self.versions[page]))
    }

    fn pages_json(&self, which: &[usize]) -> Vec<Value> {
        which
            .iter()
            .filter_map(|&i| {
                let p = self.session.layout.pages.get(i)?;
                Some(json!({ "index": i, "url": format!("file://{}", self.file(i).display()), "w": p.w, "h": p.h }))
            })
            .collect()
    }

    /// The selection as (start, end), if there is one.
    fn selection(&self) -> Option<(Pos, Pos)> {
        let (c, a) = (self.caret.as_ref()?, self.anchor.as_ref()?);
        if !same_para(a, c) || a.off == c.off {
            return None;
        }
        Some(if a.off < c.off { (a.clone(), c.clone()) } else { (c.clone(), a.clone()) })
    }

    fn state_json(&mut self) -> Value {
        let Some(pos) = self.caret.clone() else { return json!({ "caret": Value::Null }) };
        let editable = self.session.editable(&pos);
        let caret = match self.session.caret(&pos, self.page) {
            Some(c) => {
                self.page = c.page;
                json!({ "page": c.page, "x": c.x, "top": c.top, "height": c.height, "editable": editable })
            }
            None => Value::Null,
        };
        let selection: Vec<Value> = self
            .selection()
            .map(|(a, b)| self.session.selection_rects(&a, &b, self.page))
            .unwrap_or_default()
            .into_iter()
            .map(|(p, [x, y, w, h])| json!({ "page": p, "x": x, "y": y, "w": w, "h": h }))
            .collect();
        let styles = json!({
            "b": self.session.has_style(&pos, Style::Bold),
            "i": self.session.has_style(&pos, Style::Italic),
            "u": self.session.has_style(&pos, Style::Underline),
        });
        json!({
            "caret": caret,
            "selection": selection,
            "styles": styles,
            "canUndo": self.session.can_undo(),
            "canRedo": self.session.can_redo(),
            "issue": self.issue_at_caret(),
        })
    }

    /// The proofing issue the (collapsed) caret is in.
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

    /// After an edit: pages drawn again, the paragraph queued for proofing.
    fn edited(&mut self, pages: Vec<usize>, out: &mut Vec<usize>) -> anyhow::Result<()> {
        for &i in &pages {
            self.draw(i)?;
        }
        if !pages.is_empty() {
            self.unsaved_since.get_or_insert_with(Instant::now);
        }
        for pid in self.session.take_edited() {
            self.proof.issues.remove(&pid);
            self.proof.queue.insert(pid);
        }
        self.last_edit = Instant::now();
        out.extend(pages);
        Ok(())
    }

    fn text_of(&self, pid: &str) -> Option<String> {
        let p = self.session.locate(pid)?;
        self.session.text_of(&p).map(str::to_owned)
    }
}

type Opened = (Vec<u8>, babel_pages::Parts, babel_pages::Parts, i64);

fn reopen_parts(id: &str, rev: Option<i64>) -> anyhow::Result<Opened> {
    let a = app();
    let pid = id.to_owned();
    a.db(|s| {
        let docx = s.source_docx(&a.user, &pid)?;
        let source = s.source_parts(&a.user, &pid)?;
        let (rev, translated) = match rev {
            Some(r) => (r, s.parts_at(&a.user, &pid, r)?.unwrap_or_default()),
            None => s.parts(&a.user, &pid)?,
        };
        let mut target = source.clone();
        target.extend(translated);
        Ok((docx, source, target, rev))
    })
    .map_err(Into::into)
}

/// Opens the translation for editing and draws every page.
pub fn open(id: &str) -> anyhow::Result<Value> {
    let a = app();
    let (docx, source, target, rev) = reopen_parts(id, None)?;
    let meta = {
        let pid = id.to_owned();
        a.db(|s| s.get_project(&a.user, &pid))?
    };
    let session = Session::open(docx.clone(), target)?;
    let dir = runtime_dir().join(id);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;

    // Proofing: paragraphs that differ from the original are checked.
    let mut proof = Proofing { lang: meta["targetLang"].as_str().and_then(proof::lt_code), ..Default::default() };
    proof.ignored = meta["ignored"].as_array().map(|v| v.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default();
    if proof.lang.is_some() {
        if let Ok(src) = babel_pages::lay_out(&babel_pages::renderable(&docx, &source)?) {
            for (story, path) in src.layout.index.keys() {
                if let Some(p) = src.doc.para(*story, path) {
                    if let Some(id) = p.source.as_deref() {
                        proof.source.insert(id.to_owned(), p.text.clone());
                    }
                }
            }
        }
        let ids: Vec<String> = proof.source.keys().cloned().collect();
        for pid in ids {
            let now = session.locate(&pid).and_then(|p| session.text_of(&p).map(str::to_owned));
            if now.is_some() && now.as_ref() != proof.source.get(&pid) {
                proof.queue.insert(pid);
            }
        }
    }

    let n = session.page_count();
    let mut s = Native {
        project: id.to_owned(),
        session,
        rev,
        caret: None,
        anchor: None,
        goal_x: None,
        page: 0,
        versions: vec![0; n],
        dir,
        unsaved_since: None,
        redraw_all: false,
        proof,
        last_edit: Instant::now() - PROOF_DELAY,
    };
    for i in 0..n {
        s.draw(i)?;
    }
    let pages = s.pages_json(&(0..n).collect::<Vec<_>>());
    let proofing = s.proof.lang.is_some();
    *NATIVE.lock().unwrap() = Some(s);
    if proofing {
        start_proofing(id.to_owned());
    }
    Ok(json!({ "pages": pages, "proofing": proofing }))
}

/// Opens the stored state again (another device's edits came in), keeping
/// the caret where it can be kept. Null if nothing needs doing.
pub fn reload(id: &str) -> Value {
    let mut g = NATIVE.lock().unwrap();
    let Some(s) = g.as_mut().filter(|s| s.project == id && s.unsaved_since.is_none()) else { return Value::Null };
    let Ok((docx, _, target, rev)) = reopen_parts(id, None) else { return Value::Null };
    if rev == s.rev {
        return Value::Null;
    }
    let Ok(session) = Session::open(docx, target) else { return Value::Null };
    s.session = session;
    s.rev = rev;
    s.anchor = None;
    if let Some(c) = &s.caret {
        if s.session.text_of(c).is_none_or(|t| c.off > t.len()) {
            s.caret = None;
        }
    }
    let n = s.session.page_count();
    s.versions.resize(n, 0);
    for i in 0..n {
        let _ = s.draw(i);
    }
    // Paragraphs that now differ from what was checked are checked again.
    let ids: Vec<String> = s.proof.source.keys().cloned().collect();
    for pid in ids {
        if s.text_of(&pid) != s.proof.source.get(&pid).cloned() {
            s.proof.issues.remove(&pid);
            s.proof.queue.insert(pid);
        }
    }
    let mut v = s.state_json();
    v["pages"] = json!(s.pages_json(&(0..n).collect::<Vec<_>>()));
    v["all"] = json!(true);
    v
}

/// The project being edited, if any.
pub fn current() -> Option<String> {
    NATIVE.lock().unwrap().as_ref().map(|s| s.project.clone())
}

/// Saves what is unsaved, then forgets the session.
pub fn close() {
    let _ = save();
    if let Some(s) = NATIVE.lock().unwrap().take() {
        let _ = std::fs::remove_dir_all(&s.dir);
    }
}

/// Mouse: places the caret at a point (points) on a page; with `extend`
/// (Shift, or dragging) the selection's other end stays where it was.
pub fn click(page: usize, x: f32, y: f32, extend: bool) -> Value {
    let mut g = NATIVE.lock().unwrap();
    let Some(s) = g.as_mut() else { return Value::Null };
    let Some(hit) = s.session.hit(page, x, y) else { return s.state_json() };
    s.page = page;
    s.goal_x = None;
    if extend {
        if let Some(a) = s.anchor.clone().or_else(|| s.caret.clone()) {
            // A selection stays within one paragraph, as in the web editor.
            let c = if same_para(&a, &hit) {
                hit
            } else {
                let len = s.session.text_of(&a).map(str::len).unwrap_or(0);
                let after = (hit.story, &hit.path) > (a.story, &a.path);
                Pos::new(a.story, a.path.clone(), if after { len } else { 0 })
            };
            s.anchor = Some(a);
            s.caret = Some(c);
            return s.state_json();
        }
    }
    s.anchor = Some(hit.clone());
    s.caret = Some(hit);
    s.state_json()
}

/// Double-click: selects the word.
pub fn select_word(page: usize, x: f32, y: f32) -> Value {
    let mut g = NATIVE.lock().unwrap();
    let Some(s) = g.as_mut() else { return Value::Null };
    if let Some(hit) = s.session.hit(page, x, y) {
        let (a, b) = s.session.word_at(&hit);
        s.anchor = Some(a);
        s.caret = Some(b);
    }
    s.state_json()
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
    Toggle(Style),
    Text(String),
}

/// Handles a key (`extend`: Shift held, for moving keys); answers with the
/// caret and selection, the pages drawn again (and the whole page list if
/// their number changed) and any refusal.
pub fn key(k: Key, extend: bool) -> Value {
    let mut g = NATIVE.lock().unwrap();
    let Some(s) = g.as_mut() else { return Value::Null };
    let Some(pos) = s.caret.clone() else { return json!({ "caret": Value::Null }) };
    let before = s.session.page_count();
    let mut error = Value::Null;
    let mut changed: Vec<usize> = vec![];
    let sel = s.selection();
    let moving = matches!(k, Key::Left | Key::Right | Key::Up | Key::Down | Key::Home | Key::End);
    let keep_goal = matches!(k, Key::Up | Key::Down);

    fn edit(s: &mut Native, r: Result<(Pos, Vec<usize>), EditError>, changed: &mut Vec<usize>) -> Result<Pos, String> {
        let (p, pages) = r.map_err(|e| e.to_string())?;
        s.edited(pages, changed).map_err(|e| e.to_string())?;
        Ok(p)
    }
    let result: Result<Pos, String> = match &k {
        // Moving: with a selection and no Shift, Left/Right go to its ends.
        Key::Left if !extend && sel.is_some() => Ok(sel.clone().unwrap().0),
        Key::Right if !extend && sel.is_some() => Ok(sel.clone().unwrap().1),
        Key::Left | Key::Right => {
            let dir = if matches!(k, Key::Left) { -1 } else { 1 };
            Ok(if extend { s.session.step_within(&pos, dir) } else { s.session.step(&pos, dir, s.page) })
        }
        Key::Up | Key::Down => {
            let dir = if matches!(k, Key::Up) { -1 } else { 1 };
            match s.session.line(&pos, s.goal_x, dir, s.page) {
                Some((p, x)) => {
                    s.goal_x = Some(x);
                    if extend && !same_para(&p, &pos) {
                        let len = s.session.text_of(&pos).map(str::len).unwrap_or(0);
                        Ok(Pos::new(pos.story, pos.path.clone(), if dir < 0 { 0 } else { len }))
                    } else {
                        Ok(p)
                    }
                }
                None => Ok(pos.clone()),
            }
        }
        Key::Home | Key::End => Ok(s.session.line_end(&pos, matches!(k, Key::End), s.page)),
        // Enter goes to the start of the next paragraph (paragraphs are never split).
        Key::Enter => {
            let len = s.session.text_of(&pos).map(str::len).unwrap_or(0);
            let end = Pos::new(pos.story, pos.path.clone(), len);
            match s.session.line(&end, None, 1, s.page) {
                Some((p, _)) if !same_para(&p, &pos) => Ok(Pos::new(p.story, p.path, 0)),
                _ => Ok(end),
            }
        }
        Key::SelectAll => {
            let len = s.session.text_of(&pos).map(str::len).unwrap_or(0);
            s.anchor = Some(Pos::new(pos.story, pos.path.clone(), 0));
            Ok(Pos::new(pos.story, pos.path.clone(), len))
        }
        Key::Backspace | Key::Delete if sel.is_some() => {
            let (a, b) = sel.clone().unwrap();
            let r = s.session.replace(&a, &b, "");
            edit(s, r, &mut changed)
        }
        Key::Backspace | Key::Delete => {
            let r = s.session.delete(&pos, if matches!(k, Key::Backspace) { -1 } else { 1 });
            edit(s, r, &mut changed)
        }
        Key::LineBreak | Key::Text(_) => {
            let t = match &k {
                Key::Text(t) => t.as_str(),
                _ => "\n",
            };
            let (a, b) = sel.clone().unwrap_or((pos.clone(), pos.clone()));
            let r = s.session.replace(&a, &b, t);
            edit(s, r, &mut changed)
        }
        Key::Toggle(style) => match sel.clone() {
            Some((a, b)) => {
                let r = s.session.toggle(&a, &b, *style).map(|pages| (pos.clone(), pages));
                edit(s, r, &mut changed)
            }
            None => Err("Select some text first.".into()),
        },
        Key::Undo | Key::Redo => {
            let r = if matches!(k, Key::Undo) { s.session.undo() } else { s.session.redo() };
            match r {
                Some(r) => edit(s, r, &mut changed),
                None => Ok(pos.clone()),
            }
        }
    };
    let keeps_selection = matches!(k, Key::SelectAll | Key::Toggle(_));
    match result {
        Ok(p) => {
            if !(moving && extend) && !keeps_selection {
                s.anchor = Some(p.clone());
            }
            s.caret = Some(p);
        }
        Err(e) => error = json!(e),
    }
    if !keep_goal {
        s.goal_x = None;
    }
    let n = s.session.page_count();
    s.versions.resize(n, 0);
    let mut v = s.state_json();
    let all = n != before || std::mem::take(&mut s.redraw_all);
    v["pages"] = json!(s.pages_json(&if all { (0..n).collect() } else { changed }));
    v["all"] = json!(all);
    v["error"] = error;
    v
}

/// The selected text, for copying.
pub fn selected_text() -> String {
    let g = NATIVE.lock().unwrap();
    let Some(s) = g.as_ref() else { return String::new() };
    s.selection().map(|(a, b)| s.session.text_between(&a, &b)).unwrap_or_default()
}

/// Puts a proofing suggestion in place of the issue's text.
pub fn apply_suggestion(pid: &str, n: usize, value: &str) -> Value {
    {
        let mut g = NATIVE.lock().unwrap();
        let Some(s) = g.as_mut() else { return Value::Null };
        let Some(i) = s.proof.issues.get(pid).and_then(|l| l.get(n)).cloned() else { return Value::Null };
        let Some(p) = s.session.locate(pid) else { return Value::Null };
        s.anchor = Some(Pos::new(p.story, p.path.clone(), i.a));
        s.caret = Some(Pos::new(p.story, p.path, i.b));
    }
    key(Key::Text(value.to_owned()), false)
}

/// Ignores an issue in this translation (shared with the web editor).
pub fn ignore(pid: &str, n: usize) -> anyhow::Result<()> {
    let mut g = NATIVE.lock().unwrap();
    let Some(s) = g.as_mut() else { return Ok(()) };
    let Some(i) = s.proof.issues.get_mut(pid).and_then(|l| (n < l.len()).then(|| l.remove(n))) else { return Ok(()) };
    s.proof.ignored.insert(format!("{}|{}", i.rule, i.word));
    // Every issue of the same rule and word goes.
    for list in s.proof.issues.values_mut() {
        list.retain(|x| !(x.rule == i.rule && x.word == i.word));
    }
    let a = app();
    let id = s.project.clone();
    let mut meta = a.db(|st| st.get_project(&a.user, &id))?;
    let mut ignored: Vec<&String> = s.proof.ignored.iter().collect();
    ignored.sort();
    meta["ignored"] = json!(ignored);
    a.db(|st| st.save_meta(&a.user, &id, &meta))?;
    Ok(())
}

/// Underlines for every proofing issue: {underlines: [{page, x, y, w, kind}] in points, status}.
pub fn issues() -> Value {
    let g = NATIVE.lock().unwrap();
    let Some(s) = g.as_ref() else { return json!({ "underlines": [] }) };
    let mut out = vec![];
    let mut count = 0;
    for (pid, list) in &s.proof.issues {
        let Some(p) = s.session.locate(pid) else { continue };
        for i in list {
            count += 1;
            let a = Pos::new(p.story, p.path.clone(), i.a);
            let b = Pos::new(p.story, p.path.clone(), i.b);
            for (page, [x, y, w, h]) in s.session.selection_rects(&a, &b, s.page) {
                out.push(json!({ "page": page, "x": x, "y": y + h - 1.5, "w": w, "kind": i.kind }));
            }
        }
    }
    json!({ "underlines": out, "count": count, "status": s.proof.status })
}

/// Checks queued paragraphs in the background while the session is open.
fn start_proofing(project: String) {
    let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().expect("HTTP client");
    app().rt.spawn(async move {
        let mut backoff = Duration::ZERO;
        loop {
            tokio::time::sleep(Duration::from_millis(400)).await;
            let (batch, lang) = {
                let mut g = NATIVE.lock().unwrap();
                let Some(s) = g.as_mut().filter(|s| s.project == project) else { return };
                if s.last_edit.elapsed() < PROOF_DELAY {
                    continue;
                }
                let texts: std::collections::HashMap<String, String> =
                    s.proof.queue.iter().filter_map(|pid| s.text_of(pid).map(|t| (pid.clone(), t))).collect();
                let lang = s.proof.lang.clone();
                (s.proof.next_batch(|pid| texts.get(pid).cloned()), lang)
            };
            let (Some(batch), Some(lang)) = (batch, lang) else { continue };
            let joined = batch.items.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n\n");
            let res = proof::check(&http, &lang, &joined).await;
            {
                let mut g = NATIVE.lock().unwrap();
                let Some(s) = g.as_mut().filter(|s| s.project == project) else { return };
                match res {
                    Ok(matches) => {
                        backoff = Duration::ZERO;
                        let texts: std::collections::HashMap<String, String> =
                            batch.items.iter().filter_map(|(pid, _)| s.text_of(pid).map(|t| (pid.clone(), t))).collect();
                        s.proof.accept(&batch, &matches, |pid| texts.get(pid).cloned());
                        s.proof.status = String::new();
                    }
                    Err(e) => {
                        for (pid, _) in &batch.items {
                            s.proof.queue.insert(pid.clone());
                        }
                        let (wait, status) = match e {
                            proof::Failure::Busy => (Duration::from_secs(4), "LanguageTool rate limit — retrying shortly".to_owned()),
                            proof::Failure::Other(m) => (Duration::from_secs(10), m),
                        };
                        backoff = (backoff * 2).max(wait).min(Duration::from_secs(120));
                        s.proof.status = status;
                    }
                }
            }
            crate::bridge::notify_issues();
            tokio::time::sleep(proof::pace() + backoff).await;
        }
    });
}

/// Saves the changed parts if there are any.
pub fn save() -> anyhow::Result<bool> {
    let mut g = NATIVE.lock().unwrap();
    let Some(s) = g.as_mut() else { return Ok(false) };
    if s.unsaved_since.is_none() {
        return Ok(false);
    }
    let dirty = s.session.take_dirty();
    if dirty.is_empty() {
        s.unsaved_since = None;
        return Ok(false);
    }
    let a = app();
    let (id, base) = (s.project.clone(), s.rev);
    let meta = {
        let pid = id.clone();
        let mut m = a.db(|st| st.get_project(&a.user, &pid))?;
        m["updatedAt"] = json!(babel_core::store::now_ms());
        m
    };
    let saved = a.db(|st| st.save_parts(&a.user, &id, base, Some(&meta), &dirty, Some("native")))?;
    s.rev = saved.rev;
    s.unsaved_since = None;
    if saved.merged {
        // Edits from another device came in meanwhile and were merged in the
        // store: carry on from the merged state, or the next save would put
        // this session's older copy of their paragraphs back.
        let (docx, _, target, _) = reopen_parts(&id, Some(saved.rev))?;
        s.session = Session::open(docx, target)?;
        s.anchor = None;
        let n = s.session.page_count();
        s.versions.resize(n, 0);
        for i in 0..n {
            s.draw(i)?;
        }
        s.redraw_all = true;
    }
    a.state.notify(babel_server::Change { user: a.user.id, project: id, rev: saved.rev, origin: Some("native".into()), deleted: false });
    Ok(true)
}
