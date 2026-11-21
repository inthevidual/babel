//! Babel for the desktop without a toolkit: the store on this computer (the
//! same one babel-server keeps, with a single local user), the translations
//! as wholes (projects), the editing session (babel-editor, saved as you
//! type, proofed in the background) and the account it syncs with. The
//! windows are the platforms' own: babel-desktop (Qt/Kirigami) and the macOS
//! app (babel-mac and macos/). Answers are JSON, as the editor gives them;
//! what happens by itself (proofing results, saves from elsewhere, sync)
//! comes as [`Event`]s.

pub mod account;
pub mod projects;
pub mod session;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use babel_core::{Error, Store, User};
use babel_server::{AppState, Mode};

const LOCAL_USER: &str = "local";

pub struct App {
    pub rt: tokio::runtime::Runtime,
    /// The store and its change broadcasts (sync listens; the editor reloads).
    pub state: AppState,
    pub user: User,
}

/// Something the window should know about; sent from any thread.
#[derive(Debug, Clone)]
pub enum Event {
    /// New proofing results are in for the open translation.
    Issues,
    /// The translation being edited was saved from elsewhere (sync, a restore).
    ExternalChange(String),
    /// The list of translations changed.
    Projects,
    /// The account and how syncing goes.
    Sync(account::State),
    /// Something the user should be told about.
    Message { text: String, error: bool },
}

static APP: OnceLock<App> = OnceLock::new();
static EVENTS: OnceLock<Box<dyn Fn(Event) + Send + Sync>> = OnceLock::new();

pub fn app() -> &'static App {
    APP.get().expect("App::start runs before the window opens")
}

/// Tells the window (once it has connected).
pub fn emit(e: Event) {
    if let Some(f) = EVENTS.get() {
        f(e);
    }
}

/// Where Linux keeps the data: $XDG_DATA_HOME/babel or ~/.local/share/babel.
pub fn xdg_data_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share"));
    base.join("babel")
}

impl App {
    /// Opens the store in `data_dir` (made, readable only by the user).
    pub fn start(data_dir: &Path) -> anyhow::Result<&'static App> {
        std::fs::create_dir_all(data_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(data_dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let store = Store::open(&data_dir.join("babel.db"))?;
        let user = match store.user_by_name(LOCAL_USER)? {
            Some(u) => u,
            // Nobody signs in with it: it owns this computer's translations.
            None => store.create_user(LOCAL_USER, &babel_core::auth::new_token())?,
        };
        // Sync and proofing need little: two worker threads.
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
        let state = AppState::new(store, Mode::Desktop);
        let app = App { rt, state, user };
        APP.set(app).map_err(|_| anyhow::anyhow!("started twice"))?;
        Ok(self::app())
    }

    /// Runs `f` on the store (blocking; for the UI thread's quick work).
    pub fn db<T>(&self, f: impl FnOnce(&mut Store) -> Result<T, Error>) -> Result<T, Error> {
        f(&mut self.state.store.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// The window is ready for events: from now on they go to `f`. Starts
/// watching for saves (to refresh the list, and to reload the translation
/// being edited when it was saved elsewhere) and syncing, if signed in.
pub fn connect(f: impl Fn(Event) + Send + Sync + 'static) {
    if EVENTS.set(Box::new(f)).is_err() {
        return;
    }
    let a = app();
    let mut changes = a.state.changes.subscribe();
    let uid = a.user.id;
    a.rt.spawn(async move {
        loop {
            match changes.recv().await {
                Ok(c) if c.user != uid => continue,
                // The translation being edited, saved from elsewhere.
                Ok(c) if c.origin.as_deref() != Some("native") && session::current().as_deref() == Some(c.project.as_str()) => {
                    emit(Event::ExternalChange(c.project));
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                _ => {}
            }
            // Typing saves every second or so; one refresh per burst is plenty.
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            while changes.try_recv().is_ok() {}
            emit(Event::Projects);
        }
    });
    account::attach();
}
