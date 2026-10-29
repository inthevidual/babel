use babel_core::{store::Parts, Store, User, Version};
use babel_server::{AppState, Mode};
use babel_sync::{Account, Engine};
use serde_json::json;

const DOC: &str = "word/document.xml";

fn doc(texts: &[&str]) -> String {
    let body: String = texts
        .iter()
        .enumerate()
        .map(|(i, t)| format!(r#"<w:p data-bp="document:{i}"><w:r><w:t>{t}</w:t></w:r></w:p>"#))
        .collect();
    format!(r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#)
}

fn parts(texts: &[&str]) -> Parts {
    Parts::from([(DOC.to_owned(), doc(texts))])
}

struct Side {
    state: AppState,
    user: User,
}

impl Side {
    fn store<T>(&self, f: impl FnOnce(&mut Store, &User) -> T) -> T {
        f(&mut self.state.store.lock().unwrap(), &self.user)
    }
    fn text(&self, id: &str) -> String {
        self.store(|s, u| s.parts(u, id).unwrap().1.get(DOC).cloned().unwrap_or_default())
    }
    fn create(&self, id: &str) {
        self.store(|s, u| {
            s.create_project(u, &json!({ "id": id, "name": id, "updatedAt": 1 }), b"PK docx", &parts(&["One", "Two", "Three"]))
                .unwrap()
        });
    }
    fn save(&self, id: &str, texts: &[&str]) {
        self.store(|s, u| {
            let (rev, _) = s.parts(u, id).unwrap();
            s.save_parts(u, id, rev, None, &parts(texts), None).unwrap();
        });
    }
}

async fn setup() -> (Side, Side, Engine) {
    let server = AppState::new(Store::open_in_memory().unwrap(), Mode::Server);
    let suser = server.store.lock().unwrap().create_user("ada", "correct horse").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = babel_server::router(server.clone());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let local = AppState::new(Store::open_in_memory().unwrap(), Mode::Desktop);
    let luser = local.store.lock().unwrap().create_user("local", "whatever-long").unwrap();

    let account: Account = babel_sync::sign_in(&format!("http://{addr}"), "ada", "correct horse", "test").await.unwrap();
    assert_eq!(account.user, "ada");
    let engine = Engine::new(local.clone(), luser.clone(), &account);
    (Side { state: server, user: suser }, Side { state: local, user: luser }, engine)
}

#[tokio::test]
async fn wrong_password_is_reported() {
    let server = AppState::new(Store::open_in_memory().unwrap(), Mode::Server);
    server.store.lock().unwrap().create_user("ada", "correct horse").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, babel_server::router(server)).await.unwrap() });
    let e = babel_sync::sign_in(&format!("http://{addr}"), "ada", "nope nope", "t").await.unwrap_err();
    assert!(e.to_string().contains("Wrong"), "{e}");
}

#[tokio::test]
async fn local_work_is_uploaded_and_server_work_downloaded() {
    let (server, local, engine) = setup().await;
    local.create("mine");
    local.save("mine", &["Ett", "Two", "Three"]);
    server.create("theirs");
    server.save("theirs", &["One", "Två", "Three"]);

    let r = engine.run_once().await.unwrap();
    assert_eq!((r.uploaded, r.downloaded), (1, 1));
    assert_eq!(server.text("mine"), doc(&["Ett", "Two", "Three"]));
    assert_eq!(local.text("theirs"), doc(&["One", "Två", "Three"]));
    assert_eq!(local.store(|s, u| s.source_docx(u, "theirs").unwrap()), b"PK docx");

    // Nothing changed: a second round does nothing.
    assert!(!engine.run_once().await.unwrap().changed());
}

#[tokio::test]
async fn pushes_pulls_and_merges() {
    let (server, local, engine) = setup().await;
    local.create("p");
    engine.run_once().await.unwrap();

    local.save("p", &["Ett", "Two", "Three"]);
    assert_eq!(engine.run_once().await.unwrap().pushed, 1);
    assert_eq!(server.text("p"), doc(&["Ett", "Two", "Three"]));

    server.save("p", &["Ett", "Två", "Three"]);
    assert_eq!(engine.run_once().await.unwrap().pulled, 1);
    assert_eq!(local.text("p"), doc(&["Ett", "Två", "Three"]));

    // Edited on both sides between rounds, different paragraphs.
    local.save("p", &["Ett", "Två", "Tre"]);
    server.save("p", &["Uno", "Två", "Three"]);
    let r = engine.run_once().await.unwrap();
    assert_eq!((r.pushed, r.merged), (1, 1));
    let want = doc(&["Uno", "Två", "Tre"]);
    assert_eq!(server.text("p"), want);
    assert_eq!(local.text("p"), want);
    assert!(!engine.run_once().await.unwrap().changed(), "settled after the merge");
}

#[tokio::test]
async fn deletions_travel_both_ways() {
    let (server, local, engine) = setup().await;
    local.create("a");
    local.create("b");
    engine.run_once().await.unwrap();

    local.store(|s, u| s.delete_project(u, "a").unwrap());
    server.store(|s, u| s.delete_project(u, "b").unwrap());
    let r = engine.run_once().await.unwrap();
    assert_eq!(r.deleted, 2);
    assert!(server.store(|s, u| s.list_projects(u).unwrap()).is_empty());
    assert!(local.store(|s, u| s.summaries(u).unwrap()).is_empty(), "local tombstones are forgotten");
    assert!(!engine.run_once().await.unwrap().changed());
}

#[tokio::test]
async fn versions_are_shared() {
    let (server, local, engine) = setup().await;
    local.create("p");
    engine.run_once().await.unwrap();
    let v = |reason: &str| Version { sid: String::new(), time: 5, reason: reason.into(), stats: json!({}), parts: Some(parts(&["x"])) };
    local.store(|s, u| s.add_version(u, "p", &v("from here")).unwrap());
    server.store(|s, u| s.add_version(u, "p", &v("from there")).unwrap());
    assert_eq!(engine.run_once().await.unwrap().versions, 2);
    let reasons = |side: &Side| {
        let mut r: Vec<String> = side.store(|s, u| s.list_versions(u, "p").unwrap()).into_iter().map(|v| v.reason).collect();
        r.sort();
        r
    };
    assert_eq!(reasons(&local), ["from here", "from there"]);
    assert_eq!(reasons(&server), ["from here", "from there"]);
    assert!(!engine.run_once().await.unwrap().changed());
}

#[tokio::test]
async fn metadata_follows_the_latest_change() {
    let (server, local, engine) = setup().await;
    local.create("p");
    engine.run_once().await.unwrap();
    server.store(|s, u| s.save_meta(u, "p", &json!({ "id": "p", "name": "Renamed there", "updatedAt": 50 })).unwrap());
    engine.run_once().await.unwrap();
    assert_eq!(local.store(|s, u| s.get_project(u, "p").unwrap())["name"], "Renamed there");
}
