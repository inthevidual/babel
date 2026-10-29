//! Finding a paragraph in a part and telling what its runs are.

use std::collections::HashMap;
use std::ops::Range;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::tree::Tree;
use crate::writeback::Info;

/// The byte range of the `w:p` whose `data-bp` is `pid` in a part's XML
/// (`<w:p …>` to its `</w:p>`, nested paragraphs included).
pub fn find_paragraph(part: &str, pid: &str) -> Option<Range<usize>> {
    let mut r = Reader::from_str(part);
    let mut open: Option<(usize, usize)> = None; // (start, depth of w:p elements)
    let mut depth = 0usize;
    loop {
        let start = r.buffer_position() as usize;
        let ev = r.read_event().ok()?;
        let end = r.buffer_position() as usize;
        match &ev {
            Event::Eof => return None,
            Event::Start(e) | Event::Empty(e) => {
                let name = e.name();
                if !name.into_inner().ends_with(":p") && name.into_inner() != "p" {
                    continue;
                }
                let empty = matches!(ev, Event::Empty(_));
                if open.is_none() {
                    let is = e.attributes().flatten().any(|a| a.key.into_inner() == "data-bp" && a.value == pid);
                    if is {
                        if empty {
                            return Some(start..end);
                        }
                        open = Some((start, depth));
                    }
                }
                if !empty {
                    depth += 1;
                }
            }
            Event::End(e) => {
                let name = e.name();
                if !name.into_inner().ends_with(":p") && name.into_inner() != "p" {
                    continue;
                }
                depth -= 1;
                if let Some((s, d)) = open {
                    if d == depth {
                        return Some(s..end);
                    }
                }
            }
            _ => {}
        }
    }
}

/// What a run of a paragraph is to the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    /// Translatable text (one `w:t`).
    Text,
    /// A tab or line break: may be deleted.
    Atom,
    /// Anything else (pictures, note references, field parts, deleted text):
    /// kept where it is, never changed.
    Fixed,
}

/// The paragraph's own runs (not those of nested text-box paragraphs, not
/// deleted or fallback copies), with what each is and its text.
pub struct Runs {
    pub info: Info,
    pub kinds: HashMap<String, RunKind>,
    pub texts: HashMap<String, String>,
}

pub fn runs(p_xml: &str) -> Result<Runs, crate::tree::XmlError> {
    let t = Tree::parse(p_xml)?;
    let p = t.root;
    let local = |id| t.local_name(id).unwrap_or("");
    let mut info = Info::default();
    let mut kinds = HashMap::new();
    let mut texts = HashMap::new();
    for r in t.descendants(p) {
        if local(r) != "r" {
            continue;
        }
        let owner = t.ancestors(r).find(|&a| local(a) == "p");
        if owner != Some(p) {
            continue;
        }
        let Some(rid) = t.attr(r, "data-br").map(str::to_owned) else { continue };
        let skipped = t.ancestors(r).any(|a| matches!(local(a), "del" | "moveFrom" | "Fallback"));
        let content: Vec<_> = t.elements(r).into_iter().filter(|&k| local(k) != "rPr").collect();
        let only = (content.len() == 1).then(|| content[0]);
        let kind = match only.map(local) {
            _ if skipped => RunKind::Fixed,
            Some("t") if t.attr(r, "data-bk").is_none() => RunKind::Text,
            Some("tab") => RunKind::Atom,
            Some("br") if only.and_then(|b| t.attr(b, "w:type")).is_none() => RunKind::Atom,
            _ => RunKind::Fixed,
        };
        match kind {
            RunKind::Text => {
                info.text_runs.push(rid.clone());
                texts.insert(rid.clone(), t.text_content(only.unwrap()));
            }
            RunKind::Atom => {
                info.atoms.insert(rid.clone());
            }
            RunKind::Fixed => {}
        }
        kinds.insert(rid, kind);
    }
    Ok(Runs { info, kinds, texts })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PART: &str = r#"<w:document xmlns:w="w"><w:body><w:p data-bp="d:0"><w:r data-br="d:r0"><w:t>A</w:t></w:r><w:r data-br="d:r1"><w:drawing><w:txbxContent><w:p data-bp="d:1"><w:r data-br="d:r2"><w:t>Box</w:t></w:r></w:p></w:txbxContent></w:drawing></w:r><w:r data-br="d:r3"><w:tab/></w:r><w:del><w:r data-br="d:r4"><w:delText>gone</w:delText></w:r></w:del></w:p><w:p data-bp="d:2"/></w:body></w:document>"#;

    #[test]
    fn finds_paragraphs_nested_or_empty() {
        let outer = find_paragraph(PART, "d:0").unwrap();
        assert!(PART[outer.clone()].starts_with(r#"<w:p data-bp="d:0">"#) && PART[outer].ends_with("</w:del></w:p>"));
        assert_eq!(&PART[find_paragraph(PART, "d:1").unwrap()], r#"<w:p data-bp="d:1"><w:r data-br="d:r2"><w:t>Box</w:t></w:r></w:p>"#);
        assert_eq!(&PART[find_paragraph(PART, "d:2").unwrap()], r#"<w:p data-bp="d:2"/>"#);
        assert!(find_paragraph(PART, "d:9").is_none());
    }

    #[test]
    fn tells_runs_apart() {
        let r = runs(&PART[find_paragraph(PART, "d:0").unwrap()]).unwrap();
        assert_eq!(r.info.text_runs, ["d:r0"]);
        assert!(r.info.atoms.contains("d:r3"));
        assert_eq!(r.kinds["d:r1"], RunKind::Fixed);
        assert_eq!(r.kinds["d:r4"], RunKind::Fixed);
        assert!(!r.kinds.contains_key("d:r2"), "a text box's runs belong to its own paragraph");
        assert_eq!(r.texts["d:r0"], "A");
    }
}
