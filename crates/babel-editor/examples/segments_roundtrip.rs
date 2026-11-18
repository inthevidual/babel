//! Round trip over a .docx: each paragraph's own markup as its "translation"
//! must insert back to the same text, with its marks where they were.
use babel_editor::{segments, Editor};
use babel_pages::Parts;
use serde_json::json;
fn main() {
    let path = std::env::args().nth(1).expect("a .docx");
    let bytes = std::fs::read(&path).unwrap();
    let Ok(imp) = babel_docx::package::import(&bytes) else { println!("SKIP import"); return };
    let project = json!({ "id": "p", "targetLang": "sv-SE", "confirmed": [], "ignored": [] });
    let Ok(mut e) = Editor::open(bytes, imp.parts.clone(), Parts::new(), &project) else { println!("SKIP open"); return };
    let md = e.export_segments("x.docx", "sv-SE");
    // The translation of each segment: its own source.
    let filled: String = md.split("\n## ").enumerate().map(|(k, seg)| {
        if k == 0 { return seg.to_owned(); }
        let quote: Vec<&str> = seg.lines().take_while(|l| !l.is_empty()).collect();
        let src: Vec<&str> = quote[1..].iter().map(|l| l.strip_prefix("> ").unwrap_or(l)).collect();
        format!("{}\n\n{}\n", quote.join("\n"), src.join("\n"))
    }).collect::<Vec<_>>().join("\n## ");
    let v = e.attach_segments(&filled);
    let (matched, total) = (v["claude"]["matched"].as_u64().unwrap_or(0), v["claude"]["total"].as_u64().unwrap_or(0));
    let (mut same, mut changed, mut notes, mut errors, mut unreachable) = (0, 0, 0, 0, 0);
    for (pid, _) in segments::of_parts(&imp.parts, &imp.package.text_parts) {
        let Some(p) = e.session.locate(&pid) else { unreachable += 1; continue };
        let before = e.session.text_of(&p).unwrap_or("").to_owned();
        let Some(c) = e.session.caret(&p, 0) else { unreachable += 1; continue };
        e.click(c.page, c.x + 0.05, c.top + c.height / 2.0, false);
        if e.state()["paragraph"]["pid"] != json!(pid) { unreachable += 1; continue; }
        let r = e.insert_claude();
        if let Some(err) = r["error"].as_str() { errors += 1; if errors <= 3 { println!("  error {pid}: {err}"); } continue; }
        if r["message"].is_string() { notes += 1; if notes <= 3 { println!("  note {pid}: {}", r["message"]); } }
        let after = e.session.text_of(&e.session.locate(&pid).unwrap()).unwrap_or("").to_owned();
        if after == before { same += 1 } else { changed += 1; if changed <= 3 { println!("  changed {pid}: {before:?} -> {after:?}"); } }
    }
    println!("matched {matched}/{total} same {same} changed {changed} notes {notes} errors {errors} unreachable {unreachable}");
}
