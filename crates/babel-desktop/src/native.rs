//! The native editor in the desktop app: babel-editor's Editor, with its
//! pages drawn to PNG in the runtime directory (memory-backed), saved into
//! the local store a moment after typing stops, and its proofing requests
//! sent to LanguageTool in the background.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use babel_editor::{proof, Editor, Key};
use serde_json::{json, Value};

use crate::app::app;

/// Pixels per point the pages are drawn at.
const SCALE: f32 = 2.0;

struct Native {
    project: String,
    editor: Editor,
    /// The store's revision the editor's parts are based on.
    rev: i64,
    /// Drawing generation per page, so QML reloads only what changed.
    versions: Vec<u32>,
    src_versions: Vec<u32>,
    dir: PathBuf,
    unsaved_since: Option<Instant>,
}

static NATIVE: Mutex<Option<Native>> = Mutex::new(None);

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("babel-pages")
}

fn write_png(file: &std::path::Path, w: u32, h: u32, px: &[u8]) -> anyhow::Result<()> {
    use image::ImageEncoder;
    let mut out = std::io::BufWriter::new(std::fs::File::create(file)?);
    image::codecs::png::PngEncoder::new_with_quality(&mut out, image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::NoFilter)
        .write_image(px, w, h, image::ExtendedColorType::Rgba8)?;
    Ok(())
}

/// Proofing settings, kept on this computer: {picky, dictionary: [...]}.
fn proof_settings() -> Value {
    app().db(|s| s.get_kv("settings.proof")).ok().flatten().and_then(|r| serde_json::from_str(&r).ok()).unwrap_or_else(|| json!({ "picky": false, "dictionary": [] }))
}

fn save_proof_settings(v: &Value) {
    let raw = v.to_string();
    let _ = app().db(|s| s.set_kv("settings.proof", Some(&raw)));
}

impl Native {
    fn file(&self, side: &str, page: usize) -> PathBuf {
        let v = if side == "t" { &self.versions } else { &self.src_versions };
        self.dir.join(format!("{side}{page}-v{}.png", v[page]))
    }

    fn draw(&mut self, page: usize) -> anyhow::Result<()> {
        if self.versions.len() <= page {
            self.versions.resize(page + 1, 0);
        }
        let old = (self.versions[page] > 0).then(|| self.file("t", page));
        self.versions[page] += 1;
        let (w, h, px) = self.editor.render(page, SCALE);
        write_png(&self.file("t", page), w, h, &px)?;
        if let Some(old) = old {
            let _ = std::fs::remove_file(old);
        }
        Ok(())
    }

    /// Draws what changed; returns the translation's pages to reload in QML.
    fn redraw(&mut self) -> (bool, Vec<Value>) {
        let (all, pages) = self.editor.take_redraw();
        self.versions.resize(self.editor.page_count(), 0);
        for &i in &pages {
            let _ = self.draw(i);
        }
        if !pages.is_empty() {
            self.unsaved_since.get_or_insert_with(Instant::now);
        }
        (all, self.pages_json(&pages))
    }

    fn pages_json(&self, which: &[usize]) -> Vec<Value> {
        let sizes = self.editor.page_sizes();
        which
            .iter()
            .filter_map(|&i| {
                let s = sizes["target"].get(i)?;
                Some(json!({ "index": i, "url": format!("file://{}", self.file("t", i).display()), "w": s["w"], "h": s["h"] }))
            })
            .collect()
    }

    fn source_json(&mut self) -> anyhow::Result<Vec<Value>> {
        let n = self.editor.source_page_count();
        if self.editor.take_source_redraw() {
            self.src_versions = vec![1; n];
            for i in 0..n {
                let (w, h, px) = self.editor.render_source(i, SCALE);
                write_png(&self.file("s", i), w, h, &px)?;
            }
        }
        let sizes = self.editor.page_sizes();
        Ok((0..n).map(|i| json!({ "index": i, "url": format!("file://{}", self.file("s", i).display()), "w": sizes["source"][i]["w"], "h": sizes["source"][i]["h"] })).collect())
    }

    /// The editor's answer with the pages drawn again, marks and project meta saved.
    fn answer(&mut self, mut v: Value) -> Value {
        let (all, pages) = self.redraw();
        v["pages"] = json!(pages);
        v["all"] = json!(all);
        v["marks"] = self.editor.marks();
        if self.editor.meta_dirty {
            self.editor.meta_dirty = false;
            let _ = self.save_meta();
        }
        v
    }

    fn save_meta(&self) -> anyhow::Result<()> {
        let a = app();
        let id = self.project.clone();
        let meta = a.db(|st| st.get_project(&a.user, &id))?;
        let meta = self.editor.project_meta(&meta);
        a.db(|st| st.save_meta(&a.user, &id, &meta))?;
        Ok(())
    }
}

type Opened = (Vec<u8>, babel_pages::Parts, babel_pages::Parts, i64, Value);

fn stored(id: &str, rev: Option<i64>) -> anyhow::Result<Opened> {
    let a = app();
    let pid = id.to_owned();
    a.db(|s| {
        let docx = s.source_docx(&a.user, &pid)?;
        let source = s.source_parts(&a.user, &pid)?;
        let (rev, translated) = match rev {
            Some(r) => (r, s.parts_at(&a.user, &pid, r)?.unwrap_or_default()),
            None => s.parts(&a.user, &pid)?,
        };
        let meta = s.get_project(&a.user, &pid)?;
        Ok((docx, source, translated, rev, meta))
    })
    .map_err(Into::into)
}

/// Opens the translation for editing and draws every page of both documents.
pub fn open(id: &str) -> anyhow::Result<Value> {
    let (docx, source, translated, rev, meta) = stored(id, None)?;
    let mut editor = Editor::open(docx, source, translated, &meta)?;
    let settings = proof_settings();
    editor.set_proofing(
        settings["picky"].as_bool().unwrap_or(false),
        settings["dictionary"].as_array().map(|d| d.iter().filter_map(|w| w.as_str().map(str::to_owned)).collect::<Vec<_>>()).unwrap_or_default(),
    );
    let dir = runtime_dir().join(id);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let mut s = Native { project: id.to_owned(), editor, rev, versions: vec![], src_versions: vec![], dir, unsaved_since: None };
    let source = s.source_json()?;
    let (_, pages) = s.redraw();
    // Ids given on opening are saved with the next save, not as an edit now.
    s.unsaved_since = None;
    let proofing = s.editor.proof.lang.is_some();
    let mut v = s.editor.state();
    v["pages"] = json!(pages);
    v["source"] = json!(source);
    v["marks"] = s.editor.marks();
    v["proofing"] = json!(proofing);
    *NATIVE.lock().unwrap() = Some(s);
    if proofing {
        start_proofing(id.to_owned());
    }
    Ok(v)
}

/// The stored state changed elsewhere: carry on from it, unless something
/// here is unsaved. Null if nothing needs doing.
pub fn reload(id: &str) -> Value {
    let mut g = NATIVE.lock().unwrap();
    let Some(s) = g.as_mut().filter(|s| s.project == id && s.unsaved_since.is_none()) else { return Value::Null };
    let Ok((_, _, translated, rev, meta)) = stored(id, None) else { return Value::Null };
    if rev == s.rev {
        return Value::Null;
    }
    if s.editor.replace_translation(translated).is_err() {
        return Value::Null;
    }
    s.editor.confirmed = meta["confirmed"].as_array().map(|v| v.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default();
    s.rev = rev;
    let v = s.editor.state();
    let mut v = s.answer(v);
    s.unsaved_since = None;
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

fn with<T>(f: impl FnOnce(&mut Native) -> T, none: T) -> T {
    match NATIVE.lock().unwrap().as_mut() {
        Some(s) => f(s),
        None => none,
    }
}

pub fn click(page: usize, x: f32, y: f32, extend: bool) -> Value {
    with(|s| { let v = s.editor.click(page, x, y, extend); s.answer(v) }, Value::Null)
}

pub fn select_word(page: usize, x: f32, y: f32) -> Value {
    with(|s| { let v = s.editor.select_word(page, x, y); s.answer(v) }, Value::Null)
}

pub fn source_click(page: usize, x: f32, y: f32, extend: bool) -> Value {
    with(|s| { let v = s.editor.source_click(page, x, y, extend); s.answer(v) }, Value::Null)
}

pub fn source_word(page: usize, x: f32, y: f32) -> Value {
    with(|s| { let v = s.editor.source_word(page, x, y); s.answer(v) }, Value::Null)
}

pub fn source_menu(page: usize, x: f32, y: f32) -> Value {
    with(|s| s.editor.source_menu(page, x, y), Value::Null)
}

pub fn copy_source_link() -> Option<String> {
    with(|s| s.editor.copy_source_link(), None)
}

pub fn source_selected_text() -> String {
    with(|s| s.editor.source_selected_text(), String::new())
}

pub fn key(k: Key, extend: bool) -> Value {
    with(|s| { let v = s.editor.key(k, extend); s.answer(v) }, Value::Null)
}

pub fn selected_text() -> String {
    with(|s| s.editor.selected_text(), String::new())
}

pub fn original_text() -> Option<String> {
    with(|s| s.editor.original_text(), None)
}

pub fn revert() -> Value {
    with(|s| { let v = s.editor.revert_paragraph(); s.answer(v) }, Value::Null)
}

pub fn paste_link(clipboard: &str) -> Value {
    with(|s| { let v = s.editor.paste_link(clipboard); s.answer(v) }, Value::Null)
}

pub fn apply_suggestion(pid: &str, n: usize, value: &str) -> Value {
    with(|s| { let v = s.editor.apply_suggestion(pid, n, value); s.answer(v) }, Value::Null)
}

pub fn ignore(pid: &str, n: usize) {
    with(|s| { s.editor.ignore(pid, n); s.editor.meta_dirty = false; let _ = s.save_meta(); }, ());
}

/// Adds an issue's word to the personal dictionary (kept on this computer).
pub fn add_to_dictionary(pid: &str, n: usize) {
    with(|s| {
        if let Some(word) = s.editor.add_to_dictionary(pid, n) {
            let mut v = proof_settings();
            let mut words: Vec<String> = v["dictionary"].as_array().map(|d| d.iter().filter_map(|w| w.as_str().map(str::to_owned)).collect()).unwrap_or_default();
            if !words.contains(&word) {
                words.push(word);
                words.sort();
            }
            v["dictionary"] = json!(words);
            save_proof_settings(&v);
        }
    }, ());
}

/// Proofing settings: {picky, dictionary}.
pub fn settings() -> Value {
    proof_settings()
}

pub fn set_settings(picky: bool, dictionary: Vec<String>) {
    let v = json!({ "picky": picky, "dictionary": dictionary });
    save_proof_settings(&v);
    with(|s| {
        s.editor.set_proofing(picky, dictionary);
        // Everything edited is checked again with the new settings.
        let ids: Vec<String> = s.editor.proof.source.keys().cloned().collect();
        s.editor.proof.issues.clear();
        s.editor.proof.queue.extend(ids);
    }, ());
}

pub fn issues() -> Value {
    with(|s| s.editor.issues(), json!({ "underlines": [] }))
}

/// Checks queued paragraphs in the background while the session is open.
fn start_proofing(project: String) {
    let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().expect("HTTP client");
    app().rt.spawn(async move {
        let mut backoff = Duration::ZERO;
        loop {
            tokio::time::sleep(Duration::from_millis(400)).await;
            let (batch, form) = {
                let mut g = NATIVE.lock().unwrap();
                let Some(s) = g.as_mut().filter(|s| s.project == project) else { return };
                match s.editor.proof_batch() {
                    Some(b) => {
                        let form = s.editor.proof.request(&b);
                        (b, form)
                    }
                    None => continue,
                }
            };
            let res = http.post(format!("{}/v2/check", proof::ENDPOINT)).form(&form).header("Accept", "application/json").send().await;
            let outcome: Result<Vec<Value>, (Duration, String)> = match res {
                Ok(r) if r.status().is_success() => match r.json::<Value>().await {
                    Ok(v) => Ok(v["matches"].as_array().cloned().unwrap_or_default()),
                    Err(e) => Err((Duration::from_secs(10), e.to_string())),
                },
                Ok(r) if matches!(r.status().as_u16(), 429 | 503) => Err((Duration::from_secs(4), "LanguageTool rate limit — retrying shortly".into())),
                Ok(r) => Err((Duration::from_secs(10), format!("LanguageTool answered {}", r.status().as_u16()))),
                Err(e) => Err((Duration::from_secs(10), format!("LanguageTool is unreachable: {e}"))),
            };
            {
                let mut g = NATIVE.lock().unwrap();
                let Some(s) = g.as_mut().filter(|s| s.project == project) else { return };
                match outcome {
                    Ok(m) => {
                        backoff = Duration::ZERO;
                        s.editor.proof_result(&batch, &m);
                    }
                    Err((wait, status)) => {
                        backoff = (backoff * 2).max(wait).min(Duration::from_secs(120));
                        s.editor.proof_failed(&batch, &status);
                    }
                }
            }
            crate::bridge::notify_issues();
            tokio::time::sleep(Duration::from_millis(proof::MIN_INTERVAL_MS) + backoff).await;
        }
    });
}

/// Saves the changed parts (and the project's metadata) if there are any.
pub fn save() -> anyhow::Result<bool> {
    let mut g = NATIVE.lock().unwrap();
    let Some(s) = g.as_mut() else { return Ok(false) };
    let dirty = s.editor.take_dirty();
    if dirty.is_empty() {
        s.unsaved_since = None;
        return Ok(false);
    }
    let a = app();
    let (id, base) = (s.project.clone(), s.rev);
    let meta = {
        let pid = id.clone();
        let m = a.db(|st| st.get_project(&a.user, &pid))?;
        let mut m = s.editor.project_meta(&m);
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
        let (_, _, translated, _, _) = stored(&id, Some(saved.rev))?;
        s.editor.replace_translation(translated)?;
        s.redraw();
        s.unsaved_since = None;
    }
    a.state.notify(babel_server::Change { user: a.user.id, project: id, rev: saved.rev, origin: Some("native".into()), deleted: false });
    Ok(true)
}
