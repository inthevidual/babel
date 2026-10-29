//! Three-way merge of translated OOXML parts, paragraph by paragraph.
//!
//! Babel never splits or merges paragraphs, so every copy of a part has the
//! same paragraphs in the same order, each tagged with a stable `data-bp` id
//! when the original was normalised. Two devices that edited a translation
//! independently can therefore be reconciled per paragraph: a paragraph that
//! changed on one side only takes that side's text, and one that changed on
//! both is a conflict that the incoming side wins (the caller keeps the
//! other as a version). Everything outside paragraphs is never edited and is
//! taken from the stored copy.
//!
//! Relationship parts (`.rels`, changed when a link is pasted) merge the same
//! way per `Relationship` by `Id`, and relationships added on the incoming
//! side are kept.
//!
//! The text of each paragraph is compared and spliced as it was serialised;
//! nothing is re-serialised.

use std::collections::HashMap;
use std::ops::Range;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A text part (document, header, footer, footnotes…): units are the
    /// outermost `w:p` elements, keyed by `data-bp`.
    Paragraphs,
    /// A relationship part: units are `Relationship` elements, keyed by `Id`.
    Relationships,
}

impl Kind {
    pub fn for_path(path: &str) -> Kind {
        if path.ends_with(".rels") {
            Kind::Relationships
        } else {
            Kind::Paragraphs
        }
    }
}

#[derive(Debug)]
pub struct Merged {
    pub xml: String,
    /// Units changed on both sides to different text. The incoming side won.
    pub conflicts: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("cannot merge: {0}")]
pub struct MergeError(String);

struct Unit {
    key: String,
    range: Range<usize>,
}

/// Merges `ours` (incoming) and `theirs` (stored) given their common
/// ancestor `base`. Without a base, or if the paragraph structure of the two
/// differs, `ours` wins whole and every differing unit is a conflict.
pub fn merge3(
    base: Option<&str>,
    ours: &str,
    theirs: &str,
    kind: Kind,
) -> Result<Merged, MergeError> {
    if ours == theirs {
        return Ok(Merged {
            xml: theirs.to_owned(),
            conflicts: vec![],
        });
    }
    let ou = units(ours, kind)?;
    let tu = units(theirs, kind)?;
    let whole = |conflicts: Vec<String>| {
        Ok(Merged {
            xml: ours.to_owned(),
            conflicts,
        })
    };
    let Some(base) = base else {
        return whole(differing(ours, &ou, theirs, &tu));
    };
    if base == theirs {
        return Ok(Merged {
            xml: ours.to_owned(),
            conflicts: vec![],
        });
    }
    if base == ours {
        return Ok(Merged {
            xml: theirs.to_owned(),
            conflicts: vec![],
        });
    }
    if kind == Kind::Paragraphs && !ou.iter().map(|u| &u.key).eq(tu.iter().map(|u| &u.key)) {
        return whole(vec!["(paragraph structure)".into()]);
    }
    let bu = units(base, kind)?;
    let bmap: HashMap<&str, &str> = bu
        .iter()
        .map(|u| (u.key.as_str(), &base[u.range.clone()]))
        .collect();
    let omap: HashMap<&str, &str> = ou
        .iter()
        .map(|u| (u.key.as_str(), &ours[u.range.clone()]))
        .collect();
    let tkeys: std::collections::HashSet<&str> = tu.iter().map(|u| u.key.as_str()).collect();

    let mut xml = String::with_capacity(theirs.len() + 1024);
    let mut conflicts = vec![];
    let mut at = 0;
    for u in &tu {
        xml.push_str(&theirs[at..u.range.start]);
        let t = &theirs[u.range.clone()];
        let chosen = match (omap.get(u.key.as_str()), bmap.get(u.key.as_str())) {
            (None, _) => t,
            (Some(&o), Some(&b)) if o == b => t,
            (Some(&o), Some(&b)) if t == b => o,
            (Some(&o), _) if o == t => t,
            (Some(&o), _) => {
                conflicts.push(u.key.clone());
                o
            }
        };
        xml.push_str(chosen);
        at = u.range.end;
    }
    let rest = &theirs[at..];
    if kind == Kind::Relationships {
        // Relationships added on the incoming side go before the closing tag.
        let added: String = ou
            .iter()
            .filter(|u| !tkeys.contains(u.key.as_str()) && !bmap.contains_key(u.key.as_str()))
            .map(|u| &ours[u.range.clone()])
            .collect();
        match rest.rfind("</") {
            Some(close) if !added.is_empty() => {
                xml.push_str(&rest[..close]);
                xml.push_str(&added);
                xml.push_str(&rest[close..]);
            }
            _ => xml.push_str(rest),
        }
    } else {
        xml.push_str(rest);
    }
    Ok(Merged { xml, conflicts })
}

fn differing(ours: &str, ou: &[Unit], theirs: &str, tu: &[Unit]) -> Vec<String> {
    let tmap: HashMap<&str, &str> = tu
        .iter()
        .map(|u| (u.key.as_str(), &theirs[u.range.clone()]))
        .collect();
    let mut out: Vec<String> = ou
        .iter()
        .filter(|u| tmap.get(u.key.as_str()) != Some(&&ours[u.range.clone()]))
        .map(|u| u.key.clone())
        .collect();
    if out.is_empty() {
        out.push("(outside paragraphs)".into());
    }
    out
}

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    // Ids never contain entities, so the raw value is the value.
    e.attributes()
        .flatten()
        .find(|a| a.key.into_inner() == name)
        .map(|a| a.value.into_owned())
}

/// The prefix bound to the WordprocessingML namespace on the root element
/// (`w` in practice).
fn w_prefix(root: &BytesStart) -> String {
    for a in root.attributes().flatten() {
        let k = a.key.into_inner();
        if a.value == W {
            if let Some(p) = k.strip_prefix("xmlns:") {
                return p.to_owned();
            }
        }
    }
    "w".to_owned()
}

fn units(xml: &str, kind: Kind) -> Result<Vec<Unit>, MergeError> {
    let mut r = Reader::from_str(xml);
    let mut out = vec![];
    let mut p_name: Option<String> = None; // qualified name of w:p
    let mut depth = 0usize; // nesting of w:p (text boxes nest them)
    let mut open: Option<(String, usize)> = None;
    loop {
        let start = r.buffer_position() as usize;
        let ev = r
            .read_event()
            .map_err(|e| MergeError(format!("XML at byte {start}: {e}")))?;
        let end = r.buffer_position() as usize;
        match ev {
            Event::Eof => break,
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(ev, Event::Empty(_));
                let name = e.name();
                let name = name.into_inner();
                match kind {
                    Kind::Paragraphs => {
                        let pn = p_name.get_or_insert_with(|| {
                            let mut n = w_prefix(e);
                            n.push_str(":p");
                            n
                        });
                        if name != pn.as_str() {
                            continue;
                        }
                        if depth == 0 {
                            if let Some(key) = attr(e, "data-bp") {
                                if empty {
                                    out.push(Unit {
                                        key,
                                        range: start..end,
                                    });
                                } else {
                                    open = Some((key, start));
                                }
                            }
                        }
                        if !empty {
                            depth += 1;
                        }
                    }
                    Kind::Relationships => {
                        if name == "Relationship" || name.ends_with(":Relationship") {
                            let key = attr(e, "Id")
                                .ok_or_else(|| MergeError("relationship without Id".into()))?;
                            if empty {
                                out.push(Unit {
                                    key,
                                    range: start..end,
                                });
                            } else {
                                open = Some((key, start));
                            }
                        }
                    }
                }
            }
            Event::End(ref e) => {
                let name = e.name();
                let name = name.into_inner();
                let closes = match kind {
                    Kind::Paragraphs => {
                        if p_name.as_deref() != Some(name) {
                            continue;
                        }
                        depth = depth.saturating_sub(1);
                        depth == 0
                    }
                    Kind::Relationships => {
                        name == "Relationship" || name.ends_with(":Relationship")
                    }
                };
                if closes {
                    if let Some((key, s)) = open.take() {
                        out.push(Unit { key, range: s..end });
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(paras: &[(&str, &str)]) -> String {
        let body: String = paras
            .iter()
            .map(|(id, t)| {
                format!(r#"<w:p data-bp="{id}"><w:r data-br="{id}r"><w:t>{t}</w:t></w:r></w:p>"#)
            })
            .collect();
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}<w:sectPr/></w:body></w:document>"#
        )
    }

    #[test]
    fn edits_on_different_paragraphs_combine() {
        let base = doc(&[("d:1", "One"), ("d:2", "Two"), ("d:3", "Three")]);
        let ours = doc(&[("d:1", "Ett"), ("d:2", "Two"), ("d:3", "Three")]);
        let theirs = doc(&[("d:1", "One"), ("d:2", "Two"), ("d:3", "Tre")]);
        let m = merge3(Some(&base), &ours, &theirs, Kind::Paragraphs).unwrap();
        assert_eq!(
            m.xml,
            doc(&[("d:1", "Ett"), ("d:2", "Two"), ("d:3", "Tre")])
        );
        assert!(m.conflicts.is_empty());
    }

    #[test]
    fn same_paragraph_conflict_takes_ours() {
        let base = doc(&[("d:1", "One"), ("d:2", "Two")]);
        let ours = doc(&[("d:1", "Ett"), ("d:2", "Two")]);
        let theirs = doc(&[("d:1", "Uno"), ("d:2", "Dos")]);
        let m = merge3(Some(&base), &ours, &theirs, Kind::Paragraphs).unwrap();
        assert_eq!(m.xml, doc(&[("d:1", "Ett"), ("d:2", "Dos")]));
        assert_eq!(m.conflicts, vec!["d:1"]);
    }

    #[test]
    fn identical_edits_are_not_conflicts() {
        let base = doc(&[("d:1", "One")]);
        let both = doc(&[("d:1", "Ett")]);
        let m = merge3(Some(&base), &both, &both, Kind::Paragraphs).unwrap();
        assert!(m.conflicts.is_empty());
    }

    #[test]
    fn no_base_means_ours_wins_whole() {
        let ours = doc(&[("d:1", "Ett"), ("d:2", "Two")]);
        let theirs = doc(&[("d:1", "Uno"), ("d:2", "Two")]);
        let m = merge3(None, &ours, &theirs, Kind::Paragraphs).unwrap();
        assert_eq!(m.xml, ours);
        assert_eq!(m.conflicts, vec!["d:1"]);
    }

    #[test]
    fn nested_paragraphs_belong_to_the_outer_one() {
        let tx = |outer: &str, inner: &str| {
            format!(
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p data-bp="d:1"><w:r><w:t>{outer}</w:t></w:r><w:r><w:txbxContent><w:p data-bp="d:2"><w:r><w:t>{inner}</w:t></w:r></w:p></w:txbxContent></w:r></w:p><w:p data-bp="d:3"/></w:body></w:document>"#
            )
        };
        let u = units(&tx("a", "b"), Kind::Paragraphs).unwrap();
        assert_eq!(
            u.iter().map(|u| u.key.as_str()).collect::<Vec<_>>(),
            ["d:1", "d:3"]
        );
        let m = merge3(
            Some(&tx("a", "b")),
            &tx("a", "B"),
            &tx("a", "b"),
            Kind::Paragraphs,
        )
        .unwrap();
        assert_eq!(m.xml, tx("a", "B"));
    }

    #[test]
    fn other_prefix_for_the_w_namespace() {
        let d = |t: &str| {
            format!(
                r#"<x:document xmlns:x="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><x:body><x:p data-bp="d:1"><x:r><x:t>{t}</x:t></x:r></x:p><x:p data-bp="d:2"/></x:body></x:document>"#
            )
        };
        assert_eq!(units(&d("a"), Kind::Paragraphs).unwrap().len(), 2);
    }

    #[test]
    fn relationships_added_on_both_sides_are_kept() {
        let rels = |extra: &str| {
            format!(
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="t" Target="styles.xml"/>{extra}</Relationships>"#
            )
        };
        let base = rels("");
        let ours = rels(
            r#"<Relationship Id="rIdA" Type="h" Target="https://a.example/" TargetMode="External"/>"#,
        );
        let theirs = rels(
            r#"<Relationship Id="rIdB" Type="h" Target="https://b.example/" TargetMode="External"/>"#,
        );
        let m = merge3(Some(&base), &ours, &theirs, Kind::Relationships).unwrap();
        assert!(
            m.xml.contains("rIdA") && m.xml.contains("rIdB"),
            "{}",
            m.xml
        );
        assert!(m.xml.ends_with("</Relationships>"));
    }
}
