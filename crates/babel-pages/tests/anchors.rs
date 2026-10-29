//! Every paragraph Babel can edit must be found on the pages by its id.

use std::io::Read;

/// The fixture with `data-bp="document:N"` on every `w:p` of the body, as
/// Babel's normalisation gives them, and the ids given.
fn anchored(name: &str) -> (Vec<u8>, babel_pages::Parts, Vec<String>) {
    let path = format!("{}/../../test/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(path).unwrap();
    let mut xml = String::new();
    zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap().by_name("word/document.xml").unwrap().read_to_string(&mut xml).unwrap();
    let (mut out, mut ids, mut rest) = (String::new(), vec![], xml.as_str());
    while let Some(i) = rest.find("<w:p") {
        let after = &rest[i + 4..];
        out.push_str(&rest[..i + 4]);
        if after.starts_with('>') || after.starts_with(' ') || after.starts_with('/') {
            let id = format!("document:{}", ids.len());
            out.push_str(&format!(r#" data-bp="{id}""#));
            ids.push(id);
        }
        rest = after;
    }
    out.push_str(rest);
    (bytes, babel_pages::Parts::from([("word/document.xml".to_owned(), out)]), ids)
}

fn check(name: &str) {
    let (bytes, parts, ids) = anchored(name);
    let laid = babel_pages::lay_out(&babel_pages::renderable(&bytes, &parts).unwrap()).unwrap();
    let missing: Vec<&String> = ids.iter().filter(|id| !laid.pages_of.contains_key(*id)).collect();
    assert!(missing.is_empty(), "{name}: {} of {} paragraphs not on any page: {missing:?}", missing.len(), ids.len());
}

#[test]
fn every_paragraph_of_the_sample_is_placed() {
    check("sample.docx");
}

#[test]
fn every_paragraph_of_the_long_document_is_placed() {
    check("long.docx");
}
