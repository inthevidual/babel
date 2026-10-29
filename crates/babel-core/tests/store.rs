use babel_core::{Error, Parts, Store, Version};
use serde_json::json;

const DOC: &str = "word/document.xml";

fn doc(texts: &[&str]) -> String {
    let body: String = texts
        .iter()
        .enumerate()
        .map(|(i, t)| format!(r#"<w:p data-bp="document:{i}"><w:r><w:t>{t}</w:t></w:r></w:p>"#))
        .collect();
    format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
    )
}

fn parts(texts: &[&str]) -> Parts {
    Parts::from([(DOC.to_owned(), doc(texts))])
}

fn setup() -> (Store, babel_core::User) {
    let mut s = Store::open_in_memory().unwrap();
    let u = s.create_user("ada", "correct horse").unwrap();
    let meta = json!({ "id": "p1", "name": "Report", "updatedAt": 1 });
    s.create_project(&u, &meta, b"PK fake docx", &parts(&["One", "Two", "Three"]))
        .unwrap();
    (s, u)
}

#[test]
fn users_and_sessions() {
    let (s, u) = setup();
    assert!(matches!(
        s.verify_user("ada", "wrong password"),
        Err(Error::Auth)
    ));
    assert!(matches!(
        s.verify_user("nobody", "correct horse"),
        Err(Error::Auth)
    ));
    assert_eq!(s.verify_user("ADA", "correct horse").unwrap().id, u.id);
    let t = s.create_session(&u, "test").unwrap();
    assert_eq!(s.session_user(&t).unwrap().unwrap().name, "ada");
    s.set_password("ada", "battery staple").unwrap();
    assert!(
        s.session_user(&t).unwrap().is_none(),
        "a new password signs everyone out"
    );
    assert!(matches!(
        s.create_user("Ada", "whatever123"),
        Err(Error::Conflict(_))
    ));
}

#[test]
fn projects_belong_to_their_user() {
    let (mut s, _) = setup();
    let eve = s.create_user("eve", "password123").unwrap();
    assert!(s.list_projects(&eve).unwrap().is_empty());
    assert!(matches!(s.parts(&eve, "p1"), Err(Error::NotFound)));
    let meta = json!({ "id": "p1", "name": "Mine now" });
    assert!(matches!(
        s.create_project(&eve, &meta, b"x", &Parts::new()),
        Err(Error::Conflict(_))
    ));
}

#[test]
fn create_is_idempotent() {
    let (mut s, u) = setup();
    let meta = json!({ "id": "p1", "name": "Report" });
    s.create_project(&u, &meta, b"PK fake docx", &parts(&["One", "Two", "Three"]))
        .unwrap();
    assert_eq!(s.source_docx(&u, "p1").unwrap(), b"PK fake docx");
}

#[test]
fn saves_from_the_head_do_not_merge() {
    let (mut s, u) = setup();
    let r = s
        .save_parts(&u, "p1", 0, None, &parts(&["Ett", "Two", "Three"]), None)
        .unwrap();
    assert_eq!((r.rev, r.merged), (1, false));
    let (rev, p) = s.parts(&u, "p1").unwrap();
    assert_eq!(rev, 1);
    assert_eq!(p[DOC], doc(&["Ett", "Two", "Three"]));
}

#[test]
fn concurrent_saves_merge_by_paragraph() {
    let (mut s, u) = setup();
    // Two devices start from revision 0.
    s.save_parts(
        &u,
        "p1",
        0,
        None,
        &parts(&["Ett", "Two", "Three"]),
        Some("a"),
    )
    .unwrap();
    let r = s
        .save_parts(&u, "p1", 0, None, &parts(&["One", "Two", "Tre"]), Some("b"))
        .unwrap();
    assert!(r.merged);
    assert!(r.conflicts.is_empty());
    assert_eq!(r.parts.unwrap()[DOC], doc(&["Ett", "Two", "Tre"]));
    assert!(s.list_versions(&u, "p1").unwrap().is_empty());
}

#[test]
fn conflicting_paragraph_keeps_the_loser_as_a_version() {
    let (mut s, u) = setup();
    s.save_parts(&u, "p1", 0, None, &parts(&["Ett", "Two", "Three"]), None)
        .unwrap();
    let r = s
        .save_parts(&u, "p1", 0, None, &parts(&["Uno", "Two", "Three"]), None)
        .unwrap();
    assert_eq!(r.conflicts, vec![format!("{DOC}: document:0")]);
    assert_eq!(
        s.parts(&u, "p1").unwrap().1[DOC],
        doc(&["Uno", "Two", "Three"])
    );
    let vs = s.list_versions(&u, "p1").unwrap();
    assert_eq!(vs.len(), 1);
    let (_, v) = s.get_version(&u, &vs[0].sid).unwrap();
    assert_eq!(v.parts.unwrap()[DOC], doc(&["Ett", "Two", "Three"]));
}

#[test]
fn untranslated_parts_merge_against_the_source() {
    let (mut s, u) = setup();
    // Neither save had stored parts before: the base is the source.
    s.save_parts(&u, "p1", 0, None, &parts(&["One", "Två", "Three"]), None)
        .unwrap();
    let r = s
        .save_parts(&u, "p1", 0, None, &parts(&["One", "Two", "Tre"]), None)
        .unwrap();
    assert!(r.conflicts.is_empty());
    assert_eq!(
        s.parts(&u, "p1").unwrap().1[DOC],
        doc(&["One", "Två", "Tre"])
    );
}

#[test]
fn versions_prune_automatic_ones_only() {
    let (mut s, u) = setup();
    let v = |reason: &str, time| Version {
        sid: String::new(),
        time,
        reason: reason.into(),
        stats: json!({}),
        parts: Some(parts(&["x"])),
    };
    s.add_version(&u, "p1", &v("named", 0)).unwrap();
    for t in 1..=70 {
        s.add_version(&u, "p1", &v("auto", t)).unwrap();
    }
    let vs = s.list_versions(&u, "p1").unwrap();
    assert_eq!(vs.len(), 60);
    assert!(vs.iter().any(|v| v.reason == "named"));
    // Re-adding a version by its id is a no-op.
    let mut same = v("auto", 99);
    same.sid = vs[0].sid.clone();
    s.add_version(&u, "p1", &same).unwrap();
    assert_eq!(s.list_versions(&u, "p1").unwrap().len(), 60);
}

#[test]
fn delete_leaves_a_tombstone() {
    let (mut s, u) = setup();
    s.save_parts(&u, "p1", 0, None, &parts(&["Ett", "Two", "Three"]), None)
        .unwrap();
    s.delete_project(&u, "p1").unwrap();
    assert!(s.list_projects(&u).unwrap().is_empty());
    let sums = s.summaries(&u).unwrap();
    assert!(sums.len() == 1 && sums[0].deleted);
    assert!(matches!(s.parts(&u, "p1"), Err(Error::NotFound)));
}
