//! Babel's HTTP API and the web app, over a [`babel_core::Store`].
//!
//! The same router runs as the multi-user server (`babel-server serve`) and
//! inside the desktop app, bound to localhost with the computer's own store.
//! The web app in `web/` talks to it through `store-http.js`.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{DefaultBodyLimit, FromRequestParts, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header, request::Parts as ReqParts};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use babel_core::{Error, Store, User, Version, store::Parts};
use base64::Engine;
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

pub const COOKIE: &str = "babel_session";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Accounts, signing in with a password.
    Server,
    /// The desktop app's local server: one user, signed in by the app.
    Desktop,
}

/// Something happened to a project; open editors reload.
#[derive(Debug, Clone)]
pub struct Change {
    pub user: i64,
    pub project: String,
    pub rev: i64,
    pub origin: Option<String>,
    pub deleted: bool,
}

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Mutex<Store>>,
    pub changes: broadcast::Sender<Change>,
    pub mode: Mode,
    pub secure_cookies: bool,
    failures: Arc<Mutex<HashMap<String, (u32, Instant)>>>,
}

impl AppState {
    pub fn new(store: Store, mode: Mode) -> AppState {
        AppState {
            store: Arc::new(Mutex::new(store)),
            changes: broadcast::channel(256).0,
            mode,
            secure_cookies: false,
            failures: Default::default(),
        }
    }

    /// Runs `f` on the store off the async runtime.
    pub async fn db<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Store) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, ApiError> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || f(&mut store.lock().unwrap_or_else(|e| e.into_inner())))
            .await
            .map_err(|e| ApiError(Error::Internal(e.to_string())))?
            .map_err(ApiError)
    }

    pub fn notify(&self, change: Change) {
        let _ = self.changes.send(change);
    }
}

// ── Errors ─────────────────────────────────────────────────────────────────

pub struct ApiError(pub Error);

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        ApiError(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            Error::NotFound => StatusCode::NOT_FOUND,
            Error::Auth => StatusCode::UNAUTHORIZED,
            Error::Conflict(_) => StatusCode::CONFLICT,
            Error::BadRequest(_) => StatusCode::BAD_REQUEST,
            Error::Internal(_) | Error::Db(_) => {
                tracing::error!("{}", self.0);
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        (status, self.0.to_string()).into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

// ── Authentication ─────────────────────────────────────────────────────────

/// The signed-in user, from the session cookie or a bearer token.
pub struct Auth {
    pub user: User,
    token: String,
}

fn token_from(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(t) = v.strip_prefix("Bearer ") {
            return Some(t.trim().to_owned());
        }
    }
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|c| c.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE)
        .map(|(_, v)| v.to_owned())
}

impl FromRequestParts<AppState> for Auth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut ReqParts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = token_from(&parts.headers).ok_or(ApiError(Error::Auth))?;
        let t = token.clone();
        let user = state
            .db(move |s| s.session_user(&t))
            .await?
            .ok_or(ApiError(Error::Auth))?;
        Ok(Auth { user, token })
    }
}

fn origin(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-babel-client")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.chars().take(64).collect())
}

fn session_cookie(state: &AppState, token: &str, max_age: i64) -> HeaderValue {
    let secure = if state.secure_cookies { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}"
    ))
    .expect("cookie is ASCII")
}

#[derive(Deserialize)]
struct Login {
    username: String,
    password: String,
    /// The desktop app's sync asks for the token instead of a cookie.
    #[serde(default)]
    token: bool,
    #[serde(default)]
    label: Option<String>,
}

const MAX_FAILURES: u32 = 8;
const FAILURE_WINDOW: Duration = Duration::from_secs(15 * 60);

async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Login>,
) -> ApiResult<Response> {
    if state.mode != Mode::Server {
        return Err(ApiError(Error::NotFound));
    }
    let key = body.username.trim().to_lowercase();
    {
        let mut f = state.failures.lock().unwrap();
        f.retain(|_, (_, t)| t.elapsed() < FAILURE_WINDOW);
        if f.get(&key).is_some_and(|(n, _)| *n >= MAX_FAILURES) {
            return Ok((
                StatusCode::TOO_MANY_REQUESTS,
                "Too many attempts. Try again in a quarter of an hour.",
            )
                .into_response());
        }
    }
    let label = body.label.clone().unwrap_or_else(|| {
        headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("browser")
            .chars()
            .take(120)
            .collect()
    });
    let (u, p) = (body.username.clone(), body.password.clone());
    let res = state
        .db(move |s| {
            let user = s.verify_user(&u, &p)?;
            let token = s.create_session(&user, &label)?;
            Ok((user, token))
        })
        .await;
    let (user, token) = match res {
        Ok(v) => v,
        Err(ApiError(Error::Auth)) => {
            let mut f = state.failures.lock().unwrap();
            let e = f.entry(key).or_insert((0, Instant::now()));
            e.0 += 1;
            e.1 = Instant::now();
            return Err(ApiError(Error::Auth));
        }
        Err(e) => return Err(e),
    };
    state.failures.lock().unwrap().remove(&key);
    let user_json = json!({ "name": user.name });
    if body.token {
        return Ok(Json(json!({ "user": user_json, "token": token })).into_response());
    }
    let mut res = Json(json!({ "user": user_json })).into_response();
    res.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie(&state, &token, 400 * 86400),
    );
    Ok(res)
}

async fn logout(State(state): State<AppState>, auth: Auth) -> ApiResult<Response> {
    let t = auth.token.clone();
    state.db(move |s| s.delete_session(&t)).await?;
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut()
        .insert(header::SET_COOKIE, session_cookie(&state, "", 0));
    Ok(res)
}

#[derive(Deserialize)]
struct Handoff {
    token: String,
}

/// Desktop only: the app opens `/auth?token=…` with a session it created,
/// and the page continues with it as a cookie.
async fn handoff(State(state): State<AppState>, Query(q): Query<Handoff>) -> ApiResult<Response> {
    if state.mode != Mode::Desktop {
        return Err(ApiError(Error::NotFound));
    }
    let t = q.token.clone();
    state
        .db(move |s| s.session_user(&t))
        .await?
        .ok_or(ApiError(Error::Auth))?;
    let mut res = Redirect::to("/").into_response();
    res.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie(&state, &q.token, 400 * 86400),
    );
    Ok(res)
}

async fn config(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let user = match token_from(&headers) {
        Some(t) => state.db(move |s| s.session_user(&t)).await?,
        None => None,
    };
    Ok(Json(json!({
        "babel": true,
        "mode": state.mode,
        "version": env!("CARGO_PKG_VERSION"),
        "user": user.map(|u| json!({ "name": u.name })),
    })))
}

// ── Projects ───────────────────────────────────────────────────────────────

async fn list_projects(State(state): State<AppState>, a: Auth) -> ApiResult<Json<Vec<Value>>> {
    Ok(Json(state.db(move |s| s.list_projects(&a.user)).await?))
}

async fn summaries(State(state): State<AppState>, a: Auth) -> ApiResult<Json<Value>> {
    Ok(Json(json!(state.db(move |s| s.summaries(&a.user)).await?)))
}

#[derive(Deserialize)]
struct NewProject {
    project: Value,
    source: NewSource,
}

#[derive(Deserialize)]
struct NewSource {
    parts: Parts,
    bytes: String,
}

async fn create_project(
    State(state): State<AppState>,
    a: Auth,
    Json(body): Json<NewProject>,
) -> ApiResult<StatusCode> {
    let docx = base64::engine::general_purpose::STANDARD
        .decode(body.source.bytes.as_bytes())
        .map_err(|_| ApiError(Error::BadRequest("the document is not valid base64".into())))?;
    if !docx.starts_with(b"PK") {
        return Err(ApiError(Error::BadRequest(
            "that is not a .docx file".into(),
        )));
    }
    let uid = a.user.id;
    let id = body
        .project
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    state
        .db(move |s| s.create_project(&a.user, &body.project, &docx, &body.source.parts))
        .await?;
    state.notify(Change {
        user: uid,
        project: id,
        rev: 0,
        origin: None,
        deleted: false,
    });
    Ok(StatusCode::CREATED)
}

async fn get_project(
    State(state): State<AppState>,
    a: Auth,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    Ok(Json(state.db(move |s| s.get_project(&a.user, &id)).await?))
}

async fn save_meta(
    State(state): State<AppState>,
    a: Auth,
    Path(id): Path<String>,
    Json(meta): Json<Value>,
) -> ApiResult<StatusCode> {
    state.db(move |s| s.save_meta(&a.user, &id, &meta)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_project(
    State(state): State<AppState>,
    a: Auth,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let uid = a.user.id;
    let pid = id.clone();
    state.db(move |s| s.delete_project(&a.user, &pid)).await?;
    state.notify(Change {
        user: uid,
        project: id,
        rev: 0,
        origin: None,
        deleted: true,
    });
    Ok(StatusCode::NO_CONTENT)
}

async fn source_parts(
    State(state): State<AppState>,
    a: Auth,
    Path(id): Path<String>,
) -> ApiResult<Json<Parts>> {
    Ok(Json(state.db(move |s| s.source_parts(&a.user, &id)).await?))
}

async fn source_docx(
    State(state): State<AppState>,
    a: Auth,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let bytes = state.db(move |s| s.source_docx(&a.user, &id)).await?;
    Ok((
        [(
            header::CONTENT_TYPE,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        )],
        bytes,
    )
        .into_response())
}

#[derive(Deserialize)]
struct RevQuery {
    rev: Option<i64>,
}

async fn get_parts(
    State(state): State<AppState>,
    a: Auth,
    Path(id): Path<String>,
    Query(q): Query<RevQuery>,
) -> ApiResult<Json<Value>> {
    let r = state
        .db(move |s| match q.rev {
            Some(rev) => Ok((rev, s.parts_at(&a.user, &id, rev)?.ok_or(Error::NotFound)?)),
            None => s.parts(&a.user, &id),
        })
        .await?;
    Ok(Json(json!({ "rev": r.0, "parts": r.1 })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Save {
    base_rev: i64,
    #[serde(default)]
    project: Option<Value>,
    parts: Parts,
}

async fn save_parts(
    State(state): State<AppState>,
    a: Auth,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Save>,
) -> ApiResult<Json<babel_core::Saved>> {
    let origin = origin(&headers);
    let uid = a.user.id;
    let (pid, o) = (id.clone(), origin.clone());
    let saved = state
        .db(move |s| {
            s.save_parts(
                &a.user,
                &pid,
                body.base_rev,
                body.project.as_ref(),
                &body.parts,
                o.as_deref(),
            )
        })
        .await?;
    state.notify(Change {
        user: uid,
        project: id,
        rev: saved.rev,
        origin,
        deleted: false,
    });
    Ok(Json(saved))
}

async fn list_versions(
    State(state): State<AppState>,
    a: Auth,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<Version>>> {
    Ok(Json(
        state.db(move |s| s.list_versions(&a.user, &id)).await?,
    ))
}

async fn add_version(
    State(state): State<AppState>,
    a: Auth,
    Path(id): Path<String>,
    Json(v): Json<Version>,
) -> ApiResult<Json<Value>> {
    let sid = state.db(move |s| s.add_version(&a.user, &id, &v)).await?;
    Ok(Json(json!({ "sid": sid })))
}

async fn get_version(
    State(state): State<AppState>,
    a: Auth,
    Path(sid): Path<String>,
) -> ApiResult<Json<Version>> {
    Ok(Json(
        state.db(move |s| s.get_version(&a.user, &sid)).await?.1,
    ))
}

/// Server-sent events: `rev` when a project is saved, `deleted` when it is.
async fn events(
    State(state): State<AppState>,
    a: Auth,
    Path(id): Path<String>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let pid = id.clone();
    let uid = a.user.id;
    state
        .db(move |s| s.get_project(&a.user, &pid).map(|_| ()))
        .await?;
    let stream = BroadcastStream::new(state.changes.subscribe()).filter_map(move |c| {
        let c = c.ok()?;
        if c.user != uid || c.project != id {
            return None;
        }
        Some(Ok(if c.deleted {
            Event::default().event("deleted").data("{}")
        } else {
            Event::default()
                .event("rev")
                .data(json!({ "rev": c.rev, "origin": c.origin }).to_string())
        }))
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

// ── The web app ────────────────────────────────────────────────────────────

#[cfg(feature = "web")]
#[derive(rust_embed::RustEmbed)]
#[folder = "../../web/"]
struct Web;

#[cfg(not(feature = "web"))]
async fn static_file(_: Uri) -> Response {
    (StatusCode::NOT_FOUND, "not found").into_response()
}

#[cfg(feature = "web")]
async fn static_file(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Web::get(path) {
        Some(f) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let cache = if path.starts_with("fonts/") {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            };
            (
                [
                    (
                        header::CONTENT_TYPE,
                        HeaderValue::from_str(mime.as_ref()).unwrap(),
                    ),
                    (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
                    (
                        header::ETAG,
                        HeaderValue::from_str(&format!("\"{}\"", hex(&f.metadata.sha256_hash())))
                            .unwrap(),
                    ),
                ],
                f.data,
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

#[cfg(feature = "web")]
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

async fn api_not_found() -> ApiError {
    ApiError(Error::NotFound)
}

pub fn router(state: AppState) -> Router {
    router_with(state, Router::new())
}

/// The router with extra API routes under `/api` (the desktop app's).
pub fn router_with(state: AppState, extra_api: Router<AppState>) -> Router {
    let api = extra_api
        .route("/config", get(config))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/projects", get(list_projects).post(create_project))
        .route("/projects/{id}", get(get_project).delete(delete_project))
        .route("/projects/{id}/meta", axum::routing::put(save_meta))
        .route("/projects/{id}/source", get(source_parts))
        .route("/projects/{id}/source.docx", get(source_docx))
        .route("/projects/{id}/parts", get(get_parts).post(save_parts))
        .route(
            "/projects/{id}/versions",
            get(list_versions).post(add_version),
        )
        .route("/projects/{id}/events", get(events))
        .route("/versions/{sid}", get(get_version))
        .route("/sync/projects", get(summaries))
        .fallback(api_not_found)
        .layer(DefaultBodyLimit::max(512 * 1024 * 1024));
    Router::new()
        .nest("/api", api)
        .route("/auth", get(handoff))
        .fallback(static_file)
        .layer(tower_http::compression::CompressionLayer::new())
        .with_state(state)
}
