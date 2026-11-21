//! The text view: rows of segments, typing in them, and the actions on them.

use babel_editor::{Editor, Key};
use babel_pages::Parts;
use serde_json::{Value, json};

fn editor() -> Editor {
    let dir = env!("CARGO_MANIFEST_DIR");
    let original = std::fs::read(format!("{dir}/../../test/fixtures/sample.docx")).unwrap();
    let parts: Parts = serde_json::from_str(include_str!("../../babel-pages/tests/sample-parts.json")).unwrap();
    Editor::open(original, parts, Parts::new(), &json!({ "id": "p", "targetLang": "sv-SE", "confirmed": [], "ignored": [] })).unwrap()
}

/// A row's text as the view shows it (chips as their characters).
fn shown(spans: &Value) -> String {
    spans.as_array().unwrap().iter().map(|s| s["t"].as_str().unwrap()).collect()
}

fn row<'a>(rows: &'a Value, pid: &str) -> &'a Value {
    rows.as_array().unwrap().iter().find(|r| r["pid"] == pid).unwrap_or_else(|| panic!("no row {pid}"))
}

fn text(e: &Editor, pid: &str) -> String {
    e.session.text_of(&e.session.locate(pid).unwrap()).unwrap().to_owned()
}

fn utf16(s: &str) -> usize {
    s.encode_utf16().count()
}

#[test]
fn rows_are_the_paragraphs_with_text_in_order() {
    let mut e = editor();
    let rows = e.rows();
    let rows_v = rows.as_array().unwrap();
    assert_eq!(rows_v[0]["pid"], "document:0");
    assert_eq!(rows_v[0]["n"], 1);
    assert!(rows_v.iter().enumerate().all(|(k, r)| r["n"] == k + 1));
    // Every paragraph to translate has a row; so do those with only figures ("4.2 MSEK").
    assert!(rows_v.len() >= e.stats()["total"].as_u64().unwrap() as usize);
    assert!(rows_v.iter().any(|r| shown(&r["src"]) == "4.2 MSEK"));
    // The body first, then the header, footer and note, labelled.
    let labels: Vec<&str> = rows_v.iter().map(|r| r["part"].as_str().unwrap()).collect();
    assert!(labels.contains(&"Header") && labels.contains(&"Footer") && labels.contains(&"Footnote"), "{labels:?}");
    assert!(rows_v.iter().all(|r| r["editable"] == true), "every row can be edited");
    // Footnote mark and date field: chip ① on both sides.
    let f = row(&rows, "document:10");
    assert_eq!(shown(&f["src"]), "A footnote follows this sentence.① And the paragraph continues afterwards.");
    assert_eq!(shown(&f["tgt"]), shown(&f["src"]));
    assert_eq!(shown(&row(&rows, "document:12")["tgt"]), "Printed: ①");
    // A link, bold and italic.
    let l = row(&rows, "document:3");
    assert!(l["tgt"].as_array().unwrap().iter().any(|s| s["t"] == "the project website" && s["l"] == true), "{l}");
    assert!(l["src"].as_array().unwrap().iter().any(|s| s["t"] == "the project website" && s["l"] == true), "{l}");
    let b = row(&rows, "document:2");
    assert!(b["tgt"].as_array().unwrap().iter().any(|s| s["t"] == "confidential" && s["b"] == true));
    assert!(b["src"].as_array().unwrap().iter().any(|s| s["t"] == "board of directors" && s["i"] == true));
    // A heading bold by its style is bold on both sides.
    let h = row(&rows, "document:15");
    assert!(h["src"][0]["b"] == true && h["tgt"][0]["b"] == true, "{h}");
}

#[test]
fn typing_in_a_row_edits_the_translation() {
    let mut e = editor();
    e.rows();
    let v = e.text_edit("document:2", 0, 4, "Denna");
    assert!(v["error"].is_null(), "{v}");
    assert!(text(&e, "document:2").starts_with("Denna report is"));
    assert_eq!(v["textCaret"], json!({ "pid": "document:2", "at": 5, "anchor": 5 }));
    // The row is reported changed, edited now.
    let ch = e.take_row_changes();
    assert_eq!(ch["all"], false);
    assert_eq!(ch["rows"][0]["pid"], "document:2");
    assert_eq!(ch["rows"][0]["edited"], true);
    // After a chip: positions count the chip as one character.
    let before = "A footnote follows this sentence.①";
    let v = e.text_edit("document:10", utf16(before), utf16(before), " (se nedan)");
    assert!(v["error"].is_null(), "{v}");
    let t = text(&e, "document:10");
    assert!(t.contains("sentence.") && t.contains(" (se nedan) And the paragraph"), "{t:?}");
    let shown_now = shown(&e.take_row_changes()["rows"][0]["tgt"]);
    assert_eq!(shown_now, "A footnote follows this sentence.① (se nedan) And the paragraph continues afterwards.");
    // Non-BMP text (two UTF-16 units) and a line break.
    e.text_edit("document:18", 9, 9, " 🚆\u{2028}Tåg");
    assert_eq!(text(&e, "document:18"), "Transport 🚆\nTåg");
}

#[test]
fn a_chip_cant_be_typed_over() {
    let mut e = editor();
    e.rows();
    let at = utf16("A footnote follows this sentence.");
    let before = text(&e, "document:10");
    let v = e.text_edit("document:10", at, at + 1, "");
    assert!(v["error"].is_string(), "{v}");
    assert_eq!(text(&e, "document:10"), before);
    // The row is to be shown again (the view had already removed it).
    assert_eq!(e.take_row_changes()["rows"][0]["pid"], "document:10");
}

#[test]
fn a_selection_takes_the_keys_and_menu_actions() {
    let mut e = editor();
    e.rows();
    // Bold over "report".
    let v = e.text_select("document:2", 5, 11);
    assert_eq!(v["textCaret"], json!({ "pid": "document:2", "at": 11, "anchor": 5 }));
    assert_eq!(e.selected_text(), "report");
    e.key(Key::Toggle(babel_pages::edit::Style::Bold), false);
    let r = &e.take_row_changes()["rows"][0];
    assert!(r["tgt"].as_array().unwrap().iter().any(|s| s["t"] == "report" && s["b"] == true), "{r}");
    // Undo in the view: the row comes back as it was.
    e.text_edit("document:2", 0, 4, "Den");
    e.take_row_changes();
    e.key(Key::Undo, false);
    assert_eq!(e.take_row_changes()["rows"][0]["pid"], "document:2");
    assert!(text(&e, "document:2").starts_with("This"));
    // Done (Ctrl+Enter): the row is marked, and the caret goes on to the next.
    e.text_select("document:2", 0, 0);
    let v = e.key(Key::Done, false);
    let ch = e.take_row_changes();
    assert!(ch["rows"].as_array().unwrap().iter().any(|r| r["pid"] == "document:2" && r["done"] == true), "{ch}");
    assert_eq!(v["textCaret"]["pid"], "document:3");
}

#[test]
fn claude_in_the_view() {
    let mut e = editor();
    e.rows();
    let md = "## 1\n> This report is **confidential** and intended for the *board of directors* only. It summarises ***key*** findings from 2025.\n\nDenna rapport är **konfidentiell**.\n\n## 2\n> A footnote follows this sentence.{1} And the paragraph continues afterwards.\n\nEn fotnot.{1} Och mer.\n";
    e.attach_segments(md);
    let ch = e.take_row_changes();
    assert_eq!(ch["all"], true, "attaching changes every row's Claude flag");
    assert_eq!(row(&ch["rows"], "document:2")["claude"], true);
    assert_eq!(shown(&e.claude_spans("document:10")), "En fotnot.① Och mer.");
    assert!(e.claude_spans("document:0").is_null());
    e.text_select("document:10", 3, 3);
    let v = e.insert_claude();
    assert!(v["error"].is_null(), "{v}");
    assert_eq!(shown(&e.take_row_changes()["rows"][0]["tgt"]), "En fotnot.① Och mer.");
}

#[test]
fn rows_of_a_large_document_come_quickly() {
    let path = format!("{}/../../../babel-corpus/poi/test-data/document/bug65649.docx", env!("CARGO_MANIFEST_DIR"));
    let Ok(bytes) = std::fs::read(&path) else { return };
    let imp = babel_docx::package::import(&bytes).unwrap();
    let mut e = Editor::open(bytes, imp.parts, Parts::new(), &json!({ "id": "p" })).unwrap();
    let t = std::time::Instant::now();
    let rows = e.rows();
    let took = t.elapsed();
    eprintln!("{} rows in {took:?}", rows.as_array().unwrap().len());
    assert!(took.as_secs_f32() < if cfg!(debug_assertions) { 60.0 } else { 5.0 }, "{took:?}");
}

#[test]
fn listing_rows_changes_nothing() {
    let mut e = editor();
    let before = e.stats();
    e.rows();
    assert_eq!(e.stats(), before, "rows()");
    e.text_select("document:0", 0, 0);
    assert_eq!(e.stats(), before, "text_select");
    e.take_row_changes();
    assert_eq!(e.stats(), before);
}

#[test]
fn picked_rows_take_claude_done_and_revert_together() {
    let mut e = editor();
    e.rows();
    let md = "## 1\n> This report is **confidential** and intended for the *board of directors* only. It summarises ***key*** findings from 2025.\n\nDenna rapport är **konfidentiell**.\n\n## 2\n> A footnote follows this sentence.{1} And the paragraph continues afterwards.\n\nEn fotnot.{1} Och mer.\n";
    e.attach_segments(md);
    e.take_row_changes();
    let (t2, t10) = (text(&e, "document:2"), text(&e, "document:10"));
    let pids: Vec<String> = ["document:2", "document:10", "document:0"].map(String::from).to_vec();
    let v = e.insert_claude_rows(&pids);
    assert!(v["error"].is_null(), "{v}");
    let msg = v["message"].as_str().unwrap();
    assert!(msg.contains("2 of 3") && msg.contains("1 have none"), "{msg}");
    assert_eq!(text(&e, "document:2"), "Denna rapport är konfidentiell.");
    assert_eq!(shown(&row(&e.rows(), "document:10")["tgt"]), "En fotnot.① Och mer.");
    // The caret at the first of them.
    assert_eq!(v["textCaret"]["pid"], "document:2");
    // One undo takes back both, one redo puts both back.
    e.key(Key::Undo, false);
    assert_eq!((text(&e, "document:2"), text(&e, "document:10")), (t2.clone(), t10.clone()));
    e.key(Key::Redo, false);
    assert_eq!(text(&e, "document:2"), "Denna rapport är konfidentiell.");
    assert!(text(&e, "document:10").starts_with("En fotnot."));
    e.key(Key::Undo, false);
    assert_eq!((text(&e, "document:2"), text(&e, "document:10")), (t2.clone(), t10.clone()));
    e.key(Key::Redo, false);

    // Done, and not done.
    e.take_row_changes();
    e.set_done_rows(&pids, true);
    let ch = e.take_row_changes();
    assert_eq!(ch["rows"].as_array().unwrap().len(), 3, "{ch}");
    assert!(ch["rows"].as_array().unwrap().iter().all(|r| r["done"] == true));
    e.set_done_rows(&pids[..1], false);
    assert_eq!(e.take_row_changes()["rows"][0]["done"], false);

    // Revert both, as one step.
    let v = e.revert_rows(&pids[..2]);
    assert!(v["error"].is_null(), "{v}");
    assert_eq!((text(&e, "document:2"), text(&e, "document:10")), (t2.clone(), t10.clone()));
    e.key(Key::Undo, false);
    assert_eq!(text(&e, "document:2"), "Denna rapport är konfidentiell.");
    assert!(text(&e, "document:10").starts_with("En fotnot."));
}
