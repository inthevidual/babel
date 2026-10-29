//! src/ooxml.rs gives what web/js/ooxml.js gives, on cases the JavaScript
//! made from the fixtures (test/ooxml-cases.mjs).

use std::collections::HashMap;

use babel_docx::ooxml::{self, Link, LinkTarget, Part, StyleBit};
use babel_docx::tree::{Kind, Tree};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Cases {
    normalize: Vec<Whole>,
    ids: Vec<Whole>,
    text: Vec<Text>,
    links: Vec<Links>,
    #[serde(rename = "linkRange")]
    link_range: Vec<Range>,
}

#[derive(Deserialize)]
struct Whole {
    key: String,
    input: String,
    output: String,
}

#[derive(Deserialize)]
struct Text {
    xml: String,
    text: String,
}

#[derive(Deserialize)]
struct Links {
    xml: String,
    rid: String,
    rels: HashMap<String, String>,
    link: Link,
}

#[derive(Deserialize)]
struct Range {
    xml: String,
    start: usize,
    end: usize,
    target: Value,
    style: Vec<JsStyle>,
    problem: Option<String>,
    after: Option<String>,
}

#[derive(Deserialize)]
struct JsStyle {
    name: String,
    attrs: serde_json::Map<String, Value>,
}

fn cases() -> Cases {
    serde_json::from_str(include_str!("ooxml-cases.json")).unwrap()
}

/// The same XML whatever the serialiser: attributes sorted (namespace
/// declarations left out: browsers repeat them on detached elements).
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

/// JavaScript's string offsets are UTF-16 units; the Rust ones are bytes.
fn byte_offset(text: &str, utf16: usize) -> usize {
    let mut n = 0;
    for (i, c) in text.char_indices() {
        if n >= utf16 {
            return i;
        }
        n += c.len_utf16();
    }
    text.len()
}

#[test]
fn normalize_matches() {
    for c in cases().normalize {
        let mut part = Part::parse(&c.input).unwrap();
        ooxml::normalize(&mut part.tree, &c.key);
        let got = part.serialize();
        assert_eq!(canonical(&got), canonical(&c.output), "normalize {}", c.key);
        assert!(got.starts_with("<?xml") && got.contains("?>\r\n<"), "declaration and CRLF kept");
    }
}

#[test]
fn ids_after_editing_match() {
    for c in cases().ids {
        assert!(c.input.contains("data-bgen"), "the case has generated runs");
        let mut part = Part::parse(&c.input).unwrap();
        ooxml::ensure_ids(&mut part.tree, &c.key);
        let got = part.serialize();
        assert!(!got.contains("data-bgen"));
        assert_eq!(canonical(&got), canonical(&c.output), "ensureIds {}", c.key);
    }
}

#[test]
fn paragraph_text_matches() {
    let all = cases().text;
    assert!(all.len() > 300);
    for c in all {
        let t = Tree::parse(&c.xml).unwrap();
        assert_eq!(ooxml::para_text(&t, t.root), c.text, "{}", &c.xml[..c.xml.len().min(200)]);
    }
}

#[test]
fn links_of_runs_match() {
    let all = cases().links;
    assert!(!all.is_empty());
    for c in all {
        let t = Tree::parse(&c.xml).unwrap();
        let r = ooxml::find_run(&t, &c.rid).unwrap();
        assert_eq!(ooxml::link_of_run(&t, r, &c.rels), Some(c.link));
    }
}

#[test]
fn link_range_matches() {
    let all = cases().link_range;
    assert!(all.len() > 1000);
    let (mut linked, mut refused) = (0, 0);
    for c in all {
        let mut t = Tree::parse(&c.xml).unwrap();
        let text = ooxml::run_text(&t, t.root);
        let (s, e) = (byte_offset(&text, c.start), byte_offset(&text, c.end));
        let target = match (c.target["relId"].as_str(), c.target["anchor"].as_str()) {
            (Some(r), _) => LinkTarget::Rel(r.to_owned()),
            (None, Some(a)) => LinkTarget::Anchor(a.to_owned()),
            _ => unreachable!(),
        };
        let style: Vec<StyleBit> = c.style.iter().map(|b| StyleBit {
            name: b.name.clone(),
            attrs: b.attrs.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned())).collect(),
        }).collect();
        let root = t.root;
        let problem = ooxml::link_range(&mut t, root, s, e, &target, &style);
        assert_eq!(problem.map(str::to_owned), c.problem, "{}", &c.xml[..c.xml.len().min(300)]);
        match c.after {
            Some(after) => {
                assert_eq!(canonical(&t.serialize()), canonical(&after), "{}..{} of {}", c.start, c.end, &c.xml[..c.xml.len().min(300)]);
                linked += 1;
            }
            None => refused += 1,
        }
    }
    assert!(linked > 500 && refused > 100, "linked {linked}, refused {refused}");
}

#[test]
fn hyperlink_fields_and_relationships() {
    let p = r#"<w:p xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:r data-br="a"><w:fldChar w:fldCharType="begin"/></w:r><w:r data-br="b"><w:instrText> HYPERLINK "https://example.org/x" \o "tip" </w:instrText></w:r><w:r data-br="c"><w:fldChar w:fldCharType="separate"/></w:r><w:r data-br="d"><w:t>here</w:t></w:r><w:r data-br="e"><w:fldChar w:fldCharType="end"/></w:r><w:fldSimple w:instr="HYPERLINK \l &quot;sec2&quot;"><w:r data-br="f"><w:t>below</w:t></w:r></w:fldSimple></w:p>"#;
    let t = Tree::parse(p).unwrap();
    let rels = HashMap::new();
    let link = |rid| ooxml::link_of_run(&t, ooxml::find_run(&t, rid).unwrap(), &rels);
    assert_eq!(link("d").and_then(|l| l.url).as_deref(), Some("https://example.org/x"));
    assert_eq!(link("f").and_then(|l| l.anchor).as_deref(), Some("sec2"));

    let mut rels = Part::parse(&ooxml::empty_rels()).unwrap();
    let a = ooxml::add_link_rel(&mut rels, "https://a.example/");
    let b = ooxml::add_link_rel(&mut rels, "https://b.example/");
    assert_eq!((a.as_str(), b.as_str()), ("rIdBabel1", "rIdBabel2"));
    assert_eq!(ooxml::add_link_rel(&mut rels, "https://a.example/"), "rIdBabel1", "the same address reuses its relationship");
    let map = ooxml::rels_map(Some(&rels.serialize()));
    assert_eq!(map["rIdBabel2"], "https://b.example/");
    assert_eq!(ooxml::rels_path_for("word/document.xml"), "word/_rels/document.xml.rels");
}
