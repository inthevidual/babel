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

        /// Lays out both documents natively; answers with pagesReady.
        #[qinvokable]
        #[cxx_name = "renderPages"]
        fn render_pages(self: Pin<&mut Backend>, id: &QString);

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

        /// JSON: {source: [{url, w, h}], target: [...], ms}.
        #[qsignal]
        #[cxx_name = "pagesReady"]
        fn pages_ready(self: Pin<&mut Backend>, id: QString, pages: QString);

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

    fn render_pages(self: Pin<&mut Self>, id: &QString) {
        let qt = self.qt_thread();
        let id = id.to_string();
        app().rt.spawn_blocking(move || {
            let res = crate::pages::render(&id);
            let _ = qt.queue(move |o| match res {
                Ok(r) => o.pages_ready(QString::from(&id), QString::from(&serde_json::to_string(&r).unwrap_or_default())),
                Err(e) => o.message(QString::from(&format!("The native layout failed: {e}")), true),
            });
        });
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
        use crate::native::Key;
        use babel_pages::edit::Style;
        const SHIFT: i32 = 0x0200_0000;
        const CTRL: i32 = 0x0400_0000;
        let (shift, ctrl) = (modifiers & SHIFT != 0, modifiers & CTRL != 0);
        let k = match (key, ctrl) {
            (0x5A, true) if shift => Key::Redo,
            (0x5A, true) => Key::Undo,
            (0x59, true) => Key::Redo,
            (0x42, true) => Key::Toggle(Style::Bold),
            (0x49, true) => Key::Toggle(Style::Italic),
            (0x55, true) => Key::Toggle(Style::Underline),
            (0x41, true) => Key::SelectAll,
            (_, true) => return QString::from("null"),
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
                let t = text.to_string();
                if t.is_empty() || t.chars().any(|c| c.is_control() && c != '\t') {
                    return QString::from("null");
                }
                Key::Text(t)
            }
        };
        QString::from(&crate::native::key(k, shift).to_string())
    }

    fn native_text(self: Pin<&mut Self>, text: &QString) -> QString {
        let t = text.to_string();
        if t.is_empty() {
            return QString::from("null");
        }
        QString::from(&crate::native::key(crate::native::Key::Text(t), false).to_string())
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
        QString::from(&crate::native::key(crate::native::Key::Delete, false).to_string())
    }

    fn native_paste(self: Pin<&mut Self>) -> QString {
        let t = qobject::babel_clipboard_text().to_string();
        if t.is_empty() {
            return QString::from("null");
        }
        QString::from(&crate::native::key(crate::native::Key::Text(t), false).to_string())
    }

    fn native_issues(self: Pin<&mut Self>) -> QString {
        QString::from(&crate::native::issues().to_string())
    }

    fn native_apply(self: Pin<&mut Self>, pid: &QString, n: i32, value: &QString) -> QString {
        QString::from(&crate::native::apply_suggestion(&pid.to_string(), n.max(0) as usize, &value.to_string()).to_string())
    }

    fn native_ignore(self: Pin<&mut Self>, pid: &QString, n: i32) {
        if let Err(e) = crate::native::ignore(&pid.to_string(), n.max(0) as usize) {
            self.message(QString::from(&format!("Could not save that: {e}")), true);
        }
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
