//! Writing an edited paragraph back into its `w:p`: the Rust counterpart of
//! `applyParagraph` in web/js/ooxml.js, which it must match exactly (the web
//! and desktop editors save the same translations). tests/parity.rs checks
//! it against cases produced by the JavaScript.
//!
//! Tokens describe the edited paragraph in visual order:
//! - text, with the run it sits in (`rid`, if any) and the run to borrow
//!   formatting from (`base`), plus explicit bold/italic/underline changes;
//! - an atom: a non-text run that is still there (tab, break, picture…);
//! - a line break the translator inserted.
//!
//! Only runs rendered when the session started (`Info`) are ever rewritten or
//! removed; everything else in the paragraph is left as it is.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::tree::{Id, Kind, Tree};

pub const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Token {
    Text {
        rid: Option<String>,
        base: Option<String>,
        text: String,
        /// `{"b": true, "u": false, …}`, in the order given.
        #[serde(default)]
        fmt: Option<Map<String, Value>>,
    },
    Atom { rid: String },
    Br { base: Option<String> },
}

/// Run ids rendered when the session started.
#[derive(Debug, Clone, Default)]
pub struct Info {
    /// Text runs, in document order (the first one present is the fallback
    /// formatting for new text).
    pub text_runs: Vec<String>,
    /// Tabs and breaks: these may be deleted.
    pub atoms: HashSet<String>,
}

/// The formatting each run had when the session rendered it: changes are
/// always relative to that, never cumulative.
pub type Templates = HashMap<String, Option<String>>;

const RPR_ORDER: &[&str] = &[
    "rStyle", "rFonts", "b", "bCs", "i", "iCs", "caps", "smallCaps", "strike", "dstrike", "outline", "shadow", "emboss",
    "imprint", "noProof", "snapToGrid", "vanish", "webHidden", "color", "spacing", "w", "kern", "position", "sz", "szCs",
    "highlight", "u", "effect", "bdr", "shd", "fitText", "vertAlign", "rtl", "cs", "em", "lang", "eastAsianLayout",
    "specVanish", "oMath",
];

fn rank(name: &str) -> i64 {
    RPR_ORDER.iter().position(|n| *n == name).map(|i| i as i64).unwrap_or(-1)
}

/// The prefix bound to WordprocessingML (`w`), as declared on the paragraph.
fn w_prefix(t: &Tree) -> String {
    if let Kind::Element { attrs, .. } = t.kind(t.root) {
        for (k, v) in attrs {
            if v == W {
                if let Some(p) = k.strip_prefix("xmlns:") {
                    return p.to_owned();
                }
            }
        }
    }
    "w".to_owned()
}

/// Edits `w:rPr` children in schema order (shared with the link code).
pub struct RprEditor {
    w: String,
}

impl RprEditor {
    pub fn new(w_prefix: &str) -> RprEditor {
        RprEditor { w: w_prefix.to_owned() }
    }

    fn is_w(&self, t: &Tree, id: Id, local: &str) -> bool {
        t.name(id).and_then(|n| n.strip_prefix(self.w.as_str())).and_then(|n| n.strip_prefix(':')) == Some(local)
    }
    fn wname(&self, local: &str) -> String {
        format!("{}:{}", self.w, local)
    }
    fn owner_para(&self, t: &Tree, id: Id) -> Option<Id> {
        t.ancestors(id).find(|&a| self.is_w(t, a, "p"))
    }
    fn rpr_of(&self, t: &Tree, r: Id) -> Option<Id> {
        t.elements(r).into_iter().find(|&k| self.is_w(t, k, "rPr"))
    }

    /// Inserts or replaces a `w:rPr` child in schema order (Word is strict about it).
    pub fn set_rpr_child(&self, t: &mut Tree, rpr: Id, name: &str, attrs: &[(&str, &str)]) {
        for c in t.elements(rpr) {
            if self.is_w(t, c, name) {
                t.detach(c);
            }
        }
        let qattrs: Vec<(String, &str)> = attrs.iter().map(|(k, v)| (self.wname(k), *v)).collect();
        let qa: Vec<(&str, &str)> = qattrs.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        let el = t.new_element(&self.wname(name), &qa);
        let r = rank(name);
        let before = t.elements(rpr).into_iter().find(|&c| {
            t.name(c).is_some_and(|n| n.starts_with(&format!("{}:", self.w))) && rank(t.local_name(c).unwrap_or("")) > r
        });
        match before {
            Some(b) => t.insert_before(b, el),
            None => t.append(rpr, el),
        }
    }

    fn build_run(&self, t: &mut Tree, template: Option<&str>, seg: &Seg) -> Id {
        let r = t.new_element(&self.wname("r"), &[]);
        let mut rpr = template.map(|x| graft(t, x));
        if let Some(fmt) = seg.fmt.as_ref().filter(|f| !f.is_empty()) {
            let rp = *rpr.get_or_insert_with(|| t.new_element(&self.wname("rPr"), &[]));
            for (k, on) in fmt {
                let on = on.as_bool().unwrap_or(false);
                let changes: &[(&str, &[(&str, &str)])] = match (k.as_str(), on) {
                    ("b", true) => &[("b", &[]), ("bCs", &[])],
                    ("b", false) => &[("b", &[("val", "0")]), ("bCs", &[("val", "0")])],
                    ("i", true) => &[("i", &[]), ("iCs", &[])],
                    ("i", false) => &[("i", &[("val", "0")]), ("iCs", &[("val", "0")])],
                    ("u", true) => &[("u", &[("val", "single")])],
                    ("u", false) => &[("u", &[("val", "none")])],
                    _ => &[],
                };
                for (name, attrs) in changes {
                    self.set_rpr_child(t, rp, name, attrs);
                }
            }
        }
        if let Some(rp) = rpr {
            t.append(r, rp);
        }
        if seg.br {
            let br = t.new_element(&self.wname("br"), &[]);
            t.append(r, br);
        } else {
            let el = t.new_element(&self.wname("t"), &[("xml:space", "preserve")]);
            if !seg.text.is_empty() {
                let tx = t.new_text(&seg.text);
                t.append(el, tx);
            }
            t.append(r, el);
        }
        r
    }
}

/// Parses `xml` (one element) into `t` as a detached subtree.
fn graft(t: &mut Tree, xml: &str) -> Id {
    let other = Tree::parse(xml).expect("templates are serialised elements");
    copy_into(t, &other, other.root)
}

fn copy_into(t: &mut Tree, from: &Tree, id: Id) -> Id {
    let n = match from.kind(id).clone() {
        Kind::Element { name, attrs } => {
            let a: Vec<(&str, &str)> = attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            t.new_element(&name, &a)
        }
        Kind::Text(s) => t.new_text(&s),
        Kind::Raw(s) => t.new_text(&s),
    };
    for &c in from.children(id) {
        let cc = copy_into(t, from, c);
        t.append(n, cc);
    }
    n
}

#[derive(Debug, Clone)]
struct Seg {
    text: String,
    br: bool,
    fmt: Option<Map<String, Value>>,
}

#[derive(Debug)]
struct Group {
    segs: Vec<Seg>,
    base: Option<String>,
}

fn fmt_key(f: &Option<Map<String, Value>>) -> String {
    match f {
        Some(m) => Value::Object(m.clone()).to_string(),
        None => "{}".into(),
    }
}

/// Rewrites the paragraph `xml` (one serialised `w:p`) from `tokens`.
pub fn apply_paragraph(xml: &str, tokens: &[Token], info: &mut Info, templates: &mut Templates) -> Result<String, crate::tree::XmlError> {
    let mut t = Tree::parse(xml)?;
    let cx = RprEditor { w: w_prefix(&t) };
    let p = t.root;

    // The paragraph's own runs (not those of nested text-box paragraphs).
    let mut own: Vec<(String, Id)> = vec![];
    for r in t.descendants(p) {
        if !cx.is_w(&t, r, "r") || cx.owner_para(&t, r) != Some(p) {
            continue;
        }
        if t.attr(r, "data-bgen").is_some() {
            continue;
        }
        own.push((t.attr(r, "data-br").unwrap_or("").to_owned(), r));
    }
    let own_get = |rid: &str| own.iter().find(|(k, _)| k == rid).map(|(_, r)| *r);
    // Runs Babel generated in an earlier write-back are regenerated.
    for r in t.descendants(p) {
        if t.attr(r, "data-bgen").is_some() && cx.owner_para(&t, r) == Some(p) {
            t.detach(r);
        }
    }

    let mut tmpl = |t: &Tree, rid: Option<&str>| -> Option<String> {
        let rid = rid?;
        if !templates.contains_key(rid) {
            let v = own_get(rid).and_then(|r| cx.rpr_of(t, r)).map(|rp| t.serialize_node(rp));
            templates.insert(rid.to_owned(), v);
        }
        templates[rid].clone()
    };

    // Group segments by the node they belong after.
    let mut order: Vec<String> = vec![];
    let mut groups: HashMap<String, Group> = HashMap::new();
    let mut key = "^".to_owned();
    let mut present_atoms: HashSet<String> = HashSet::new();
    let group = |k: &str, base: Option<String>, order: &mut Vec<String>, groups: &mut HashMap<String, Group>| {
        if !groups.contains_key(k) {
            order.push(k.to_owned());
            groups.insert(k.to_owned(), Group { segs: vec![], base });
        }
    };
    for tok in tokens {
        match tok {
            Token::Atom { rid } => {
                present_atoms.insert(rid.clone());
                key = format!("after:{rid}");
            }
            Token::Text { rid: Some(rid), text, fmt, .. } if info.text_runs.contains(rid) && own_get(rid).is_some() => {
                key = rid.clone();
                group(&key, Some(rid.clone()), &mut order, &mut groups);
                groups.get_mut(&key).unwrap().segs.push(Seg { text: text.clone(), br: false, fmt: fmt.clone() });
            }
            Token::Text { base, text, fmt, .. } => {
                group(&key, base.clone(), &mut order, &mut groups);
                let g = groups.get_mut(&key).unwrap();
                if g.base.is_none() {
                    g.base = base.clone();
                }
                g.segs.push(Seg { text: text.clone(), br: false, fmt: fmt.clone() });
            }
            Token::Br { base } => {
                group(&key, base.clone(), &mut order, &mut groups);
                let g = groups.get_mut(&key).unwrap();
                if g.base.is_none() {
                    g.base = base.clone();
                }
                g.segs.push(Seg { text: String::new(), br: true, fmt: None });
            }
        }
    }
    // Neighbouring text segments with the same formatting become one.
    for g in groups.values_mut() {
        let mut out: Vec<Seg> = vec![];
        for s in g.segs.drain(..) {
            match out.last_mut() {
                Some(prev) if !prev.br && !s.br && fmt_key(&prev.fmt) == fmt_key(&s.fmt) => prev.text.push_str(&s.text),
                _ => out.push(s),
            }
        }
        g.segs = out;
    }

    let first_text_rid = info.text_runs.iter().find(|rid| own_get(rid).is_some()).cloned();
    let paragraph_mark_rpr = |t: &Tree| -> Option<String> {
        let ppr = t.elements(p).into_iter().find(|&k| cx.is_w(t, k, "pPr"))?;
        let rpr = t.elements(ppr).into_iter().find(|&k| cx.is_w(t, k, "rPr"))?;
        let mut copy = Tree::parse(&t.serialize_node(rpr)).ok()?;
        for k in copy.elements(copy.root) {
            if matches!(copy.local_name(k), Some("ins" | "del" | "moveFrom" | "moveTo" | "rPrChange")) {
                copy.detach(k);
            }
        }
        Some(copy.serialize())
    };

    let insert_clones = |t: &mut Tree, segs: &[Seg], template: Option<&str>, reference: Id, before: bool| {
        let mut reference = reference;
        for seg in segs {
            let r = cx.build_run(t, template, seg);
            t.set_attr(r, "data-bgen", "1");
            if before {
                t.insert_before(reference, r);
            } else {
                t.insert_after(reference, r);
                reference = r;
            }
        }
    };

    // Text runs: the first segment replaces the run's content, the rest follow it.
    for rid in info.text_runs.clone() {
        let Some(r) = own_get(&rid) else { continue };
        let empty = Seg { text: String::new(), br: false, fmt: None };
        let segs: Vec<Seg> = groups.get(&rid).map(|g| g.segs.clone()).unwrap_or_default();
        let first = segs.first().cloned().unwrap_or(empty);
        let template = tmpl(&t, Some(&rid));
        let fresh = cx.build_run(&mut t, template.as_deref(), &first);
        for c in t.elements(r) {
            t.detach(c);
        }
        for c in t.children(fresh).to_vec() {
            t.append(r, c);
        }
        if segs.len() > 1 {
            insert_clones(&mut t, &segs[1..], template.as_deref(), r, false);
        }
    }

    let mut template_for = |t: &Tree, g: &Group| -> Option<String> {
        let from_base = match &g.base {
            Some(b) if own_get(b).is_some() => tmpl(t, Some(b)),
            _ => None,
        };
        from_base.or_else(|| match &first_text_rid {
            Some(rid) => tmpl(t, Some(rid)),
            None => paragraph_mark_rpr(t),
        })
    };

    for k in &order {
        let Some(rid) = k.strip_prefix("after:") else { continue };
        let Some(atom) = own_get(rid) else { continue };
        let g = &groups[k];
        let template = template_for(&t, g);
        insert_clones(&mut t, &g.segs, template.as_deref(), atom, false);
    }
    if let Some(lead) = groups.get("^").filter(|g| !g.segs.is_empty()) {
        let template = template_for(&t, lead);
        let first_run = own
            .iter()
            .map(|(_, r)| *r)
            .find(|&r| !t.ancestors(r).any(|a| cx.is_w(&t, a, "del") || cx.is_w(&t, a, "moveFrom")));
        match first_run {
            Some(fr) => insert_clones(&mut t, &lead.segs, template.as_deref(), fr, true),
            None => {
                let holder = t.new_element(&cx.wname("r"), &[]);
                t.append(p, holder);
                insert_clones(&mut t, &lead.segs, template.as_deref(), holder, false);
                t.detach(holder);
            }
        }
    }
    // Breaks and tabs the translator deleted.
    for rid in info.atoms.clone() {
        if !present_atoms.contains(&rid) {
            if let Some(r) = own_get(&rid) {
                t.detach(r);
            }
            info.atoms.remove(&rid);
        }
    }
    Ok(t.serialize())
}
