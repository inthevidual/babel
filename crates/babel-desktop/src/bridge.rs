//! The QML side's view of the app: the list of translations, opening files,
//! and (in sync.rs) the account.

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
        /// Must run before the QGuiApplication exists.
        fn babel_init_webengine();
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
        #[qproperty(QString, editor_url, cxx_name = "editorUrl")]
        #[qproperty(QString, origin)]
        #[qproperty(QString, data_dir, cxx_name = "dataDir")]
        #[qproperty(QString, sync_status, cxx_name = "syncStatus")]
        #[qproperty(QString, sync_server, cxx_name = "syncServer")]
        #[qproperty(QString, sync_user, cxx_name = "syncUser")]
        #[qproperty(bool, signed_in, cxx_name = "signedIn")]
        #[qproperty(bool, syncing)]
        type Backend = super::BackendRust;

        /// The translations as JSON: [{id, name, fileName, targetLang, updatedAt, stats}].
        #[qinvokable]
        fn projects(self: &Backend) -> QString;

        /// Reads a .docx for the editor; returns the id to open it by, or "".
        #[qinvokable]
        #[cxx_name = "stageFile"]
        fn stage_file(self: Pin<&mut Backend>, url: &QUrl) -> QString;

        /// Opens the translation for editing on native pages: {pages: [{index, url, w, h}]}.
        #[qinvokable]
        #[cxx_name = "nativeOpen"]
        fn native_open(self: Pin<&mut Backend>, id: &QString) -> QString;

        /// Saves and closes the native editing session.
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

        #[qinvokable]
        #[cxx_name = "nativeAddWord"]
        fn native_add_word(self: Pin<&mut Backend>, pid: &QString, n: i32);

        /// Proofing settings: {picky, dictionary}.
        #[qinvokable]
        #[cxx_name = "proofSettings"]
        fn proof_settings(self: Pin<&mut Backend>) -> QString;

        #[qinvokable]
        #[cxx_name = "setProofSettings"]
        fn set_proof_settings(self: Pin<&mut Backend>, picky: bool, dictionary: &QString);

        /// New proofing results are in.
        #[qsignal]
        #[cxx_name = "nativeIssuesChanged"]
        fn native_issues_changed(self: Pin<&mut Backend>);

        /// The translation being edited natively was saved from elsewhere (sync, the web editor).
        #[qsignal]
        #[cxx_name = "nativeExternalChange"]
        fn native_external_change(self: Pin<&mut Backend>, id: QString);

        /// Saves native edits if there are any.
        #[qinvokable]
        #[cxx_name = "nativeSave"]
        fn native_save(self: Pin<&mut Backend>);

        #[qinvokable]
        #[cxx_name = "deleteProject"]
        fn delete_project(self: Pin<&mut Backend>, id: &QString);

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

use std::pin::Pin;

use cxx_qt::Threading;
use cxx_qt_lib::{QString, QUrl};
use serde_json::{json, Value};

use crate::app::app;
use crate::sync;

#[derive(Default)]
pub struct BackendRust {
    editor_url: QString,
    origin: QString,
    data_dir: QString,
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
    fn initialize(mut self: Pin<&mut Self>) {
        let _ = QT.set(self.qt_thread());
        let a = app();
        self.as_mut().set_editor_url(QString::from(&a.editor_url()));
        self.as_mut().set_origin(QString::from(&a.base));
        self.as_mut().set_data_dir(QString::from(&a.data_dir.display().to_string()));

        // Any save, from the editor or from syncing, refreshes the list.
        let qt = self.qt_thread();
        let mut changes = a.state.changes.subscribe();
        let uid = a.user.id;
        a.rt.spawn(async move {
            loop {
                match changes.recv().await {
                    Ok(c) if c.user != uid => continue,
                    // The translation being edited natively, saved from elsewhere.
                    Ok(c) if c.origin.as_deref() != Some("native") && crate::native::current().as_deref() == Some(c.project.as_str()) => {
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

/// A language's own name, from the editor's table (web/js/langs.js).
fn language_name(code: &str) -> Option<&'static str> {
    static NAMES: std::sync::OnceLock<Vec<(&'static str, &'static str)>> = std::sync::OnceLock::new();
    let names = NAMES.get_or_init(|| {
        include_str!("../../../web/js/langs.js")
            .lines()
            .filter_map(|l| {
                let field = |key: &str| {
                    let rest = &l[l.find(&format!("{key}: '"))? + key.len() + 3..];
                    Some(&rest[..rest.find('\'')?])
                };
                Some((field("code")?, field("name")?))
            })
            .collect()
    });
    names.iter().find(|(c, _)| *c == code).map(|(_, n)| *n)
}

impl qobject::Backend {
    fn projects(&self) -> QString {
        let a = app();
        let list = a.db(|s| s.list_projects(&a.user)).unwrap_or_default();
        let slim: Vec<Value> = list
            .iter()
            .map(|p| {
                json!({
                    "id": p["id"], "name": p["name"], "fileName": p["fileName"],
                    "targetLang": p["targetLang"], "updatedAt": p["updatedAt"], "stats": p["stats"],
                    "langName": p["targetLang"].as_str().and_then(language_name),
                })
            })
            .collect();
        QString::from(&Value::Array(slim).to_string())
    }

    fn stage_file(self: Pin<&mut Self>, url: &QUrl) -> QString {
        let path = url.to_local_file().map(|s| s.to_string()).unwrap_or_default();
        let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match std::fs::read(&path) {
            Ok(bytes) => QString::from(&app().stage(name, bytes)),
            Err(e) => {
                self.message(QString::from(&format!("Could not read {path}: {e}")), true);
                QString::default()
            }
        }
    }

    fn native_open(self: Pin<&mut Self>, id: &QString) -> QString {
        match crate::native::open(&id.to_string()) {
            Ok(v) => QString::from(&v.to_string()),
            Err(e) => {
                self.message(QString::from(&format!("Could not open the translation for editing: {e}")), true);
                QString::from("null")
            }
        }
    }

    fn native_close(self: Pin<&mut Self>) {
        crate::native::close();
    }

    fn native_click(self: Pin<&mut Self>, page: i32, x: f64, y: f64, extend: bool) -> QString {
        QString::from(&crate::native::click(page.max(0) as usize, x as f32, y as f32, extend).to_string())
    }

    fn native_word(self: Pin<&mut Self>, page: i32, x: f64, y: f64) -> QString {
        QString::from(&crate::native::select_word(page.max(0) as usize, x as f32, y as f32).to_string())
    }

    fn native_key(self: Pin<&mut Self>, key: i32, text: &QString, modifiers: i32) -> QString {
        match babel_editor::Key::from_qt(key, &text.to_string(), modifiers) {
            Some(k) => QString::from(&crate::native::key(k, modifiers & 0x0200_0000 != 0).to_string()),
            None => QString::from("null"),
        }
    }

    fn native_text(self: Pin<&mut Self>, text: &QString) -> QString {
        let t = text.to_string();
        if t.is_empty() {
            return QString::from("null");
        }
        QString::from(&crate::native::key(babel_editor::Key::Text(t), false).to_string())
    }

    fn native_copy(self: Pin<&mut Self>, cut: bool) -> QString {
        let text = crate::native::selected_text();
        if text.is_empty() {
            return QString::from("null");
        }
        qobject::babel_clipboard_set(&QString::from(&text));
        if !cut {
            return QString::from("null");
        }
        QString::from(&crate::native::key(babel_editor::Key::Delete, false).to_string())
    }

    fn native_paste(self: Pin<&mut Self>) -> QString {
        let t = qobject::babel_clipboard_text().to_string();
        if t.is_empty() {
            return QString::from("null");
        }
        QString::from(&crate::native::key(babel_editor::Key::Text(t), false).to_string())
    }

    fn native_issues(self: Pin<&mut Self>) -> QString {
        QString::from(&crate::native::issues().to_string())
    }

    fn native_apply(self: Pin<&mut Self>, pid: &QString, n: i32, value: &QString) -> QString {
        QString::from(&crate::native::apply_suggestion(&pid.to_string(), n.max(0) as usize, &value.to_string()).to_string())
    }

    fn native_ignore(self: Pin<&mut Self>, pid: &QString, n: i32) {
        crate::native::ignore(&pid.to_string(), n.max(0) as usize);
    }

    fn native_source_click(self: Pin<&mut Self>, page: i32, x: f64, y: f64, extend: bool) -> QString {
        QString::from(&crate::native::source_click(page.max(0) as usize, x as f32, y as f32, extend).to_string())
    }

    fn native_source_word(self: Pin<&mut Self>, page: i32, x: f64, y: f64) -> QString {
        QString::from(&crate::native::source_word(page.max(0) as usize, x as f32, y as f32).to_string())
    }

    fn native_source_menu(self: Pin<&mut Self>, page: i32, x: f64, y: f64) -> QString {
        QString::from(&crate::native::source_menu(page.max(0) as usize, x as f32, y as f32).to_string())
    }

    fn native_copy_link(mut self: Pin<&mut Self>) -> QString {
        match crate::native::copy_source_link() {
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
        let t = crate::native::source_selected_text();
        if !t.is_empty() {
            qobject::babel_clipboard_set(&QString::from(&t));
        }
    }

    fn native_copy_original(self: Pin<&mut Self>) {
        if let Some(t) = crate::native::original_text() {
            qobject::babel_clipboard_set(&QString::from(&t));
        }
    }

    fn native_revert(self: Pin<&mut Self>) -> QString {
        QString::from(&crate::native::revert().to_string())
    }

    fn native_paste_link(self: Pin<&mut Self>) -> QString {
        let clip = qobject::babel_clipboard_text().to_string();
        QString::from(&crate::native::paste_link(&clip).to_string())
    }

    fn native_add_word(self: Pin<&mut Self>, pid: &QString, n: i32) {
        crate::native::add_to_dictionary(&pid.to_string(), n.max(0) as usize);
    }

    fn proof_settings(self: Pin<&mut Self>) -> QString {
        QString::from(&crate::native::settings().to_string())
    }

    fn set_proof_settings(self: Pin<&mut Self>, picky: bool, dictionary: &QString) {
        let words: Vec<String> = dictionary.to_string().lines().map(|l| l.trim().to_owned()).filter(|l| !l.is_empty()).collect();
        crate::native::set_settings(picky, words);
    }

    fn native_reload(self: Pin<&mut Self>, id: &QString) -> QString {
        QString::from(&crate::native::reload(&id.to_string()).to_string())
    }

    fn native_save(self: Pin<&mut Self>) {
        if let Err(e) = crate::native::save() {
            self.message(QString::from(&format!("Could not save: {e}")), true);
        }
    }

    fn delete_project(self: Pin<&mut Self>, id: &QString) {
        let a = app();
        let id = id.to_string();
        let pid = id.clone();
        match a.db(|s| s.delete_project(&a.user, &pid)) {
            Ok(()) => a.state.notify(babel_server::Change {
                user: a.user.id,
                project: id,
                rev: 0,
                origin: None,
                deleted: true,
            }),
            Err(e) => self.message(QString::from(&format!("Could not delete: {e}")), true),
        }
    }

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

pub use qobject::{babel_exit_on_load_failure, babel_init_style, babel_init_webengine, Backend};
