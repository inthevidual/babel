//! Editing the translation on Babel's own pages (babel-pages::edit) from the
//! native view: one open session at a time, caret and keys from QML, pages
//! drawn to PNG in the runtime directory (memory-backed) as they change, and
//! saves into the local store a moment after typing stops.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use babel_pages::edit::Session;
use serde_json::{json, Value};
use wordcraft_doc::Pos;

use crate::app::app;

/// Pixels per point the pages are drawn at.
const SCALE: f32 = 2.0;

struct Native {
    project: String,
    session: Session,
    /// The store's revision the session's parts are based on.
    rev: i64,
    caret: Option<Pos>,
    goal_x: Option<f32>,
    page: usize,
    /// Drawing generation per page, so QML reloads only what changed.
    versions: Vec<u32>,
    dir: PathBuf,
    unsaved_since: Option<Instant>,
    /// Every page must be sent again (the session was reopened).
    redraw_all: bool,
}

static NATIVE: Mutex<Option<Native>> = Mutex::new(None);

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("babel-pages")
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
        // The previous drawing of this page is of no further use.
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

    fn caret_json(&mut self) -> Value {
        let Some(pos) = self.caret.clone() else { return Value::Null };
        let editable = self.session.editable(&pos);
        match self.session.caret(&pos, self.page) {
            Some(c) => {
                self.page = c.page;
                json!({ "page": c.page, "x": c.x, "top": c.top, "height": c.height, "editable": editable })
            }
            None => Value::Null,
        }
    }
}

/// Opens the translation for editing and draws every page.
pub fn open(id: &str) -> anyhow::Result<Value> {
    let a = app();
    let pid = id.to_owned();
    let (docx, source, (rev, translated)) =
        a.db(|s| Ok((s.source_docx(&a.user, &pid)?, s.source_parts(&a.user, &pid)?, s.parts(&a.user, &pid)?)))?;
    let mut parts = source;
    parts.extend(translated);
    let session = Session::open(docx, parts)?;
    let dir = runtime_dir().join(id);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let n = session.page_count();
    let mut s = Native { project: id.to_owned(), session, rev, caret: None, goal_x: None, page: 0, versions: vec![0; n], dir, unsaved_since: None, redraw_all: false };
    for i in 0..n {
        s.draw(i)?;
    }
    let pages = s.pages_json(&(0..n).collect::<Vec<_>>());
    *NATIVE.lock().unwrap() = Some(s);
    Ok(json!({ "pages": pages }))
}

/// Saves what is unsaved, then forgets the session.
pub fn close() {
    let _ = save();
    if let Some(s) = NATIVE.lock().unwrap().take() {
        let _ = std::fs::remove_dir_all(&s.dir);
    }
}

/// Places the caret at a point on a page (points).
pub fn click(page: usize, x: f32, y: f32) -> Value {
    let mut g = NATIVE.lock().unwrap();
    let Some(s) = g.as_mut() else { return Value::Null };
    s.caret = s.session.hit(page, x, y);
    s.page = page;
    s.goal_x = None;
    json!({ "caret": s.caret_json() })
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
    Text(String),
}

/// Handles a key; answers with the caret, the pages drawn again (and the
/// whole page list if their number changed) and any refusal.
pub fn key(k: Key) -> Value {
    let mut g = NATIVE.lock().unwrap();
    let Some(s) = g.as_mut() else { return Value::Null };
    let Some(pos) = s.caret.clone() else { return json!({ "caret": Value::Null }) };
    let before = s.session.page_count();
    let mut error = Value::Null;
    let mut changed: Vec<usize> = vec![];
    let keep_goal = matches!(k, Key::Up | Key::Down);
    let result: Result<Pos, String> = match k {
        Key::Left => Ok(s.session.step(&pos, -1, s.page)),
        Key::Right => Ok(s.session.step(&pos, 1, s.page)),
        Key::Up | Key::Down => {
            let dir = if matches!(k, Key::Up) { -1 } else { 1 };
            match s.session.line(&pos, s.goal_x, dir, s.page) {
                Some((p, x)) => {
                    s.goal_x = Some(x);
                    Ok(p)
                }
                None => Ok(pos.clone()),
            }
        }
        Key::Home | Key::End => {
            let len = s.session.doc.para(pos.story, &pos.path).map(|p| p.text.len()).unwrap_or(0);
            Ok(Pos::new(pos.story, pos.path.clone(), if matches!(k, Key::Home) { 0 } else { len }))
        }
        // Enter goes to the next paragraph (paragraphs are never split).
        Key::Enter => match s.session.line(&pos, None, 1, s.page) {
            Some((p, _)) => Ok(Pos::new(p.story, p.path, 0)),
            None => Ok(pos.clone()),
        },
        Key::Backspace | Key::Delete => {
            let dir = if matches!(k, Key::Backspace) { -1 } else { 1 };
            s.session.delete(&pos, dir).map(|(p, pages)| {
                changed = pages;
                p
            }).map_err(|e| e.to_string())
        }
        Key::Text(t) => s.session.insert(&pos, &t).map(|(p, pages)| {
            changed = pages;
            p
        }).map_err(|e| e.to_string()),
    };
    match result {
        Ok(p) => s.caret = Some(p),
        Err(e) => error = json!(e),
    }
    if !keep_goal {
        s.goal_x = None;
    }
    let n = s.session.page_count();
    s.versions.resize(n, 0);
    for &i in &changed {
        if let Err(e) = s.draw(i) {
            error = json!(format!("Could not draw page {}: {e}", i + 1));
        }
    }
    if !changed.is_empty() {
        s.unsaved_since.get_or_insert_with(Instant::now);
    }
    let caret = s.caret_json();
    let all = n != before || std::mem::take(&mut s.redraw_all);
    json!({
        "caret": caret,
        "pages": s.pages_json(&if all { (0..n).collect() } else { changed }),
        "all": all,
        "error": error,
    })
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
        let (pid, rev) = (id.clone(), saved.rev);
        let (docx, source, translated) =
            a.db(|st| Ok((st.source_docx(&a.user, &pid)?, st.source_parts(&a.user, &pid)?, st.parts_at(&a.user, &pid, rev)?.unwrap_or_default())))?;
        let mut parts = source;
        parts.extend(translated);
        s.session = Session::open(docx, parts)?;
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
