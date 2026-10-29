//! The QML side's view of the app: the translations, opening and exporting
//! them, versions, settings, the native editor (native.rs) and the account
//! (sync.rs). Answers are JSON strings.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qurl.h");
        type QUrl = cxx_qt_lib::QUrl;
    }

    unsafe extern "C++" {
        include!("babel-desktop/init.h");
        /// The KDE controls style, unless the user picked one.
        fn babel_init_style();

        include!("cxx-qt-lib/qqmlapplicationengine.h");
        type QQmlApplicationEngine = cxx_qt_lib::QQmlApplicationEngine;
        fn babel_exit_on_load_failure(engine: Pin<&mut QQmlApplicationEngine>);
        fn babel_clipboard_set(text: &QString);
        fn babel_clipboard_text() -> QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(QString, sync_status, cxx_name = "syncStatus")]
        #[qproperty(QString, sync_server, cxx_name = "syncServer")]
        #[qproperty(QString, sync_user, cxx_name = "syncUser")]
        #[qproperty(bool, signed_in, cxx_name = "signedIn")]
        #[qproperty(bool, syncing)]
        type Backend = super::BackendRust;

        // ── Translations ────────────────────────────────────────────────

        /// [{id, name, fileName, targetLang, langName, updatedAt, stats}].
        #[qinvokable]
        fn projects(self: &Backend) -> QString;

        /// The target languages: [{code, name, en, lt}].
        #[qinvokable]
        fn languages(self: &Backend) -> QString;

        /// Opens a .docx as a new translation into `lang`; its id, or "" (and a message).
        #[qinvokable]
        #[cxx_name = "importFile"]
        fn import_file(self: Pin<&mut Backend>, url: &QUrl, lang: &QString) -> QString;

        /// Continue from a translated file: the original and an earlier translation of it.
        #[qinvokable]
        #[cxx_name = "pairFiles"]
        fn pair_files(self: Pin<&mut Backend>, original: &QUrl, translated: &QUrl, lang: &QString) -> QString;

        /// Exports the translated .docx of a project (of a version, if `sid` isn't empty).
        #[qinvokable]
        #[cxx_name = "exportProject"]
        fn export_project(self: Pin<&mut Backend>, id: &QString, sid: &QString, url: &QUrl) -> bool;

        /// The file name an export gets ("name_SV.docx", with the time for a version).
        #[qinvokable]
        #[cxx_name = "exportName"]
        fn export_name(self: &Backend, id: &QString, version_time: f64) -> QString;

        #[qinvokable]
        #[cxx_name = "deleteProject"]
        fn delete_project(self: Pin<&mut Backend>, id: &QString);

        // ── Versions and settings ───────────────────────────────────────

        /// [{sid, time, reason, stats}], newest first.
        #[qinvokable]
        fn versions(self: &Backend, id: &QString) -> QString;

        /// Keeps the translation being edited as a version named `name`.
        #[qinvokable]
        #[cxx_name = "addVersion"]
        fn add_version(self: Pin<&mut Backend>, name: &QString);

        /// Puts a version back (the current state is kept as a version first).
        #[qinvokable]
        #[cxx_name = "restoreVersion"]
        fn restore_version(self: Pin<&mut Backend>, id: &QString, sid: &QString);

        /// Changes the target language of the translation being edited.
        #[qinvokable]
        #[cxx_name = "setLanguage"]
        fn set_language(self: Pin<&mut Backend>, code: &QString);

        /// {proof, picky, dictionary, endpoint, user, key, marks}.
        #[qinvokable]
        fn settings(self: &Backend) -> QString;

        #[qinvokable]
        #[cxx_name = "setSettings"]
        fn set_settings(self: Pin<&mut Backend>, json: &QString);

        // ── The editor ──────────────────────────────────────────────────

        /// Opens a translation: {source: [{index, url, w, h}], pages, caret, …}.
        #[qinvokable]
        #[cxx_name = "nativeOpen"]
        fn native_open(self: Pin<&mut Backend>, id: &QString) -> QString;

        /// Saves and closes the editing session.
        #[qinvokable]
        #[cxx_name = "nativeClose"]
        fn native_close(self: Pin<&mut Backend>);

        /// Places the caret at a point (points) on a page; `extend` keeps the
        /// selection's other end (Shift-click, dragging).
        #[qinvokable]
        #[cxx_name = "nativeClick"]
        fn native_click(self: Pin<&mut Backend>, page: i32, x: f64, y: f64, extend: bool) -> QString;

        /// Double-click: selects the word.
        #[qinvokable]
        #[cxx_name = "nativeWord"]
        fn native_word(self: Pin<&mut Backend>, page: i32, x: f64, y: f64) -> QString;

        /// A key (Qt key code, its text, Qt modifier flags): {caret, selection, pages, all, error, …}.
        #[qinvokable]
        #[cxx_name = "nativeKey"]
        fn native_key(self: Pin<&mut Backend>, key: i32, text: &QString, modifiers: i32) -> QString;

        /// Text from an input method, committed.
        #[qinvokable]
        #[cxx_name = "nativeText"]
        fn native_text(self: Pin<&mut Backend>, text: &QString) -> QString;

        /// Copies the selection to the clipboard; with `cut` deletes it too.
        #[qinvokable]
        #[cxx_name = "nativeCopy"]
        fn native_copy(self: Pin<&mut Backend>, cut: bool) -> QString;

        /// Pastes the clipboard's text.
        #[qinvokable]
        #[cxx_name = "nativePaste"]
        fn native_paste(self: Pin<&mut Backend>) -> QString;

        /// Proofing underlines: {underlines: [{page, x, y, w, kind}], count, status}.
        #[qinvokable]
        #[cxx_name = "nativeIssues"]
        fn native_issues(self: Pin<&mut Backend>) -> QString;

        /// Puts suggestion `value` in place of issue `n` of paragraph `pid`.
        #[qinvokable]
        #[cxx_name = "nativeApply"]
        fn native_apply(self: Pin<&mut Backend>, pid: &QString, n: i32, value: &QString) -> QString;

        #[qinvokable]
        #[cxx_name = "nativeIgnore"]
        fn native_ignore(self: Pin<&mut Backend>, pid: &QString, n: i32);

        #[qinvokable]
        #[cxx_name = "nativeAddWord"]
        fn native_add_word(self: Pin<&mut Backend>, pid: &QString, n: i32);

        /// Opens the stored state again after edits from elsewhere; "null" if unchanged.
        #[qinvokable]
        #[cxx_name = "nativeReload"]
        fn native_reload(self: Pin<&mut Backend>, id: &QString) -> QString;

        /// A click (or drag, with `extend`) on the original.
        #[qinvokable]
        #[cxx_name = "nativeSourceClick"]
        fn native_source_click(self: Pin<&mut Backend>, page: i32, x: f64, y: f64, extend: bool) -> QString;

        #[qinvokable]
        #[cxx_name = "nativeSourceWord"]
        fn native_source_word(self: Pin<&mut Backend>, page: i32, x: f64, y: f64) -> QString;

        /// What a right-click on the original offers: {text, lookup, link, paragraph}.
        #[qinvokable]
        #[cxx_name = "nativeSourceMenu"]
        fn native_source_menu(self: Pin<&mut Backend>, page: i32, x: f64, y: f64) -> QString;

        /// Copies the link under the last right-click on the original.
        #[qinvokable]
        #[cxx_name = "nativeCopyLink"]
        fn native_copy_link(self: Pin<&mut Backend>) -> QString;

        /// Copies text to the clipboard.
        #[qinvokable]
        #[cxx_name = "copyText"]
        fn copy_text(self: Pin<&mut Backend>, text: &QString);

        /// Copies the original's selection.
        #[qinvokable]
        #[cxx_name = "nativeCopySource"]
        fn native_copy_source(self: Pin<&mut Backend>);

        /// Copies the original text of the paragraph at the caret.
        #[qinvokable]
        #[cxx_name = "nativeCopyOriginal"]
        fn native_copy_original(self: Pin<&mut Backend>);

        /// Reverts the paragraph at the caret to the original.
        #[qinvokable]
        #[cxx_name = "nativeRevert"]
        fn native_revert(self: Pin<&mut Backend>) -> QString;

        /// Ctrl+K: links the selection to the copied link or the clipboard's address.
        #[qinvokable]
        #[cxx_name = "nativePasteLink"]
        fn native_paste_link(self: Pin<&mut Backend>) -> QString;

        /// Saves edits if there are any.
        #[qinvokable]
        #[cxx_name = "nativeSave"]
        fn native_save(self: Pin<&mut Backend>);

        /// New proofing results are in.
        #[qsignal]
        #[cxx_name = "nativeIssuesChanged"]
        fn native_issues_changed(self: Pin<&mut Backend>);

        /// The translation being edited was saved from elsewhere (sync, a restore).
        #[qsignal]
        #[cxx_name = "nativeExternalChange"]
        fn native_external_change(self: Pin<&mut Backend>, id: QString);

        // ── Account ─────────────────────────────────────────────────────

        #[qinvokable]
        #[cxx_name = "signIn"]
        fn sign_in(self: Pin<&mut Backend>, server: &QString, user: &QString, password: &QString);

        #[qinvokable]
        #[cxx_name = "signOut"]
        fn sign_out(self: Pin<&mut Backend>);

        #[qinvokable]
        #[cxx_name = "syncNow"]
        fn sync_now(self: Pin<&mut Backend>);

        #[qsignal]
        #[cxx_name = "projectsChanged"]
        fn projects_changed(self: Pin<&mut Backend>);

        /// Something the user should be told about.
        #[qsignal]
        fn message(self: Pin<&mut Backend>, text: QString, error: bool);
    }

    impl cxx_qt::Threading for Backend {}
    impl cxx_qt::Initialize for Backend {}
}

use std::path::PathBuf;
use std::pin::Pin;

use cxx_qt::Threading;
use cxx_qt_lib::{QString, QUrl};
use serde_json::{json, Value};

use crate::app::app;
use crate::{native, projects, sync};

#[derive(Default)]
pub struct BackendRust {
    sync_status: QString,
    sync_server: QString,
    sync_user: QString,
    signed_in: bool,
    syncing: bool,
}

static QT: std::sync::OnceLock<cxx_qt::CxxQtThread<qobject::Backend>> = std::sync::OnceLock::new();

/// Tells QML that new proofing results are in (from any thread).
pub fn notify_issues() {
    if let Some(qt) = QT.get() {
        let _ = qt.queue(|o| o.native_issues_changed());
    }
}

impl cxx_qt::Initialize for qobject::Backend {
    fn initialize(self: Pin<&mut Self>) {
        let _ = QT.set(self.qt_thread());
        let a = app();
        // Any save, from the editor or from syncing, refreshes the list.
        let qt = self.qt_thread();
        let mut changes = a.state.changes.subscribe();
        let uid = a.user.id;
        a.rt.spawn(async move {
            loop {
                match changes.recv().await {
                    Ok(c) if c.user != uid => continue,
                    // The translation being edited, saved from elsewhere.
                    Ok(c) if c.origin.as_deref() != Some("native") && native::current().as_deref() == Some(c.project.as_str()) => {
                        let id = QString::from(&c.project);
                        let _ = qt.queue(move |o| o.native_external_change(id));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    _ => {}
                }
                // Typing saves every second or so; one refresh per burst is plenty.
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                while changes.try_recv().is_ok() {}
                if qt.queue(|o| o.projects_changed()).is_err() {
                    break;
                }
            }
        });
        sync::attach(self);
    }
}

fn path_of(url: &QUrl) -> Option<PathBuf> {
    url.to_local_file().map(|s| PathBuf::from(s.to_string()))
}

fn s(v: &Value) -> QString {
    QString::from(&v.to_string())
}

impl qobject::Backend {
    // ── Translations ──────────────────────────────────────────────────

    fn projects(&self) -> QString {
        let a = app();
        let list = a.db(|st| st.list_projects(&a.user)).unwrap_or_default();
        let slim: Vec<Value> = list
            .iter()
            .map(|p| {
                json!({
                    "id": p["id"], "name": p["name"], "fileName": p["fileName"],
                    "targetLang": p["targetLang"], "updatedAt": p["updatedAt"], "stats": p["stats"],
                    "langName": p["targetLang"].as_str().and_then(projects::language_name),
                })
            })
            .collect();
        s(&Value::Array(slim))
    }

    fn languages(&self) -> QString {
        s(&projects::languages())
    }

    fn import_file(self: Pin<&mut Self>, url: &QUrl, lang: &QString) -> QString {
        let Some(path) = path_of(url) else { return QString::default() };
        match projects::import(&path, &lang.to_string(), None) {
            Ok(id) => QString::from(&id),
            Err(e) => {
                self.message(QString::from(&e.to_string()), true);
                QString::default()
            }
        }
    }

    fn pair_files(self: Pin<&mut Self>, original: &QUrl, translated: &QUrl, lang: &QString) -> QString {
        let (Some(o), Some(t)) = (path_of(original), path_of(translated)) else { return QString::default() };
        match projects::import(&o, &lang.to_string(), Some(&t)) {
            Ok(id) => QString::from(&id),
            Err(e) => {
                self.message(QString::from(&e.to_string()), true);
                QString::default()
            }
        }
    }

    fn export_project(self: Pin<&mut Self>, id: &QString, sid: &QString, url: &QUrl) -> bool {
        let Some(path) = path_of(url) else { return false };
        let (id, sid) = (id.to_string(), sid.to_string());
        let sid = (!sid.is_empty()).then_some(sid);
        // What is being typed is part of the file; an export is kept as a version.
        if native::current().as_deref() == Some(id.as_str()) && sid.is_none() {
            let _ = native::add_version("download");
        }
        match projects::export(&id, sid.as_deref(), &path) {
            Ok(()) => {
                self.message(QString::from(&format!("Saved {}", path.display())), false);
                true
            }
            Err(e) => {
                self.message(QString::from(&format!("Could not export: {e}")), true);
                false
            }
        }
    }

    fn export_name(&self, id: &QString, version_time: f64) -> QString {
        QString::from(&projects::export_name(&id.to_string(), (version_time > 0.0).then_some(version_time as i64)))
    }

    fn delete_project(self: Pin<&mut Self>, id: &QString) {
        let a = app();
        let id = id.to_string();
        let pid = id.clone();
        match a.db(|st| st.delete_project(&a.user, &pid)) {
            Ok(()) => a.state.notify(babel_server::Change { user: a.user.id, project: id, rev: 0, origin: None, deleted: true }),
            Err(e) => self.message(QString::from(&format!("Could not delete: {e}")), true),
        }
    }

    // ── Versions and settings ─────────────────────────────────────────

    fn versions(&self, id: &QString) -> QString {
        s(&projects::versions(&id.to_string()))
    }

    fn add_version(self: Pin<&mut Self>, name: &QString) {
        let n = name.to_string();
        let reason = if n.trim().is_empty() { "saved by you".to_owned() } else { n.trim().to_owned() };
        if let Err(e) = native::add_version(&reason) {
            self.message(QString::from(&format!("Could not keep a version: {e}")), true);
        }
    }

    fn restore_version(self: Pin<&mut Self>, id: &QString, sid: &QString) {
        let _ = native::save();
        let stats = native::stats();
        match projects::restore(&id.to_string(), &sid.to_string(), stats) {
            Ok(()) => self.message(QString::from("Version restored."), false),
            Err(e) => self.message(QString::from(&format!("Could not restore: {e}")), true),
        }
    }

    fn set_language(self: Pin<&mut Self>, code: &QString) {
        if let Err(e) = native::set_language(&code.to_string()) {
            self.message(QString::from(&format!("Could not change the language: {e}")), true);
        }
    }

    fn settings(&self) -> QString {
        s(&native::settings())
    }

    fn set_settings(self: Pin<&mut Self>, json: &QString) {
        if let Ok(v) = serde_json::from_str::<Value>(&json.to_string()) {
            native::set_settings(v);
        }
    }

    // ── The editor ────────────────────────────────────────────────────

    fn native_open(self: Pin<&mut Self>, id: &QString) -> QString {
        match native::open(&id.to_string()) {
            Ok(v) => s(&v),
            Err(e) => {
                self.message(QString::from(&format!("Could not open the translation: {e}")), true);
                QString::from("null")
            }
        }
    }

    fn native_close(self: Pin<&mut Self>) {
        native::close();
    }

    fn native_click(self: Pin<&mut Self>, page: i32, x: f64, y: f64, extend: bool) -> QString {
        s(&native::click(page.max(0) as usize, x as f32, y as f32, extend))
    }

    fn native_word(self: Pin<&mut Self>, page: i32, x: f64, y: f64) -> QString {
        s(&native::select_word(page.max(0) as usize, x as f32, y as f32))
    }

    fn native_key(self: Pin<&mut Self>, key: i32, text: &QString, modifiers: i32) -> QString {
        match babel_editor::Key::from_qt(key, &text.to_string(), modifiers) {
            Some(k) => s(&native::key(k, modifiers & 0x0200_0000 != 0)),
            None => QString::from("null"),
        }
    }

    fn native_text(self: Pin<&mut Self>, text: &QString) -> QString {
        let t = text.to_string();
        if t.is_empty() {
            return QString::from("null");
        }
        s(&native::key(babel_editor::Key::Text(t), false))
    }

    fn native_copy(self: Pin<&mut Self>, cut: bool) -> QString {
        let text = native::selected_text();
        if text.is_empty() {
            return QString::from("null");
        }
        qobject::babel_clipboard_set(&QString::from(&text));
        if !cut {
            return QString::from("null");
        }
        s(&native::key(babel_editor::Key::Delete, false))
    }

    fn native_paste(self: Pin<&mut Self>) -> QString {
        let t = qobject::babel_clipboard_text().to_string();
        if t.is_empty() {
            return QString::from("null");
        }
        s(&native::key(babel_editor::Key::Text(t), false))
    }

    fn native_issues(self: Pin<&mut Self>) -> QString {
        s(&native::issues())
    }

    fn native_apply(self: Pin<&mut Self>, pid: &QString, n: i32, value: &QString) -> QString {
        s(&native::apply_suggestion(&pid.to_string(), n.max(0) as usize, &value.to_string()))
    }

    fn native_ignore(self: Pin<&mut Self>, pid: &QString, n: i32) {
        native::ignore(&pid.to_string(), n.max(0) as usize);
    }

    fn native_add_word(self: Pin<&mut Self>, pid: &QString, n: i32) {
        native::add_to_dictionary(&pid.to_string(), n.max(0) as usize);
    }

    fn native_reload(self: Pin<&mut Self>, id: &QString) -> QString {
        s(&native::reload(&id.to_string()))
    }

    fn native_source_click(self: Pin<&mut Self>, page: i32, x: f64, y: f64, extend: bool) -> QString {
        s(&native::source_click(page.max(0) as usize, x as f32, y as f32, extend))
    }

    fn native_source_word(self: Pin<&mut Self>, page: i32, x: f64, y: f64) -> QString {
        s(&native::source_word(page.max(0) as usize, x as f32, y as f32))
    }

    fn native_source_menu(self: Pin<&mut Self>, page: i32, x: f64, y: f64) -> QString {
        s(&native::source_menu(page.max(0) as usize, x as f32, y as f32))
    }

    fn native_copy_link(mut self: Pin<&mut Self>) -> QString {
        match native::copy_source_link() {
            Some(clip) => {
                qobject::babel_clipboard_set(&QString::from(&clip));
                self.as_mut().message(QString::from(&format!("Link copied: {clip}")), false);
                QString::from(&clip)
            }
            None => QString::default(),
        }
    }

    fn copy_text(self: Pin<&mut Self>, text: &QString) {
        qobject::babel_clipboard_set(text);
    }

    fn native_copy_source(self: Pin<&mut Self>) {
        let t = native::source_selected_text();
        if !t.is_empty() {
            qobject::babel_clipboard_set(&QString::from(&t));
        }
    }

    fn native_copy_original(self: Pin<&mut Self>) {
        if let Some(t) = native::original_text() {
            qobject::babel_clipboard_set(&QString::from(&t));
        }
    }

    fn native_revert(self: Pin<&mut Self>) -> QString {
        s(&native::revert())
    }

    fn native_paste_link(self: Pin<&mut Self>) -> QString {
        let clip = qobject::babel_clipboard_text().to_string();
        s(&native::paste_link(&clip))
    }

    fn native_save(self: Pin<&mut Self>) {
        if let Err(e) = native::save() {
            self.message(QString::from(&format!("Could not save: {e}")), true);
        }
    }

    // ── Account ───────────────────────────────────────────────────────

    fn sign_in(self: Pin<&mut Self>, server: &QString, user: &QString, password: &QString) {
        sync::sign_in(self, server.to_string(), user.to_string(), password.to_string());
    }

    fn sign_out(self: Pin<&mut Self>) {
        sync::sign_out(self);
    }

    fn sync_now(self: Pin<&mut Self>) {
        sync::sync_now();
    }
}

pub use qobject::{babel_exit_on_load_failure, babel_init_style, Backend};
