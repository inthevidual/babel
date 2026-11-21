//! Babel for macOS: babel-app behind a C interface (include/babel.h) for the
//! Swift app in macos/. One entry point takes a method and its arguments as
//! JSON and answers JSON, as the Qt bridge does with QML; pages are drawn on
//! demand (`babel_render`), so only those on screen are kept in memory.
//! Clipboard, files and dialogs are the Swift side's.

use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use babel_app::{account, projects, session, Event};
use babel_editor::Key;
use babel_pages::edit::Style;
use serde_json::{json, Value};

/// ~/Library/Application Support/Babel.
fn default_data_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library/Application Support/Babel")
}

fn c_string(s: String) -> *mut c_char {
    CString::new(s.replace('\0', "")).expect("no NUL").into_raw()
}

unsafe fn str_of<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(p) }.to_str().ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn babel_start(data_dir: *const c_char, error: *mut *mut c_char) -> bool {
    let dir = unsafe { str_of(data_dir) }.map(PathBuf::from).unwrap_or_else(default_data_dir);
    match catch_unwind(|| babel_app::App::start(&dir).map(|_| ())) {
        Ok(Ok(())) => true,
        outcome => {
            let msg = match outcome {
                Ok(Err(e)) => format!("Could not open Babel's data in {}: {e}", dir.display()),
                _ => "Babel could not start.".to_owned(),
            };
            if !error.is_null() {
                unsafe { *error = c_string(msg) };
            }
            false
        }
    }
}

pub type Listener = extern "C" fn(*const c_char);

fn event_json(e: Event) -> Value {
    match e {
        Event::Issues => json!({ "event": "issues" }),
        Event::ExternalChange(id) => json!({ "event": "external", "id": id }),
        Event::Projects => json!({ "event": "projects" }),
        Event::Sync(st) => json!({ "event": "sync", "state": st.to_json() }),
        Event::Message { text, error } => json!({ "event": "message", "text": text, "error": error }),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn babel_connect(listener: Listener) {
    babel_app::connect(move |e| {
        let s = CString::new(event_json(e).to_string()).expect("no NUL");
        listener(s.as_ptr());
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn babel_call(method: *const c_char, args: *const c_char) -> *mut c_char {
    let method = unsafe { str_of(method) }.unwrap_or("");
    let args: Value = unsafe { str_of(args) }.and_then(|a| serde_json::from_str(a).ok()).unwrap_or(Value::Null);
    let answer = catch_unwind(AssertUnwindSafe(|| call(method, &args))).unwrap_or_else(|_| json!({ "error": format!("Babel failed doing “{method}”.") }));
    c_string(answer.to_string())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn babel_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

#[repr(C)]
pub struct BabelImage {
    pub width: u32,
    pub height: u32,
    pub data: *mut u8,
    pub len: usize,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn babel_render(side: u8, page: u32, scale: f32, out: *mut BabelImage) -> bool {
    if out.is_null() {
        return false;
    }
    let side = if side == 0 { session::Side::Target } else { session::Side::Source };
    let Ok(Some((w, h, px))) = catch_unwind(|| session::render(side, page as usize, scale)) else { return false };
    let mut px = px.into_boxed_slice();
    let img = BabelImage { width: w, height: h, data: px.as_mut_ptr(), len: px.len() };
    std::mem::forget(px);
    unsafe { *out = img };
    true
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn babel_image_free(image: *mut BabelImage) {
    let Some(img) = (unsafe { image.as_mut() }) else { return };
    if !img.data.is_null() {
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(img.data, img.len)) });
        img.data = std::ptr::null_mut();
        img.len = 0;
    }
}

// ── Calls ─────────────────────────────────────────────────────────────────

fn s<'a>(a: &'a Value, k: &str) -> &'a str {
    a[k].as_str().unwrap_or("")
}

fn n(a: &Value, k: &str) -> usize {
    a[k].as_f64().unwrap_or(0.0).max(0.0) as usize
}

fn f(a: &Value, k: &str) -> f32 {
    a[k].as_f64().unwrap_or(0.0) as f32
}

fn b(a: &Value, k: &str) -> bool {
    a[k].as_bool().unwrap_or(false)
}

fn error(e: impl std::fmt::Display) -> Value {
    json!({ "error": e.to_string() })
}

/// A key by name, as the Swift side names what AppKit's key bindings ask for.
fn key_of(name: &str, text: &str) -> Option<Key> {
    Some(match name {
        "left" => Key::Left,
        "right" => Key::Right,
        "up" => Key::Up,
        "down" => Key::Down,
        "home" => Key::Home,
        "end" => Key::End,
        "backspace" => Key::Backspace,
        "delete" => Key::Delete,
        "enter" => Key::Enter,
        "lineBreak" => Key::LineBreak,
        "selectAll" => Key::SelectAll,
        "undo" => Key::Undo,
        "redo" => Key::Redo,
        "done" => Key::Done,
        "bold" => Key::Toggle(Style::Bold),
        "italic" => Key::Toggle(Style::Italic),
        "underline" => Key::Toggle(Style::Underline),
        "text" if !text.is_empty() => Key::Text(text.to_owned()),
        _ => return None,
    })
}

fn path(a: &Value, k: &str) -> Option<PathBuf> {
    let p = s(a, k);
    (!p.is_empty()).then(|| PathBuf::from(p))
}

fn import(a: &Value) -> Value {
    let Some(original) = path(a, "path") else { return error("No file was given.") };
    let translated = path(a, "translated");
    match projects::import(&original, s(a, "lang"), translated.as_deref()) {
        Ok(id) => json!({ "id": id }),
        Err(e) => error(e),
    }
}

fn export(a: &Value) -> Value {
    let (id, sid) = (s(a, "id"), s(a, "sid"));
    let sid = (!sid.is_empty()).then_some(sid);
    let Some(to) = path(a, "path") else { return error("No file was given.") };
    // What is being typed is part of the file; an export is kept as a version.
    if session::current().as_deref() == Some(id) && sid.is_none() {
        let _ = session::add_version("download");
    }
    match projects::export(id, sid, &to) {
        Ok(()) => json!({ "message": format!("Saved {}", display(&to)) }),
        Err(e) => error(format!("Could not export: {e}")),
    }
}

fn display(p: &Path) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let full = p.display().to_string();
    match full.strip_prefix(&home) {
        Some(rest) if !home.is_empty() => format!("~{rest}"),
        _ => full,
    }
}

fn call(method: &str, a: &Value) -> Value {
    let (page, x, y) = (n(a, "page"), f(a, "x"), f(a, "y"));
    match method {
        // ── Translations ──────────────────────────────────────────────
        "projects" => projects::list(),
        "languages" => projects::languages(),
        "import" => import(a),
        "export" => export(a),
        "exportName" => {
            let t = a["versionTime"].as_f64().filter(|t| *t > 0.0).map(|t| t as i64);
            json!(projects::export_name(s(a, "id"), t))
        }
        "segmentsName" => json!(projects::segments_name(s(a, "id"))),
        "delete" => match projects::delete(s(a, "id")) {
            Ok(()) => json!({}),
            Err(e) => error(format!("Could not delete: {e}")),
        },

        // ── Versions and settings ─────────────────────────────────────
        "versions" => projects::versions(s(a, "id")),
        "addVersion" => {
            let name = s(a, "name").trim();
            match session::add_version(if name.is_empty() { "saved by you" } else { name }) {
                Ok(()) => json!({}),
                Err(e) => error(format!("Could not keep a version: {e}")),
            }
        }
        "restore" => {
            let _ = session::save();
            match projects::restore(s(a, "id"), s(a, "sid"), session::stats()) {
                Ok(()) => json!({ "message": "Version restored." }),
                Err(e) => error(format!("Could not restore: {e}")),
            }
        }
        "setLanguage" => match session::set_language(s(a, "code")) {
            Ok(()) => json!({}),
            Err(e) => error(format!("Could not change the language: {e}")),
        },
        "settings" => session::settings(),
        "setSettings" => {
            session::set_settings(a.clone());
            json!({})
        }

        // ── The editor ────────────────────────────────────────────────
        "open" => match session::open(s(a, "id"), Box::new(session::OnDemand)) {
            Ok(v) => v,
            Err(e) => error(format!("Could not open the translation: {e}")),
        },
        "close" => {
            session::close();
            json!({})
        }
        "current" => json!(session::current()),
        "save" => match session::save() {
            Ok(_) => json!({}),
            Err(e) => error(format!("Could not save: {e}")),
        },
        "reload" => session::reload(s(a, "id")),
        "click" => session::click(page, x, y, b(a, "extend")),
        "word" => session::select_word(page, x, y),
        "paragraph" => session::select_paragraph(page, x, y),
        "sourceClick" => session::source_click(page, x, y, b(a, "extend")),
        "sourceWord" => session::source_word(page, x, y),
        "sourceParagraph" => session::source_select_paragraph(page, x, y),
        "sourceMenu" => session::source_menu(page, x, y),
        "key" => match key_of(s(a, "key"), s(a, "text")) {
            Some(k) => session::key(k, b(a, "extend")),
            None => Value::Null,
        },
        "selectedText" => json!(session::selected_text()),
        "sourceSelectedText" => json!(session::source_selected_text()),
        "originalText" => json!(session::original_text()),
        "copySourceLink" => json!(session::copy_source_link()),
        "revert" => session::revert(),
        "pasteLink" => session::paste_link(s(a, "clipboard")),
        "issues" => session::issues(),
        "apply" => session::apply_suggestion(s(a, "pid"), n(a, "n"), s(a, "value")),
        "ignore" => {
            session::ignore(s(a, "pid"), n(a, "n"));
            json!({})
        }
        "addWord" => {
            session::add_to_dictionary(s(a, "pid"), n(a, "n"));
            json!({})
        }

        // ── The text view ─────────────────────────────────────────────
        "rows" => session::rows(),
        "rowChanges" => session::row_changes(),
        "textSelect" => session::text_select(s(a, "pid"), n(a, "anchor"), n(a, "caret")),
        "textEdit" => session::text_edit(s(a, "pid"), n(a, "from"), n(a, "to"), s(a, "text")),
        "claudeSpans" => session::claude_spans(s(a, "pid")),
        "showPages" => session::show_pages(b(a, "on")),

        // ── Claude's translation ──────────────────────────────────────
        "insertClaude" => session::insert_claude(),
        "rowsAction" => session::rows_action(s(a, "action"), &a["pids"].to_string()),
        "attachClaude" => match path(a, "path") {
            Some(p) => session::attach_claude(&p),
            None => error("No file was given."),
        },
        "removeClaude" => session::remove_claude(),
        "exportSegments" => match path(a, "path").map(|p| session::export_segments(&p)) {
            Some(Ok(())) => json!({}),
            Some(Err(e)) => error(e),
            None => error("No file was given."),
        },

        // ── Account ───────────────────────────────────────────────────
        "account" => account::state().to_json(),
        "signIn" => {
            account::sign_in(s(a, "server").to_owned(), s(a, "user").to_owned(), s(a, "password").to_owned(), s(a, "device").to_owned());
            json!({})
        }
        "signOut" => {
            account::sign_out();
            json!({})
        }
        "syncNow" => {
            account::sync_now();
            json!({})
        }

        _ => error(format!("Babel has no “{method}”.")),
    }
}
