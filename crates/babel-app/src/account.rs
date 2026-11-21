//! The account: signing in to a Babel server and syncing with it in the
//! background (babel-sync), told to the window as [`Event::Sync`].

use std::sync::Mutex;
use std::time::Duration;

use babel_sync::{Account, Status, Syncer};

use crate::{app, emit, Event};

/// How often to look for edits made on other devices.
const INTERVAL: Duration = Duration::from_secs(20);

static SYNCER: Mutex<Option<Syncer>> = Mutex::new(None);
static STATE: Mutex<State> = Mutex::new(State { signed_in: false, syncing: false, server: String::new(), user: String::new(), status: String::new() });

/// The account as the window shows it.
#[derive(Debug, Clone, Default)]
pub struct State {
    pub signed_in: bool,
    pub syncing: bool,
    pub server: String,
    pub user: String,
    /// "Synced at 14:02", "Syncing…", what went wrong; or empty.
    pub status: String,
}

impl State {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({ "signedIn": self.signed_in, "syncing": self.syncing, "server": self.server, "user": self.user, "status": self.status })
    }
}

pub fn state() -> State {
    STATE.lock().unwrap().clone()
}

fn update(f: impl FnOnce(&mut State)) {
    let s = {
        let mut g = STATE.lock().unwrap();
        f(&mut g);
        g.clone()
    };
    emit(Event::Sync(s));
}

fn saved_account() -> Option<Account> {
    let raw = app().db(|s| s.get_kv("sync.account")).ok().flatten()?;
    serde_json::from_str(&raw).ok()
}

fn status_text(s: &Status) -> String {
    if s.signed_out {
        return "Signed out by the server. Sign in again to keep syncing.".into();
    }
    if s.syncing {
        return "Syncing…".into();
    }
    if let Some(e) = &s.error {
        return format!("Not synced: {e}");
    }
    match s.last.and_then(chrono::DateTime::from_timestamp_millis) {
        Some(t) => format!("Synced at {}", t.with_timezone(&chrono::Local).format("%H:%M")),
        None => String::new(),
    }
}

fn start(account: &Account) {
    let a = app();
    let _guard = a.rt.enter();
    let syncer = Syncer::start(a.state.clone(), a.user.clone(), account, INTERVAL);
    let mut status = syncer.status();
    a.rt.spawn(async move {
        loop {
            let s = status.borrow_and_update().clone();
            update(|st| {
                st.syncing = s.syncing;
                st.status = status_text(&s);
                if s.signed_out {
                    st.signed_in = false;
                }
            });
            if status.changed().await.is_err() {
                break;
            }
        }
    });
    *SYNCER.lock().unwrap() = Some(syncer);
}

/// Carries on syncing with the account signed in before, if any.
pub fn attach() {
    if let Some(acc) = saved_account() {
        update(|st| {
            st.server = acc.server.clone();
            st.user = acc.user.clone();
            st.signed_in = true;
        });
        start(&acc);
    }
}

/// Signs in in the background; `device` names this computer on the server
/// ("Babel desktop on …"). Tells how it went with a message.
pub fn sign_in(server: String, user: String, password: String, device: String) {
    update(|st| st.status = "Signing in…".into());
    let label = format!("Babel desktop on {}", if device.trim().is_empty() { "this computer" } else { device.trim() });
    app().rt.spawn(async move {
        match babel_sync::sign_in(&server, &user, &password, &label).await {
            Ok(acc) => {
                let raw = serde_json::to_string(&acc).expect("account serialises");
                if let Err(e) = app().db(|s| s.set_kv("sync.account", Some(&raw))) {
                    emit(Event::Message { text: format!("Could not save the account: {e}"), error: true });
                    return;
                }
                update(|st| {
                    st.server = acc.server.clone();
                    st.user = acc.user.clone();
                    st.signed_in = true;
                });
                emit(Event::Message { text: format!("Signed in as {}. Your translations sync now.", acc.user), error: false });
                start(&acc);
            }
            Err(e) => {
                update(|st| st.status = String::new());
                emit(Event::Message { text: e.to_string(), error: true });
            }
        }
    });
}

pub fn sign_out() {
    SYNCER.lock().unwrap().take();
    if let Some(acc) = saved_account() {
        app().rt.spawn(async move { babel_sync::sign_out(&acc).await });
    }
    let _ = app().db(|s| s.set_kv("sync.account", None));
    update(|st| {
        st.signed_in = false;
        st.status = String::new();
        st.syncing = false;
    });
}

pub fn sync_now() {
    if let Some(s) = SYNCER.lock().unwrap().as_ref() {
        s.sync_now();
    }
}
