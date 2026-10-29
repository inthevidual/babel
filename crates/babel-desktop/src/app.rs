//! What runs behind the window: the store on this computer, served on
//! localhost with the same API as babel-server, for the editor panels.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use axum::extract::Path;
use axum::routing::get;
use axum::{Extension, Router};
use babel_core::{Error, Store, User};
use babel_server::{ApiError, AppState, Auth, Mode};

/// The editor's origin must stay the same between runs: its recovery
/// journal lives in the origin's localStorage.
const PORT: u16 = 47917;
const LOCAL_USER: &str = "local";

/// Files picked in the native dialog, waiting for the editor to fetch them.
#[derive(Clone, Default)]
pub struct Staged(Arc<Mutex<HashMap<String, (String, Vec<u8>)>>>);

pub struct App {
    pub rt: tokio::runtime::Runtime,
    pub state: AppState,
    pub user: User,
    /// `http://127.0.0.1:PORT/`
    pub base: String,
    pub token: String,
    pub data_dir: PathBuf,
    staged: Staged,
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

async fn staged_file(
    _: Auth,
    Extension(staged): Extension<Staged>,
    Path(id): Path<String>,
) -> Result<Vec<u8>, ApiError> {
    staged.0.lock().unwrap().remove(&id).map(|(_, b)| b).ok_or(ApiError(Error::NotFound))
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
            // Nobody signs in with it; the app hands the editor a session.
            None => store.create_user(LOCAL_USER, &babel_core::auth::new_token())?,
        };
        let token = match store.get_kv("desktop.token")? {
            Some(t) if store.session_user(&t)?.is_some() => t,
            _ => {
                let t = store.create_session(&user, "desktop editor")?;
                store.set_kv("desktop.token", Some(&t))?;
                t
            }
        };

        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
        let state = AppState::new(store, Mode::Desktop);
        let staged = Staged::default();
        let listener = rt.block_on(async {
            match tokio::net::TcpListener::bind(("127.0.0.1", PORT)).await {
                Ok(l) => Ok(l),
                Err(e) => {
                    tracing::warn!("port {PORT} is taken ({e}); unsaved-edit recovery starts afresh on another port");
                    tokio::net::TcpListener::bind(("127.0.0.1", 0)).await
                }
            }
        })?;
        let base = format!("http://{}/", listener.local_addr()?);
        let extra: Router<AppState> = Router::new()
            .route("/desktop/staged/{id}", get(staged_file))
            .layer(Extension(staged.clone()));
        let router = babel_server::router_with(state.clone(), extra);
        rt.spawn(async move {
            if let Err(e) = axum::serve(listener, router).await {
                tracing::error!("local server stopped: {e}");
            }
        });
        tracing::info!("editor on {base}");
        let app = App { rt, state, user, base, token, data_dir, staged };
        APP.set(app).map_err(|_| anyhow::anyhow!("started twice"))?;
        Ok(self::app())
    }

    /// Runs `f` on the store (blocking; for the UI thread's quick reads).
    pub fn db<T>(&self, f: impl FnOnce(&mut Store) -> Result<T, Error>) -> Result<T, Error> {
        f(&mut self.state.store.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Holds a file for the editor to fetch once; returns its id.
    pub fn stage(&self, name: String, bytes: Vec<u8>) -> String {
        let id = uuid::Uuid::new_v4().simple().to_string();
        self.staged.0.lock().unwrap().insert(id.clone(), (name, bytes));
        id
    }

    /// The address that signs the editor in and opens it.
    pub fn editor_url(&self) -> String {
        format!("{}auth?token={}", self.base, self.token)
    }
}
