//! Claude's translation as a segment file: export, attach, match, insert.

use babel_editor::segments;
use babel_editor::{Editor, Key};
use babel_pages::Parts;
use serde_json::{Value, json};

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

fn caret_at(e: &mut Editor, pid: &str) {
    let p = e.session.locate(pid).unwrap();
    let c = e.session.caret(&p, 0).unwrap();
    e.click(c.page, c.x + 0.1, c.top + c.height / 2.0, false);
}

fn para_xml(e: &Editor, pid: &str) -> String {
    let xml = &e.session.parts["word/document.xml"];
    xml[babel_docx::para::find_paragraph(xml, pid).unwrap()].to_owned()
}

fn text(e: &Editor, pid: &str) -> String {
    e.session.text_of(&e.session.locate(pid).unwrap()).unwrap().to_owned()
}

/// The exported file with the translations given (by segment id), others left empty.
fn translated(e: &Editor, with: &[(&str, &str)]) -> String {
    let md = e.export_segments("sample.docx", "sv-SE");
    md.split("\n## ")
        .enumerate()
        .map(|(k, seg)| {
            let id = seg.lines().next().and_then(|h| h.split(" · ").nth(1)).unwrap_or("");
            match with.iter().find(|(pid, _)| *pid == id) {
                Some((_, t)) if k > 0 => {
                    let quote: Vec<&str> = seg.lines().take_while(|l| !l.is_empty()).collect();
                    format!("{}\n\n{t}\n", quote.join("\n"))
                }
                _ => seg.to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n## ")
}

#[test]
fn every_paragraph_of_the_export_matches_back() {
    let mut e = editor();
    let md = e.export_segments("sample.docx", "sv-SE");
    let segs = segments::parse(&md).unwrap();
    let total = e.stats()["total"].as_u64().unwrap() as usize;
    assert_eq!(segs.len(), total, "a segment for every paragraph Babel counts");
    // Translate everything with a marker; every segment finds its paragraph.
    let filled: String = md
        .split("\n## ")
        .enumerate()
        .map(|(k, s)| if k == 0 { s.to_owned() } else { format!("{}SV\n", s.trim_end_matches('\n').to_owned() + "\n\n") })
        .collect::<Vec<_>>()
        .join("\n## ");
    let v = e.attach_segments(&filled);
    assert!(v["error"].is_null(), "{v}");
    assert_eq!(v["claude"], json!({ "matched": total, "total": total }));
}

#[test]
fn inserting_keeps_bold_and_italic_and_is_one_undo() {
    let mut e = editor();
    let md = translated(&e, &[("document:2", "Rapporten är **konfidentiell** och endast för *styrelsen*. Den sammanfattar ***viktiga*** resultat från 2025.")]);
    assert!(e.attach_segments(&md)["error"].is_null());
    let before = para_xml(&e, "document:2");
    caret_at(&mut e, "document:2");
    assert_eq!(e.state()["paragraph"]["claude"], true);
    let v = e.insert_claude();
    assert!(v["error"].is_null(), "{v}");
    assert!(v["message"].is_null(), "{v}");
    assert_eq!(text(&e, "document:2"), "Rapporten är konfidentiell och endast för styrelsen. Den sammanfattar viktiga resultat från 2025.");
    let xml = para_xml(&e, "document:2");
    let bold_run = |w: &str| xml.split("<w:r ").any(|r| r.contains(&format!(">{w}<")) && r.contains("<w:b/>"));
    let italic_run = |w: &str| xml.split("<w:r ").any(|r| r.contains(&format!(">{w}<")) && r.contains("<w:i/>"));
    assert!(bold_run("konfidentiell"), "{xml}");
    assert!(italic_run("styrelsen"), "{xml}");
    assert!(bold_run("viktiga") && italic_run("viktiga"), "{xml}");
    assert!(!bold_run("Rapporten är ") && !italic_run("Rapporten är "), "{xml}");
    e.key(Key::Undo, false);
    assert_eq!(para_xml(&e, "document:2"), before, "one undo takes it all back");
}

#[test]
fn footnote_marks_and_fields_stay_where_the_placeholders_put_them() {
    let mut e = editor();
    let md = translated(
        &e,
        &[
            ("document:10", "En fotnot följer efter denna mening.{1} Och stycket fortsätter sedan."),
            ("document:12", "Utskriven: {1}"),
        ],
    );
    assert!(e.attach_segments(&md)["error"].is_null());
    caret_at(&mut e, "document:10");
    let v = e.insert_claude();
    assert!(v["error"].is_null() && v["message"].is_null(), "{v}");
    let t = text(&e, "document:10");
    let mark = t.find(|c: char| !c.is_alphanumeric() && !" .åäöÅÄÖ".contains(c)).unwrap();
    assert_eq!(&t[..mark], "En fotnot följer efter denna mening.");
    assert!(t.ends_with(" Och stycket fortsätter sedan."), "{t:?}");
    let xml = para_xml(&e, "document:10");
    assert!(xml.contains("w:footnoteReference"), "{xml}");
    assert!(xml.find("denna mening.").unwrap() < xml.find("w:footnoteReference").unwrap());
    assert!(xml.find("w:footnoteReference").unwrap() < xml.find("Och stycket").unwrap());

    caret_at(&mut e, "document:12");
    let v = e.insert_claude();
    assert!(v["error"].is_null(), "{v}");
    assert!(text(&e, "document:12").starts_with("Utskriven: "));
    let xml = para_xml(&e, "document:12");
    assert!(xml.contains("fldChar") || xml.contains("fldSimple"), "the date field stays: {xml}");
}

#[test]
fn a_missing_placeholder_keeps_the_mark_and_says_so() {
    let mut e = editor();
    let md = translated(&e, &[("document:10", "En fotnot följer efter denna mening. Och stycket fortsätter.")]);
    e.attach_segments(&md);
    caret_at(&mut e, "document:10");
    let v = e.insert_claude();
    assert!(v["error"].is_null(), "{v}");
    assert!(v["message"].as_str().unwrap().contains("check where they ended up"), "{v}");
    assert!(v["message"].as_str().unwrap().contains("1 footnote mark(s)"), "{v}");
    assert!(para_xml(&e, "document:10").contains("w:footnoteReference"));
    assert!(text(&e, "document:10").starts_with("En fotnot följer efter denna mening. Och stycket fortsätter."));
}

#[test]
fn headings_stay_bold_line_breaks_and_tabs_come_through() {
    let mut e = editor();
    let md = translated(
        &e,
        &[
            ("document:15", "Område"),
            ("document:6", "Första adressraden\nAndra adressraden"),
            ("document:4", "Post{tab}Belopp{tab}Andel"),
        ],
    );
    e.attach_segments(&md);
    for pid in ["document:15", "document:6", "document:4"] {
        caret_at(&mut e, pid);
        let v = e.insert_claude();
        assert!(v["error"].is_null(), "{pid}: {v}");
    }
    let xml = para_xml(&e, "document:15");
    assert!(xml.contains(">Område<") && xml.contains("<w:b/>"), "{xml}");
    assert_eq!(text(&e, "document:6"), "Första adressraden\nAndra adressraden");
    assert!(para_xml(&e, "document:6").contains("<w:br/>"));
    assert_eq!(text(&e, "document:4"), "Post\tBelopp\tAndel");
    let tabs = para_xml(&e, "document:4");
    assert!(tabs.matches("<w:tab/>").count() >= 2, "{tabs}");
}

#[test]
fn link_text_goes_into_the_link() {
    let mut e = editor();
    let md = translated(&e, &[("document:3", "Se även [projektets webbplats] för bakgrundsmaterial.")]);
    assert!(md.contains("> See also [the project website] for background material."), "links are bracketed in the export");
    e.attach_segments(&md);
    caret_at(&mut e, "document:3");
    let v = e.insert_claude();
    assert!(v["error"].is_null() && v["message"].is_null(), "{v}");
    assert_eq!(text(&e, "document:3"), "Se även projektets webbplats för bakgrundsmaterial.");
    let xml = para_xml(&e, "document:3");
    let link = &xml[xml.find("<w:hyperlink").unwrap()..xml.find("</w:hyperlink>").unwrap()];
    assert!(link.contains(">projektets webbplats<"), "{xml}");
    assert!(!link.contains("Se även") && !link.contains("bakgrund"), "{xml}");
}

#[test]
fn a_link_field_gets_its_text_too() {
    let (original, mut source) = fixture();
    let xml = source["word/document.xml"].clone();
    let a = xml.find("<w:hyperlink").unwrap();
    let b = xml.find("</w:hyperlink>").unwrap() + "</w:hyperlink>".len();
    let field = r#"<w:r data-br="document:r900"><w:fldChar w:fldCharType="begin"/></w:r><w:r data-br="document:r901"><w:instrText>HYPERLINK "https://example.org/"</w:instrText></w:r><w:r data-br="document:r902"><w:fldChar w:fldCharType="separate"/></w:r><w:r data-br="document:r910"><w:t>the project website</w:t></w:r><w:r data-br="document:r903"><w:fldChar w:fldCharType="end"/></w:r>"#;
    source.insert("word/document.xml".into(), format!("{}{field}{}", &xml[..a], &xml[b..]));
    let mut e = Editor::open(original, source, Parts::new(), &project()).unwrap();
    let md = translated(&e, &[("document:3", "Läs mer på [projektets webbplats], med bakgrundsmaterial.")]);
    assert!(md.contains("> See also [the project website] for background material."), "{md}");
    e.attach_segments(&md);
    caret_at(&mut e, "document:3");
    let v = e.insert_claude();
    assert!(v["error"].is_null() && v["message"].is_null(), "{v}");
    assert_eq!(text(&e, "document:3"), "Läs mer på projektets webbplats, med bakgrundsmaterial.");
    let xml = para_xml(&e, "document:3");
    let sep = xml.find(r#"w:fldCharType="separate""#).unwrap();
    let end = xml.find(r#"w:fldCharType="end""#).unwrap();
    assert!(xml[sep..end].contains(">projektets webbplats<"), "the field's result is the link text: {xml}");
}

#[test]
fn brackets_that_dont_line_up_still_insert_and_say_so() {
    let mut e = editor();
    let md = translated(&e, &[("document:3", "Se även projektets webbplats för bakgrundsmaterial.")]);
    e.attach_segments(&md);
    caret_at(&mut e, "document:3");
    let v = e.insert_claude();
    assert!(v["error"].is_null(), "{v}");
    assert!(v["message"].as_str().unwrap().contains("check the links"), "{v}");
    assert_eq!(text(&e, "document:3"), "Se även projektets webbplats för bakgrundsmaterial.");
}

#[test]
fn a_file_without_ids_matches_by_text_in_any_order() {
    let mut e = editor();
    let md = "---\nbabel-segments: 1\n---\n\n## 1\n> Transport\n\nTransporter\n\n## 2\n> This report is confidential and intended for the board of directors only. It summarises key findings from 2025.\n\nOformaterad översättning.\n\n## 3\n> Nothing like this paragraph exists\n\nIngenting\n";
    let v = e.attach_segments(md);
    assert!(v["error"].is_null(), "{v}");
    assert_eq!(v["claude"]["matched"], 2);
    caret_at(&mut e, "document:18");
    e.insert_claude();
    assert_eq!(text(&e, "document:18"), "Transporter");
    caret_at(&mut e, "document:2");
    e.insert_claude();
    assert_eq!(text(&e, "document:2"), "Oformaterad översättning.");
}

#[test]
fn a_file_for_another_document_is_refused() {
    let mut e = editor();
    let v = e.attach_segments("## 1\n> Something else entirely\n\nNågot annat\n");
    assert!(v["error"].as_str().unwrap().contains("match"), "{v}");
    assert!(v["claude"].is_null());
    assert!(e.attach_segments("no segments here")["error"].is_string());
}

#[test]
fn the_file_is_kept_with_the_project_and_can_be_removed() {
    let (original, source) = fixture();
    let mut e = editor();
    let md = translated(&e, &[("document:18", "Transporter")]);
    e.attach_segments(&md);
    assert!(e.meta_dirty);
    let meta = e.project_meta(&project());
    assert_eq!(meta["claudeSegments"], json!(md));
    let mut e2 = Editor::open(original, source, Parts::new(), &meta).unwrap();
    caret_at(&mut e2, "document:18");
    assert_eq!(e2.state()["paragraph"]["claude"], true);
    e2.remove_segments();
    assert!(e2.project_meta(&meta).get("claudeSegments").is_none());
    let v = e2.insert_claude();
    assert!(v["error"].is_string());
}

/// The whole sample translated with the skill (test/fixtures/sample.sv-SE.babel.md,
/// made by its script from the .docx: no ids), inserted paragraph by paragraph
/// and exported.
#[test]
fn a_whole_document_translated_with_the_skill() {
    let (original, source) = fixture();
    let mut e = Editor::open(original.clone(), source, Parts::new(), &project()).unwrap();
    let md = std::fs::read_to_string(format!("{}/../../test/fixtures/sample.sv-SE.babel.md", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let v = e.attach_segments(&md);
    assert_eq!(v["claude"], json!({ "matched": 46, "total": 46 }), "{v}");
    let pids: Vec<String> = {
        let mut ids: Vec<(usize, String)> = (0..200)
            .flat_map(|k| ["document", "header1", "header2", "footer1", "footer2", "footnotes"].map(|p| format!("{p}:{k}")))
            .filter_map(|pid| e.session.locate(&pid).map(|_| (0, pid)))
            .collect();
        ids.sort();
        ids.into_iter().map(|x| x.1).collect()
    };
    let mut inserted = 0;
    for pid in &pids {
        let p = e.session.locate(pid).unwrap();
        let Some(c) = e.session.caret(&p, 0) else { continue };
        e.click(c.page, c.x + 0.05, c.top + c.height / 2.0, false);
        if e.state()["paragraph"]["pid"] != json!(pid) || e.state()["paragraph"]["claude"] != json!(true) {
            continue;
        }
        let v = e.insert_claude();
        assert!(v["error"].is_null(), "{pid}: {v}");
        assert!(v["message"].is_null(), "{pid}: {v}");
        inserted += 1;
    }
    // The header and footer are reached by clicking on the page.
    for (y, pid) in [(40.0, "header1:0"), (800.0, "footer1:0")] {
        let v = e.click(0, 400.0, y, false);
        assert_eq!(v["paragraph"]["pid"], pid);
        let v = e.insert_claude();
        assert!(v["error"].is_null() && v["message"].is_null(), "{pid}: {v}");
        inserted += 1;
    }
    let v = e.click(0, 400.0, 40.0, false);
    assert_eq!(v["paragraph"]["claude"], true);
    assert_eq!(inserted, 46, "every paragraph");
    // Every paragraph counts as done, but "Budget" and "Status", which read the same in Swedish.
    assert_eq!(e.stats()["done"], json!(inserted - 2));
    let header = e.session.parts.iter().find(|(k, _)| k.contains("header1")).map(|(_, v)| v.clone()).unwrap();
    assert!(header.contains("Myndigheten för regional utveckling"), "{header}");
    let footer = e.session.parts.iter().find(|(k, _)| k.contains("footer1")).map(|(_, v)| v.clone()).unwrap();
    assert!(footer.contains(">Sida <") || footer.contains("Sida "), "{footer}");
    assert!(footer.contains("PAGE"), "the page number field stays: {footer}");
    let link = para_xml(&e, "document:3");
    assert!(link[link.find("<w:hyperlink").unwrap()..].starts_with("<w:hyperlink") && link.contains(">projektets webbplats<"), "{link}");
    assert!(para_xml(&e, "document:10").contains("w:footnoteReference"));
    assert!(text(&e, "document:4").contains('\t'));
    // The exported document: the translation, Babel's ids gone, the fields and links kept.
    let parts = e.take_dirty();
    let out = babel_docx::package::build_docx(&original, &parts, Some("sv-SE")).unwrap();
    let doc = babel_pages::read_part(&out, "word/document.xml").unwrap();
    for t in ["Årsrapport om regional utveckling", "konfidentiell", "projektets webbplats", "Adressens andra rad", "Rekommendationen antogs."] {
        assert!(doc.contains(t), "{t} in the export");
    }
    assert!(!doc.contains("This report is") && !doc.contains("data-b"));
    assert!(doc.contains("w:footnoteReference") && doc.contains("<w:hyperlink"));
}

/// The skill's script and Babel make the same segments from the same .docx
/// (the script has no ids; everything else is alike). Needs python3.
#[test]
fn the_skill_script_makes_the_same_segments() {
    let root = format!("{}/../..", env!("CARGO_MANIFEST_DIR"));
    let out = std::process::Command::new("python3")
        .args(["-I", &format!("{root}/skills/babel-translate/scripts/babel_segments.py"), "extract", &format!("{root}/test/fixtures/sample.docx"), "--lang", "sv-SE"])
        .output();
    let Ok(out) = out else {
        eprintln!("python3 not found: skipped");
        return;
    };
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let script = String::from_utf8(out.stdout).unwrap();
    let babel: String = editor()
        .export_segments("sample.docx", "sv-SE")
        .lines()
        .map(|l| match l.strip_prefix("## ").and_then(|h| h.split_once(" · ")) {
            Some((n, _)) => format!("## {n}"),
            None => l.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(script.trim_end(), babel.trim_end());
}
