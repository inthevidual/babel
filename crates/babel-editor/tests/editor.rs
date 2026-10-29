//! The shared editor on the sample: done marks, reverting, links, the
//! original's menu, statistics, and runs from an earlier session.

use babel_editor::{lookup_term, Editor, Key};
use babel_pages::Parts;
use serde_json::{json, Value};

fn fixture() -> (Vec<u8>, Parts) {
    let dir = env!("CARGO_MANIFEST_DIR");
    let original = std::fs::read(format!("{dir}/../../test/fixtures/sample.docx")).unwrap();
    let parts: Parts = serde_json::from_str(include_str!("../../babel-pages/tests/sample-parts.json")).unwrap();
    (original, parts)
}

fn project() -> Value {
    json!({ "id": "p", "targetLang": "sv-SE", "confirmed": [], "ignored": [] })
}

fn editor() -> Editor {
    let (original, source) = fixture();
    Editor::open(original, source, Parts::new(), &project()).unwrap()
}

/// Puts the caret at `off` in paragraph `pid` (through a click at its caret position).
fn caret_at(e: &mut Editor, pid: &str, off: usize) -> Value {
    let p = e.session.locate(pid).unwrap();
    let pos = wordcraft_doc::Pos::new(p.story, p.path.clone(), off);
    let c = e.session.caret(&pos, 0).unwrap();
    e.click(c.page, c.x + 0.1, c.top + c.height / 2.0, false)
}

fn para_xml(e: &Editor, pid: &str) -> String {
    let xml = &e.session.parts["word/document.xml"];
    xml[babel_docx::para::find_paragraph(xml, pid).unwrap()].to_owned()
}

#[test]
fn statistics_match_the_web_editor() {
    let mut e = editor();
    let s = e.stats();
    // The web editor's status bar for the sample: "5,534 characters with spaces", 46 paragraphs.
    assert_eq!(s["srcChars"], 5534);
    assert_eq!(s["tgtChars"], 5534);
    assert_eq!(s["done"], 0);
    assert_eq!(s["srcPages"], 4);
    caret_at(&mut e, "document:2", 0);
    e.key(Key::Text("Denna ".into()), false);
    assert_eq!(e.stats()["tgtChars"], 5540);
    assert_eq!(e.stats()["done"], 1, "an edited paragraph counts as done");
}

#[test]
fn ctrl_enter_marks_done_and_moves_on() {
    let mut e = editor();
    caret_at(&mut e, "document:2", 3);
    let v = e.key(Key::Done, false);
    assert!(e.confirmed.contains("document:2"));
    assert!(e.meta_dirty);
    assert_ne!(v["paragraph"]["pid"], "document:2", "the caret went on to the next paragraph");
    assert_eq!(e.project_meta(&project())["confirmed"], json!(["document:2"]));
    let marks = e.marks();
    assert!(marks.as_array().unwrap().iter().any(|m| m["kind"] == "done"));
    // Ctrl+Enter on a done paragraph takes the mark away.
    caret_at(&mut e, "document:2", 0);
    e.key(Key::Done, false);
    assert!(!e.confirmed.contains("document:2"));
}

#[test]
fn revert_puts_the_original_back_and_can_be_undone() {
    let mut e = editor();
    let original = para_xml(&e, "document:2");
    caret_at(&mut e, "document:2", 0);
    e.key(Key::Text("Ändrad ".into()), false);
    let edited = para_xml(&e, "document:2");
    assert_ne!(edited, original);
    let v = e.revert_paragraph();
    assert!(v["error"].is_null(), "{v}");
    assert_eq!(para_xml(&e, "document:2"), original, "the original's w:p, exactly");
    assert_eq!(v["paragraph"]["edited"], false);
    e.key(Key::Undo, false);
    assert_eq!(para_xml(&e, "document:2"), edited);
    assert_eq!(e.original_text().unwrap().split(' ').next(), Some("This"));
}

#[test]
fn ctrl_k_links_the_selection_to_a_copied_address() {
    let mut e = editor();
    caret_at(&mut e, "document:2", "This ".len());
    let p = e.session.locate("document:2").unwrap();
    let end = wordcraft_doc::Pos::new(p.story, p.path.clone(), "This report".len());
    let c = e.session.caret(&end, 0).unwrap();
    e.click(c.page, c.x + 0.1, c.top + c.height / 2.0, true);
    assert_eq!(e.selected_text(), "report");
    let v = e.paste_link("https://example.org/rapport");
    assert!(v["error"].is_null(), "{v}");
    let xml = para_xml(&e, "document:2");
    assert!(xml.contains(r#"<w:hyperlink r:id="rIdBabel1" w:history="1">"#), "{xml}");
    let dirty = e.take_dirty();
    let rels = &dirty["word/_rels/document.xml.rels"];
    assert!(rels.contains(r#"Id="rIdBabel1""#) && rels.contains("https://example.org/rapport"));
    // The text is unchanged and still editable after the change of structure.
    assert!(e.session.text_of(&p).unwrap().starts_with("This report is "));
    e.key(Key::Text("X".into()), false);
    // Undo takes the typing back, then the link.
    e.key(Key::Undo, false);
    e.key(Key::Undo, false);
    assert!(!para_xml(&e, "document:2").contains("hyperlink"));
}

#[test]
fn ctrl_k_without_anything_to_paste_says_so() {
    let mut e = editor();
    caret_at(&mut e, "document:2", 0);
    e.key(Key::SelectAll, false);
    let v = e.paste_link("not a link");
    assert!(v["error"].as_str().unwrap().starts_with("Nothing to paste"));
}

#[test]
fn copy_link_in_the_original_and_paste_it_with_its_style() {
    let mut e = editor();
    // "See also the project website for background material." — the link in the original.
    let pid = (0..40).map(|i| format!("document:{i}")).find(|id| e.session.locate(id).is_some_and(|p| e.session.text_of(&p).unwrap_or("").starts_with("See also"))).unwrap();
    // Its position in the original: the same paragraph, found by its id in the source layout.
    let off = "See also the pro".len();
    let p = e.session.locate(&pid).unwrap();
    let c = e.session.caret(&wordcraft_doc::Pos::new(p.story, p.path.clone(), off), 0).unwrap();
    let menu = e.source_menu(c.page, c.x, c.top + c.height / 2.0);
    assert_eq!(menu["link"], "https://example.org/", "{menu}");
    assert!(menu["paragraph"].as_str().unwrap().starts_with("See also"));
    assert_eq!(e.copy_source_link().as_deref(), Some("https://example.org/"));
    caret_at(&mut e, "document:2", 0);
    e.key(Key::SelectAll, false);
    let v = e.paste_link("");
    assert!(v["error"].is_null(), "{v}");
    let xml = para_xml(&e, "document:2");
    assert!(xml.contains("<w:hyperlink"), "{xml}");
    // The original link's look comes with it.
    assert!(xml.contains("w:rStyle") || xml.contains(r#"<w:color w:val="0563C1"/>"#), "{xml}");
}

#[test]
fn a_click_in_the_original_goes_to_the_translation() {
    let mut e = editor();
    let p = e.session.locate("document:4").unwrap();
    let c = e.session.caret(&p, 0).unwrap();
    let v = e.source_click(c.page, c.x + 20.0, c.top + c.height / 2.0, false);
    assert_eq!(v["paragraph"]["pid"], "document:4");
    let w = e.source_word(c.page, c.x + 5.0, c.top + c.height / 2.0);
    assert!(!w["sourceSelection"].as_array().unwrap().is_empty());
    assert_eq!(e.source_selected_text(), "Item");
}

#[test]
fn runs_typed_in_an_earlier_session_stay_editable() {
    let (original, source) = fixture();
    let mut e = Editor::open(original.clone(), source.clone(), Parts::new(), &project()).unwrap();
    caret_at(&mut e, "document:2", "This ".len());
    e.key(Key::Right, true);
    e.key(Key::Right, true);
    e.key(Key::Toggle(babel_pages::edit::Style::Bold), false);
    let saved = e.take_dirty();
    assert!(saved["word/document.xml"].contains("data-bgen"), "bold made generated runs");
    // Next session: they get ids and can be typed into.
    let mut e2 = Editor::open(original, source, saved, &project()).unwrap();
    let again = e2.take_dirty();
    assert!(!again["word/document.xml"].contains("data-bgen"), "adopted, and saved so");
    caret_at(&mut e2, "document:2", "This r".len());
    let v = e2.key(Key::Text("Z".into()), false);
    assert!(v["error"].is_null(), "{v}");
    assert!(e2.session.text_of(&e2.session.locate("document:2").unwrap()).unwrap().starts_with("This rZe"));
}

#[test]
fn look_up_terms() {
    assert_eq!(lookup_term("  board of directors. ").as_deref(), Some("board of directors"));
    assert_eq!(lookup_term("one two three four five six seven"), None);
    assert_eq!(lookup_term("…"), None);
}
