//! Importing and exporting a .docx give what web/js/ooxml.js gives
//! (openPackage, normalize, buildDocx), on cases it made from the fixtures
//! (test/export-cases.mjs).

use std::collections::BTreeMap;
use std::io::Read;

use babel_docx::package::{self, Parts};
use babel_docx::tree::{Kind, Tree};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    file: String,
    text_parts: Vec<String>,
    imported: Parts,
    parts: Parts,
    lang: String,
    output: BTreeMap<String, String>,
    entries: Vec<String>,
}

fn cases() -> Vec<Case> {
    serde_json::from_str(include_str!("export-cases.json")).unwrap()
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../../test/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn canonical(xml: &str) -> String {
    let t = Tree::parse(xml).unwrap();
    let mut out = String::new();
    write(&t, t.root, &mut out);
    out
}

fn write(t: &Tree, id: usize, out: &mut String) {
    match t.kind(id) {
        Kind::Text(s) => out.push_str(&format!("{s:?}")),
        Kind::Raw(s) => out.push_str(s),
        Kind::Element { name, attrs } => {
            let mut a: Vec<_> = attrs.iter().filter(|(k, _)| !k.starts_with("xmlns")).cloned().collect();
            a.sort();
            out.push_str(&format!("<{name} {a:?}>"));
            for &c in t.children(id) {
                write(t, c, out);
            }
            out.push_str(&format!("</{name}>"));
        }
    }
}

fn entry(zip: &[u8], name: &str) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(zip)).unwrap();
    let mut f = z.by_name(name).unwrap();
    let mut v = vec![];
    f.read_to_end(&mut v).unwrap();
    v
}

#[test]
fn import_matches() {
    for c in cases() {
        let imp = package::import(&fixture(&c.file)).unwrap();
        assert_eq!(imp.package.text_parts, c.text_parts, "{}", c.file);
        for (path, xml) in &c.imported {
            assert_eq!(canonical(&imp.parts[path]), canonical(xml), "{} {path}", c.file);
        }
    }
}

#[test]
fn export_matches() {
    for c in cases() {
        let original = fixture(&c.file);
        let out = package::build_docx(&original, &c.parts, Some(&c.lang)).unwrap();
        let mut names: Vec<String> = {
            let z = zip::ZipArchive::new(std::io::Cursor::new(&out[..])).unwrap();
            z.file_names().filter_map(|n| n.ok().map(|n| n.into_owned())).filter(|n| !n.ends_with('/')).collect()
        };
        names.sort();
        assert_eq!(names, c.entries, "{}: the same files", c.file);
        for (path, xml) in &c.output {
            let got = String::from_utf8(entry(&out, path)).unwrap();
            assert_eq!(canonical(&got), canonical(xml), "{} {path}", c.file);
            if c.text_parts.contains(path) {
                assert!(!got.contains("data-b"), "no Babel ids left in {path}");
            }
        }
        // Everything else is the original's, byte for byte.
        for name in &c.entries {
            if c.output.contains_key(name) {
                continue;
            }
            assert_eq!(entry(&out, name), entry(&original, name), "{} {name}", c.file);
        }
    }
}

#[test]
fn pairing_checks_the_structure() {
    let original = fixture("sample.docx");
    let imp = package::import(&original).unwrap();
    // The original paired with itself: same structure.
    let paired = package::pair(&imp, &original).unwrap();
    assert_eq!(paired.keys().collect::<Vec<_>>(), imp.parts.keys().collect::<Vec<_>>());
    // A different document: refused, saying why.
    let other = fixture("long.docx");
    let e = package::pair(&imp, &other).unwrap_err().to_string();
    assert!(e.contains("can’t be paired") || e.contains("doesn’t share"), "{e}");
    assert!(package::import(b"not a zip").is_err());
}
