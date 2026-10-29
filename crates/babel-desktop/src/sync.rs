//! The account: signing in to a Babel server and syncing with it in the
//! background (babel-sync), reflected in the Backend's properties.

use std::pin::Pin;
use std::sync::Mutex;
use std::time::Duration;

use babel_sync::{Account, Status, Syncer};
use cxx_qt::{CxxQtThread, Threading};
use cxx_qt_lib::QString;

use crate::app::app;
use crate::bridge::Backend;

/// How often to look for edits made on other devices.
const INTERVAL: Duration = Duration::from_secs(20);

static SYNCER: Mutex<Option<Syncer>> = Mutex::new(None);

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

fn start(account: &Account, qt: CxxQtThread<Backend>) {
    let a = app();
    let _guard = a.rt.enter();
    let syncer = Syncer::start(a.state.clone(), a.user.clone(), account, INTERVAL);
    let mut status = syncer.status();
    a.rt.spawn(async move {
        loop {
            let s = status.borrow_and_update().clone();
            let text = QString::from(&status_text(&s));
            let ok = qt
                .queue(move |mut o| {
                    o.as_mut().set_syncing(s.syncing);
                    o.as_mut().set_sync_status(text);
                    if s.signed_out {
                        o.as_mut().set_signed_in(false);
                    }
                })
                .is_ok();
            if !ok || status.changed().await.is_err() {
                break;
            }
        }
    });
    *SYNCER.lock().unwrap() = Some(syncer);
}

pub fn attach(mut b: Pin<&mut Backend>) {
    if let Some(acc) = saved_account() {
        b.as_mut().set_sync_server(QString::from(&acc.server));
        b.as_mut().set_sync_user(QString::from(&acc.user));
        b.as_mut().set_signed_in(true);
        start(&acc, b.qt_thread());
    }
}

pub fn sign_in(mut b: Pin<&mut Backend>, server: String, user: String, password: String) {
    b.as_mut().set_sync_status(QString::from("Signing in…"));
    let qt = b.qt_thread();
    let host = std::fs::read_to_string("/etc/hostname").map(|h| h.trim().to_owned()).unwrap_or_default();
    let label = format!("Babel desktop on {}", if host.is_empty() { "this computer" } else { &host });
    app().rt.spawn(async move {
        match babel_sync::sign_in(&server, &user, &password, &label).await {
            Ok(acc) => {
                let raw = serde_json::to_string(&acc).expect("account serialises");
                if let Err(e) = app().db(|s| s.set_kv("sync.account", Some(&raw))) {
                    let _ = qt.queue(move |o| o.message(QString::from(&format!("Could not save the account: {e}")), true));
                    return;
                }
                let qt2 = qt.clone();
                let _ = qt.queue(move |mut o| {
                    o.as_mut().set_sync_server(QString::from(&acc.server));
                    o.as_mut().set_sync_user(QString::from(&acc.user));
                    o.as_mut().set_signed_in(true);
                    o.as_mut().message(QString::from(&format!("Signed in as {}. Your translations sync now.", acc.user)), false);
                    start(&acc, qt2);
                });
            }
            Err(e) => {
                let text = QString::from(&e.to_string());
                let _ = qt.queue(move |mut o| {
                    o.as_mut().set_sync_status(QString::default());
                    o.as_mut().message(text, true);
                });
            }
        }
    });
}

pub fn sign_out(mut b: Pin<&mut Backend>) {
    SYNCER.lock().unwrap().take();
    if let Some(acc) = saved_account() {
        app().rt.spawn(async move { babel_sync::sign_out(&acc).await });
    }
    let _ = app().db(|s| s.set_kv("sync.account", None));
    b.as_mut().set_signed_in(false);
    b.as_mut().set_sync_status(QString::default());
    b.as_mut().set_syncing(false);
}

pub fn sync_now() {
    if let Some(s) = SYNCER.lock().unwrap().as_ref() {
        s.sync_now();
    }
}
