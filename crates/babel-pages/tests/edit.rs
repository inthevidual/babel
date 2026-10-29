//! Native editing of the sample translation: what is typed and deleted ends
//! up in the runs Babel's web editor would put it in, and nothing else in the
//! XML changes.

use babel_pages::edit::{EditError, Piece, Session};
use wordcraft_doc::Pos;

fn session() -> Session {
    let dir = env!("CARGO_MANIFEST_DIR");
    let original = std::fs::read(format!("{dir}/../../test/fixtures/sample.docx")).unwrap();
    let parts: babel_pages::Parts = serde_json::from_str(include_str!("sample-parts.json")).unwrap();
    Session::open(original, parts).unwrap()
}

fn text_of(s: &Session, pos: &Pos) -> String {
    s.doc.para(pos.story, &pos.path).unwrap().text.clone()
}

fn para_xml<'a>(s: &'a Session, pid: &str) -> &'a str {
    let xml = &s.parts["word/document.xml"];
    &xml[babel_docx::para::find_paragraph(xml, pid).unwrap()]
}

fn run_text(xml: &str, rid: &str) -> String {
    let i = xml.find(&format!(r#"data-br="{rid}""#)).unwrap();
    let t = &xml[i..];
    let a = t.find("<w:t").unwrap();
    let a = a + t[a..].find('>').unwrap() + 1;
    t[a..a + t[a..].find("</w:t>").unwrap()].to_owned()
}

/// The pieces always spell WordCraft's text, and the stored XML, opened
/// afresh, lays out the same text.
fn consistent(s: &mut Session, pos: &Pos, pid: &str) {
    let pieces: String = s.pieces(pos).unwrap().iter().map(|p| match p {
        Piece::Text { text, .. } | Piece::Atom { text, .. } | Piece::Fixed { text, .. } => text.clone(),
    }).collect();
    assert_eq!(pieces, text_of(s, pos));
    let again = Session::open(s.original().to_vec(), s.parts.clone()).unwrap();
    let p2 = again.locate(pid).unwrap();
    assert_eq!(text_of(&again, &p2), text_of(s, pos), "the saved XML reads back as what is shown");
}

#[test]
fn typing_goes_into_the_run_before_the_caret() {
    let mut s = session();
    let p = s.locate("document:2").unwrap();
    let (c, pages) = s.insert(&p, "Denna ").unwrap();
    assert_eq!(c.off, 6);
    assert!(pages.contains(&0), "page 1 is drawn again: {pages:?}");
    assert_eq!(run_text(para_xml(&s, "document:2"), "document:r2"), "Denna This report is ");
    // At the end of the plain run, before the bold one: stays plain.
    let at = Pos::new(p.story, p.path.clone(), "Denna This report is ".len());
    s.insert(&at, "X").unwrap();
    let xml = para_xml(&s, "document:2");
    assert_eq!(run_text(xml, "document:r2"), "Denna This report is X");
    assert_eq!(run_text(xml, "document:r3"), "confidential");
    consistent(&mut s, &p, "document:2");
}

#[test]
fn deleting_works_within_and_across_runs() {
    let mut s = session();
    let p = s.locate("document:2").unwrap();
    // Backspace at the start of "confidential" removes the space before it.
    let at = Pos::new(p.story, p.path.clone(), "This report is ".len());
    let (c, _) = s.delete(&at, -1).unwrap();
    assert_eq!(c.off, "This report is".len());
    assert_eq!(run_text(para_xml(&s, "document:2"), "document:r2"), "This report is");
    // Delete forward eats the bold run's first letter.
    s.delete(&c, 1).unwrap();
    assert_eq!(run_text(para_xml(&s, "document:2"), "document:r3"), "onfidential");
    // Backspace at the start of a paragraph does nothing: paragraphs are never merged.
    let (c0, pages) = s.delete(&p, -1).unwrap();
    assert_eq!((c0.off, pages.len()), (0, 0));
    consistent(&mut s, &p, "document:2");
}

#[test]
fn tabs_can_be_deleted_and_nothing_else_moves() {
    let mut s = session();
    let p = s.locate("document:4").unwrap();
    let before = para_xml(&s, "document:4").to_owned();
    let tab = Pos::new(p.story, p.path.clone(), "Item".len());
    s.delete(&tab, 1).unwrap();
    let after = para_xml(&s, "document:4");
    assert!(!after.contains(r#"data-br="document:r13""#), "the first tab's run is gone");
    assert!(after.contains(r#"data-br="document:r15""#));
    for rid in ["document:r12", "document:r14", "document:r16"] {
        assert_eq!(run_text(after, rid), run_text(&before, rid));
    }
    assert!(after.starts_with(&before[..before.find("<w:r ").unwrap()]), "the paragraph properties are untouched");
    assert_eq!(text_of(&s, &p), "ItemAmount\tShare");
    consistent(&mut s, &p, "document:4");
}

#[test]
fn a_field_result_is_never_lost() {
    let mut s = session();
    let p = s.locate("document:12").unwrap();
    s.insert(&p, "Utskriven ").unwrap();
    let xml = para_xml(&s, "document:12");
    assert_eq!(run_text(xml, "document:r39"), "2025-06-30", "the date field keeps its result");
    assert!(run_text(xml, "document:r35").starts_with("Utskriven "));
}

#[test]
fn pictures_and_note_references_are_not_deleted() {
    let mut s = session();
    // The paragraph with the footnote reference: "A footnote follows this sentence.¹ And…"
    let pid = (0..40).map(|i| format!("document:{i}")).find(|id| {
        s.locate(id).is_some_and(|p| text_of(&s, &p).starts_with("A footnote follows"))
    }).unwrap();
    let p = s.locate(&pid).unwrap();
    let t = text_of(&s, &p);
    let obj = t.find('\u{FFFC}').expect("the reference is an object in the text");
    let e = s.delete(&Pos::new(p.story, p.path.clone(), obj), 1).unwrap_err();
    assert!(matches!(e, EditError::Fixed), "{e}");
}

#[test]
fn only_the_edited_paragraph_changes_in_the_part() {
    let mut s = session();
    let before = s.parts["word/document.xml"].clone();
    let p = s.locate("document:2").unwrap();
    s.insert(&p, "Ny ").unwrap();
    let after = &s.parts["word/document.xml"];
    let r0 = babel_docx::para::find_paragraph(&before, "document:2").unwrap();
    let r1 = babel_docx::para::find_paragraph(after, "document:2").unwrap();
    assert_eq!(before[..r0.start], after[..r1.start]);
    assert_eq!(before[r0.end..], after[r1.end..]);
    assert_eq!(s.take_dirty().keys().collect::<Vec<_>>(), ["word/document.xml"]);
    assert!(s.take_dirty().is_empty());
}
