//! Syncing the desktop app's store with an account on a Babel server.
//!
//! Both sides number their saves (revisions). For each project the local
//! store remembers the last server revision it reconciled with and the
//! local revision that matched it. A round then:
//!
//! - uploads projects the server has never seen, and downloads projects
//!   this computer has never seen;
//! - pushes local edits made since the last round: the parts that changed,
//!   based on the server revision last seen. If the server moved on in the
//!   meantime it merges paragraph by paragraph and returns the merged parts,
//!   which are saved locally (the editor reloads);
//! - otherwise pulls server revisions newer than the last one seen;
//! - carries deletions both ways (deletion wins), metadata by most recent
//!   change, and versions as a union.
//!
//! Rounds run one at a time, after local saves, on a timer, and on demand.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use babel_core::{store::Parts, Saved, Summary, User, Version};
use babel_server::{AppState, Change};
use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{watch, Notify};

const ORIGIN: &str = "sync";

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("signed out of the server; sign in again")]
    SignedOut,
    #[error("{0}")]
    Server(String),
    #[error("could not reach the server: {0}")]
    Network(#[from] reqwest::Error),
    #[error("{0}")]
    Local(String),
}

impl From<babel_server::ApiError> for SyncError {
    fn from(e: babel_server::ApiError) -> Self {
        SyncError::Local(e.0.to_string())
    }
}

type Result<T> = std::result::Result<T, SyncError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub server: String,
    pub user: String,
    pub token: String,
}

impl Account {
    fn key(&self) -> String {
        format!("{}|{}", self.server, self.user.to_lowercase())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    pub uploaded: usize,
    pub downloaded: usize,
    pub pushed: usize,
    pub pulled: usize,
    pub merged: usize,
    pub deleted: usize,
    pub versions: usize,
}

impl Report {
    pub fn changed(&self) -> bool {
        *self != Report::default()
    }
}

/// What the local store remembers per project.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Known {
    remote_rev: i64,
    local_rev: i64,
    /// Versions both sides have had; not transferred again (they may have
    /// been pruned on one side since).
    versions: BTreeSet<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SyncState {
    account: String,
    projects: BTreeMap<String, Known>,
}

// ── The server ─────────────────────────────────────────────────────────────

struct Remote {
    http: reqwest::Client,
    base: String,
    token: String,
}

fn normalize_server(server: &str) -> String {
    let s = server.trim().trim_end_matches('/');
    let s = if s.contains("://") { s.to_owned() } else { format!("https://{s}") };
    format!("{s}/")
}

async fn check(res: reqwest::Response) -> Result<reqwest::Response> {
    match res.status() {
        s if s.is_success() => Ok(res),
        reqwest::StatusCode::UNAUTHORIZED => Err(SyncError::SignedOut),
        s => {
            let body = res.text().await.unwrap_or_default();
            Err(SyncError::Server(if body.is_empty() { s.to_string() } else { body }))
        }
    }
}

impl Remote {
    fn new(account: &Account) -> Remote {
        Remote {
            http: client(),
            base: normalize_server(&account.server),
            token: account.token.clone(),
        }
    }

    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}api/{path}", self.base))
            .bearer_auth(&self.token)
            .header("x-babel-client", ORIGIN)
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        Ok(check(self.req(reqwest::Method::GET, path).send().await?).await?.json().await?)
    }

    async fn bytes(&self, path: &str) -> Result<Vec<u8>> {
        Ok(check(self.req(reqwest::Method::GET, path).send().await?).await?.bytes().await?.to_vec())
    }

    async fn send<T: DeserializeOwned>(&self, method: reqwest::Method, path: &str, body: &Value) -> Result<Option<T>> {
        let res = check(self.req(method, path).json(body).send().await?).await?;
        let json = res
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .is_some_and(|v| v.to_str().unwrap_or("").contains("json"));
        Ok(if json { Some(res.json().await?) } else { None })
    }

    async fn delete(&self, path: &str) -> Result<()> {
        check(self.req(reqwest::Method::DELETE, path).send().await?).await?;
        Ok(())
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("babel-desktop/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(300))
        .build()
        .expect("HTTP client")
}

/// Signs in to a server and returns the account to sync with.
pub async fn sign_in(server: &str, user: &str, password: &str, label: &str) -> Result<Account> {
    let base = normalize_server(server);
    let res = client()
        .post(format!("{base}api/login"))
        .json(&json!({ "username": user, "password": password, "token": true, "label": label }))
        .send()
        .await?;
    if res.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(SyncError::Server("Wrong user name or password.".into()));
    }
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(SyncError::Server("That address doesn’t look like a Babel server.".into()));
    }
    let v: Value = check(res).await?.json().await?;
    let token = v["token"].as_str().ok_or_else(|| SyncError::Server("the server sent no token".into()))?;
    let name = v["user"]["name"].as_str().unwrap_or(user).to_owned();
    Ok(Account { server: base.trim_end_matches('/').to_owned(), user: name, token: token.to_owned() })
}

/// Ends the server session (best effort).
pub async fn sign_out(account: &Account) {
    let r = Remote::new(account);
    let _ = r.send::<Value>(reqwest::Method::POST, "logout", &json!({})).await;
}

// ── One round ──────────────────────────────────────────────────────────────

pub struct Engine {
    state: AppState,
    user: User,
    remote: Remote,
    account_key: String,
}

impl Engine {
    pub fn new(state: AppState, user: User, account: &Account) -> Engine {
        Engine { state, user, remote: Remote::new(account), account_key: account.key() }
    }

    async fn load(&self) -> Result<SyncState> {
        let raw = self.state.db(|s| s.get_kv("sync.state")).await?;
        let st: SyncState = raw.and_then(|r| serde_json::from_str(&r).ok()).unwrap_or_default();
        // Another account: nothing about the old one applies.
        Ok(if st.account == self.account_key {
            st
        } else {
            SyncState { account: self.account_key.clone(), ..Default::default() }
        })
    }

    async fn store_state(&self, st: &SyncState) -> Result<()> {
        let raw = serde_json::to_string(st).expect("state serialises");
        self.state.db(move |s| s.set_kv("sync.state", Some(&raw))).await?;
        Ok(())
    }

    fn notify(&self, id: &str, rev: i64, deleted: bool) {
        self.state.notify(Change {
            user: self.user.id,
            project: id.to_owned(),
            rev,
            origin: Some(ORIGIN.into()),
            deleted,
        });
    }

    async fn db<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut babel_core::Store, &User) -> std::result::Result<T, babel_core::Error> + Send + 'static,
    ) -> Result<T> {
        let u = self.user.clone();
        Ok(self.state.db(move |s| f(s, &u)).await?)
    }

    pub async fn run_once(&self) -> Result<Report> {
        let mut st = self.load().await?;
        let mut report = Report::default();
        let remote: Vec<Summary> = self.remote.get("sync/projects").await?;
        let local: Vec<Summary> = self.db(|s, u| s.summaries(u)).await?;
        let rmap: HashMap<&str, &Summary> = remote.iter().map(|r| (r.id.as_str(), r)).collect();
        let lids: BTreeSet<&str> = local.iter().map(|l| l.id.as_str()).collect();

        for l in &local {
            let r = rmap.get(l.id.as_str()).copied();
            let res = self.project(&mut st, &mut report, l, r).await;
            // Save progress after each project, so a failure part-way loses nothing.
            self.store_state(&st).await?;
            if let Err(e) = res {
                match e {
                    SyncError::SignedOut | SyncError::Network(_) => return Err(e),
                    e => tracing::warn!("sync of {} failed: {e}", l.id),
                }
            }
        }
        for r in &remote {
            if r.deleted || lids.contains(r.id.as_str()) || st.projects.contains_key(&r.id) {
                continue;
            }
            let res = self.download(&mut st, &mut report, r).await;
            self.store_state(&st).await?;
            if let Err(e) = res {
                match e {
                    SyncError::SignedOut | SyncError::Network(_) => return Err(e),
                    e => tracing::warn!("download of {} failed: {e}", r.id),
                }
            }
        }
        Ok(report)
    }

    async fn project(&self, st: &mut SyncState, report: &mut Report, l: &Summary, r: Option<&Summary>) -> Result<()> {
        let id = l.id.clone();
        if l.deleted {
            // Deleted here: delete there too if it was synced, then forget it.
            if let Some(r) = r {
                if !r.deleted && st.projects.contains_key(&id) {
                    self.remote.delete(&format!("projects/{id}")).await?;
                    report.deleted += 1;
                }
            }
            let pid = id.clone();
            self.db(move |s, u| s.forget_project(u, &pid)).await?;
            st.projects.remove(&id);
            return Ok(());
        }
        if r.is_some_and(|r| r.deleted) {
            // Deleted on another device.
            let pid = id.clone();
            self.db(move |s, u| s.delete_project(u, &pid)).await?;
            self.db({ let pid = id.clone(); move |s, u| s.forget_project(u, &pid) }).await?;
            st.projects.remove(&id);
            self.notify(&id, 0, true);
            report.deleted += 1;
            return Ok(());
        }
        if r.is_none() {
            self.upload(l).await?;
            st.projects.insert(id.clone(), Known::default());
            report.uploaded += 1;
        }
        let mut k = st.projects.get(&id).cloned().unwrap_or_default();
        let r_rev = r.map(|r| r.rev).unwrap_or(0);

        if l.rev > k.local_rev {
            // Push what changed here since the last round.
            let pid = id.clone();
            let base_rev = k.local_rev;
            let (lrev, parts, base) = self
                .db(move |s, u| {
                    let (rev, parts) = s.parts(u, &pid)?;
                    let base = s.parts_at(u, &pid, base_rev)?;
                    Ok((rev, parts, base))
                })
                .await?;
            let changed: Parts = match &base {
                Some(b) => parts.iter().filter(|(p, x)| b.get(*p) != Some(*x)).map(|(p, x)| (p.clone(), x.clone())).collect(),
                None => parts.clone(),
            };
            let pid = id.clone();
            let meta = self.db(move |s, u| s.get_project(u, &pid)).await?;
            let saved: Saved = self
                .remote
                .send(
                    reqwest::Method::POST,
                    &format!("projects/{id}/parts"),
                    &json!({ "baseRev": k.remote_rev, "project": meta, "parts": changed }),
                )
                .await?
                .ok_or_else(|| SyncError::Server("empty reply to a save".into()))?;
            k.remote_rev = saved.rev;
            k.local_rev = lrev;
            if saved.merged {
                let merged = saved.parts.unwrap_or_default();
                let local = self.apply(&id, lrev, None, merged).await?;
                k.local_rev = local;
                report.merged += 1;
            }
            report.pushed += 1;
        } else if r_rev > k.remote_rev {
            // Pull what changed there.
            let got: Value = self.remote.get(&format!("projects/{id}/parts")).await?;
            let rev = got["rev"].as_i64().unwrap_or(r_rev);
            let parts: Parts = serde_json::from_value(got["parts"].clone()).map_err(|e| SyncError::Server(e.to_string()))?;
            let meta = r.map(|r| r.meta.clone());
            k.local_rev = self.apply(&id, l.rev, meta, parts).await?;
            k.remote_rev = rev;
            report.pulled += 1;
        } else if let Some(r) = r {
            // Same parts; the most recent metadata wins.
            if r.updated_at > l.updated_at && r.meta != l.meta {
                let (pid, meta) = (id.clone(), r.meta.clone());
                self.db(move |s, u| s.save_meta(u, &pid, &meta)).await?;
                self.notify(&id, l.rev, false);
            } else if l.updated_at > r.updated_at && r.meta != l.meta {
                self.remote.send::<Value>(reqwest::Method::PUT, &format!("projects/{id}/meta"), &l.meta).await?;
            }
        }
        st.projects.insert(id.clone(), k);
        self.versions(st, report, &id).await
    }

    /// Saves parts that came from the server over local revision `base`.
    /// Returns the local revision that now matches the server: the new one,
    /// or `base` if the editor saved in the meantime (so that its edits are
    /// pushed next round).
    async fn apply(&self, id: &str, base: i64, meta: Option<Value>, parts: Parts) -> Result<i64> {
        let pid = id.to_owned();
        let saved = self.db(move |s, u| s.save_parts(u, &pid, base, meta.as_ref(), &parts, Some(ORIGIN))).await?;
        self.notify(id, saved.rev, false);
        Ok(if saved.merged { base } else { saved.rev })
    }

    async fn upload(&self, l: &Summary) -> Result<()> {
        let pid = l.id.clone();
        let (docx, source) = self.db(move |s, u| Ok((s.source_docx(u, &pid)?, s.source_parts(u, &pid)?))).await?;
        let mut meta = l.meta.clone();
        meta["id"] = json!(l.id);
        self.remote
            .send::<Value>(
                reqwest::Method::POST,
                "projects",
                &json!({
                    "project": meta,
                    "source": { "parts": source, "bytes": base64::engine::general_purpose::STANDARD.encode(docx) },
                }),
            )
            .await?;
        Ok(())
    }

    async fn download(&self, st: &mut SyncState, report: &mut Report, r: &Summary) -> Result<()> {
        let id = r.id.clone();
        let source: Parts = self.remote.get(&format!("projects/{id}/source")).await?;
        let docx = self.remote.bytes(&format!("projects/{id}/source.docx")).await?;
        let got: Value = self.remote.get(&format!("projects/{id}/parts")).await?;
        let rev = got["rev"].as_i64().unwrap_or(0);
        let parts: Parts = serde_json::from_value(got["parts"].clone()).map_err(|e| SyncError::Server(e.to_string()))?;
        let meta = r.meta.clone();
        let pid = id.clone();
        let local_rev = self
            .db(move |s, u| {
                s.create_project(u, &meta, &docx, &source)?;
                if parts.is_empty() {
                    return Ok(0);
                }
                Ok(s.save_parts(u, &pid, 0, None, &parts, Some(ORIGIN))?.rev)
            })
            .await?;
        st.projects.insert(id.clone(), Known { remote_rev: rev, local_rev, versions: BTreeSet::new() });
        self.notify(&id, local_rev, false);
        report.downloaded += 1;
        self.versions(st, report, &id).await
    }

    async fn versions(&self, st: &mut SyncState, report: &mut Report, id: &str) -> Result<()> {
        let rv: Vec<Version> = self.remote.get(&format!("projects/{id}/versions")).await?;
        let pid = id.to_owned();
        let lv = self.db(move |s, u| s.list_versions(u, &pid)).await?;
        let k = st.projects.entry(id.to_owned()).or_default();
        let rs: BTreeSet<String> = rv.iter().map(|v| v.sid.clone()).collect();
        let ls: BTreeSet<String> = lv.iter().map(|v| v.sid.clone()).collect();
        for sid in ls.difference(&rs) {
            if k.versions.contains(sid) {
                continue;
            }
            let s2 = sid.clone();
            let (_, v) = self.db(move |s, u| s.get_version(u, &s2)).await?;
            self.remote.send::<Value>(reqwest::Method::POST, &format!("projects/{id}/versions"), &json!(v)).await?;
            k.versions.insert(sid.clone());
            report.versions += 1;
        }
        for sid in rs.difference(&ls) {
            if k.versions.contains(sid) {
                continue;
            }
            let v: Version = self.remote.get(&format!("versions/{sid}")).await?;
            let pid = id.to_owned();
            self.db(move |s, u| s.add_version(u, &pid, &v)).await?;
            k.versions.insert(sid.clone());
            report.versions += 1;
        }
        for sid in ls.intersection(&rs) {
            k.versions.insert(sid.clone());
        }
        k.versions.retain(|s| ls.contains(s) || rs.contains(s));
        Ok(())
    }
}

// ── Running in the background ──────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub syncing: bool,
    /// When the last round finished (ms), if one has.
    pub last: Option<i64>,
    pub error: Option<String>,
    pub signed_out: bool,
}

/// A background task syncing one account until dropped.
pub struct Syncer {
    kick: Arc<Notify>,
    status: watch::Receiver<Status>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Syncer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Syncer {
    /// Starts syncing: right away, a few seconds after local saves, and every
    /// `interval` to pick up edits made elsewhere.
    pub fn start(state: AppState, user: User, account: &Account, interval: Duration) -> Syncer {
        let kick = Arc::new(Notify::new());
        let (tx, status) = watch::channel(Status::default());
        let engine = Engine::new(state.clone(), user.clone(), account);
        let k = kick.clone();
        let mut changes = state.changes.subscribe();
        let uid = user.id;
        // Local saves trigger a round shortly after typing pauses.
        let k2 = kick.clone();
        let watcher = tokio::spawn(async move {
            loop {
                match changes.recv().await {
                    Ok(c) if c.user == uid && c.origin.as_deref() != Some(ORIGIN) => {
                        tokio::time::sleep(Duration::from_secs(3)).await;
                        while changes.try_recv().is_ok() {}
                        k2.notify_one();
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    _ => {}
                }
            }
        });
        let task = tokio::spawn(async move {
            let _watcher = AbortOnDrop(watcher);
            loop {
                tx.send_modify(|s| s.syncing = true);
                let res = engine.run_once().await;
                let now = babel_core::store::now_ms();
                let signed_out = matches!(res, Err(SyncError::SignedOut));
                tx.send_modify(|s| {
                    s.syncing = false;
                    match &res {
                        Ok(_) => {
                            s.last = Some(now);
                            s.error = None;
                        }
                        Err(e) => s.error = Some(e.to_string()),
                    }
                    s.signed_out = signed_out;
                });
                if let Ok(r) = res {
                    if r.changed() {
                        tracing::info!("synced: {r:?}");
                    }
                }
                if signed_out {
                    break;
                }
                let _ = tokio::time::timeout(interval, k.notified()).await;
            }
        });
        Syncer { kick, status, task }
    }

    pub fn sync_now(&self) {
        self.kick.notify_one();
    }

    pub fn status(&self) -> watch::Receiver<Status> {
        self.status.clone()
    }
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
