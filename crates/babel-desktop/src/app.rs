//! What runs behind the window: the store on this computer (the same one
//! babel-server keeps, with a single local user) and the runtime that syncs
//! it with an account and checks spelling.

use std::path::PathBuf;
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

static APP: OnceLock<App> = OnceLock::new();

pub fn app() -> &'static App {
    APP.get().expect("App::start runs before the window opens")
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share"));
    base.join("babel")
}

impl App {
    pub fn start() -> anyhow::Result<&'static App> {
        let data_dir = data_dir();
        std::fs::create_dir_all(&data_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o700))?;
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
