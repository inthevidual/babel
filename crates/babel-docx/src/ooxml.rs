//! The rest of web/js/ooxml.js that editing needs, in Rust: whole parts,
//! normalisation and ids, paragraph text, links. Each function follows its
//! JavaScript namesake; tests/ooxml_parity.rs holds them to it.

use std::collections::{HashMap, HashSet};

use crate::tree::{Id, Kind, Tree, XmlError};
use crate::writeback::W;

const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const PKG_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
pub const REL_HYPERLINK: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";

/// A whole XML part: the declaration it had, and its tree.
pub struct Part {
    pub decl: String,
    pub tree: Tree,
}

impl Part {
    pub fn parse(xml: &str) -> Result<Part, XmlError> {
        let s = xml.trim_start_matches('\u{feff}').trim_start();
        let decl = if s.starts_with("<?xml") {
            s[..s.find("?>").map(|i| i + 2).unwrap_or(0)].to_owned()
        } else {
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#.to_owned()
        };
        Ok(Part { decl, tree: Tree::parse(xml)? })
    }

    /// As `serializeXml`: the original declaration, CRLF, the document.
    pub fn serialize(&self) -> String {
        format!("{}\r\n{}", self.decl, self.tree.serialize())
    }
}

/// Prefixes bound to a namespace on the root element.
fn prefix_of(t: &Tree, ns: &str, default: &str) -> String {
    if let Kind::Element { attrs, .. } = t.kind(t.root) {
        for (k, v) in attrs {
            if v == ns {
                if let Some(p) = k.strip_prefix("xmlns:") {
                    return p.to_owned();
                }
            }
        }
    }
    default.to_owned()
}

/// Element-name checks with the document's own prefixes.
pub struct Names {
    pub w: String,
    pub mc: String,
}

impl Names {
    pub fn of(t: &Tree) -> Names {
        Names { w: prefix_of(t, W, "w"), mc: prefix_of(t, MC, "mc") }
    }
    pub fn is_w(&self, t: &Tree, id: Id, local: &str) -> bool {
        t.name(id).and_then(|n| n.strip_prefix(self.w.as_str())).and_then(|n| n.strip_prefix(':')) == Some(local)
    }
    fn is_fallback(&self, t: &Tree, id: Id) -> bool {
        t.name(id).and_then(|n| n.strip_prefix(self.mc.as_str())).and_then(|n| n.strip_prefix(':')) == Some("Fallback")
    }
    pub fn wn(&self, local: &str) -> String {
        format!("{}:{local}", self.w)
    }
    fn owner_para(&self, t: &Tree, id: Id) -> Option<Id> {
        t.ancestors(id).find(|&a| self.is_w(t, a, "p"))
    }
}

/// `w:` elements named `local` under `root` (or the root itself), in document order.
fn all(t: &Tree, n: &Names, root: Id, local: &str) -> Vec<Id> {
    let mut out: Vec<Id> = vec![];
    if n.is_w(t, root, local) {
        out.push(root);
    }
    out.extend(t.descendants(root).into_iter().filter(|&e| n.is_w(t, e, local)));
    out
}

// ── Normalising and ids ────────────────────────────────────────────────────

/// `normalize`: every `w:r` holds exactly one content element, then ids.
pub fn normalize(t: &mut Tree, key: &str) {
    let n = Names::of(t);
    for r in all(t, &n, t.root, "r") {
        let kids = t.elements(r);
        let rpr = kids.iter().copied().find(|&k| n.is_w(t, k, "rPr"));
        let content: Vec<Id> = kids.into_iter().filter(|&k| Some(k) != rpr).collect();
        let mut reference = r;
        for &c in content.iter().skip(1) {
            let nr = t.shallow_clone(r);
            t.remove_attr(nr, "data-br");
            if let Some(rp) = rpr {
                let copy = t.deep_clone(rp);
                t.append(nr, copy);
            }
            t.append(nr, c);
            t.insert_after(reference, nr);
            reference = nr;
        }
    }
    ensure_ids(t, key);
}

/// `ensureIds`: ids for paragraphs and runs that lack one (runs Babel
/// generated become first-class runs), never renumbering existing ones; then
/// fields are marked.
pub fn ensure_ids(t: &mut Tree, key: &str) {
    let n = Names::of(t);
    let ps = all(t, &n, t.root, "p");
    let rs = all(t, &n, t.root, "r");
    let mut used: HashSet<String> = HashSet::new();
    for &p in &ps {
        if let Some(v) = t.attr(p, "data-bp").filter(|v| !v.is_empty()) {
            used.insert(v.to_owned());
        }
    }
    for &r in &rs {
        if let Some(v) = t.attr(r, "data-br").filter(|v| !v.is_empty()) {
            used.insert(v.to_owned());
        }
    }
    let mut i = 0;
    for &p in &ps {
        if t.attr(p, "data-bp").is_some_and(|v| !v.is_empty()) {
            continue;
        }
        while used.contains(&format!("{key}:{i}")) {
            i += 1;
        }
        let id = format!("{key}:{i}");
        t.set_attr(p, "data-bp", &id);
        used.insert(id);
    }
    let mut i = 0;
    for &r in &rs {
        t.remove_attr(r, "data-bgen");
        if t.attr(r, "data-br").is_some_and(|v| !v.is_empty()) {
            continue;
        }
        while used.contains(&format!("{key}:r{i}")) {
            i += 1;
        }
        let id = format!("{key}:r{i}");
        t.set_attr(r, "data-br", &id);
        used.insert(id);
    }
    mark_fields(t, &n);
}

fn field_kind(instr: &str) -> Option<&'static str> {
    let s = instr.trim_start().to_ascii_uppercase();
    let word = s.split(|c: char| !c.is_ascii_alphanumeric()).next().unwrap_or("");
    match word {
        "PAGE" => Some("page"),
        "NUMPAGES" | "SECTIONPAGES" => Some("numpages"),
        _ => None,
    }
}

/// `markFields`: runs of field results are marked `data-bk` (not translatable;
/// PAGE and NUMPAGES so pages can show their numbers).
fn mark_fields(t: &mut Tree, n: &Names) {
    for f in all(t, n, t.root, "fldSimple") {
        let k = field_kind(t.attr(f, &n.wn("instr")).unwrap_or("")).unwrap_or("a");
        for r in all(t, n, f, "r") {
            t.set_attr(r, "data-bk", k);
        }
    }
    for p in all(t, n, t.root, "p") {
        struct F {
            instr: String,
            result: bool,
        }
        let mut stack: Vec<F> = vec![];
        for r in all(t, n, p, "r") {
            if n.owner_para(t, r) != Some(p) {
                continue;
            }
            let kids = t.elements(r);
            let fc = kids.iter().copied().find(|&k| n.is_w(t, k, "fldChar"));
            let ty = fc.and_then(|f| t.attr(f, &n.wn("fldCharType"))).map(str::to_owned);
            match ty.as_deref() {
                Some("begin") => {
                    stack.push(F { instr: String::new(), result: false });
                    continue;
                }
                Some("separate") => {
                    if let Some(f) = stack.last_mut() {
                        f.result = true;
                    }
                    continue;
                }
                Some("end") => {
                    stack.pop();
                    continue;
                }
                _ => {}
            }
            let Some(top) = stack.last_mut() else { continue };
            let it = kids.iter().copied().find(|&k| n.is_w(t, k, "instrText"));
            match it {
                Some(it) if !top.result => {
                    let s = t.text_content(it);
                    top.instr.push_str(&s);
                }
                _ if top.result => {
                    if let Some(k) = field_kind(&top.instr) {
                        t.set_attr(r, "data-bk", k);
                    }
                }
                _ => {}
            }
        }
    }
}

// ── Text ───────────────────────────────────────────────────────────────────

/// `paraText`: the paragraph's own text (not nested text-box paragraphs, not
/// deleted text, not field codes), tabs and breaks as `\t` and `\n`.
pub fn para_text(t: &Tree, p: Id) -> String {
    let n = Names::of(t);
    let mut out = String::new();
    fn walk(t: &Tree, n: &Names, el: Id, out: &mut String) {
        for c in t.elements(el) {
            if n.is_fallback(t, c) {
                continue;
            }
            let Some(name) = t.name(c) else { continue };
            let Some(local) = name.strip_prefix(n.w.as_str()).and_then(|x| x.strip_prefix(':')) else {
                walk(t, n, c, out);
                continue;
            };
            match local {
                "p" => {}
                "t" => out.push_str(&t.text_content(c)),
                "tab" | "ptab" => out.push('\t'),
                "br" | "cr" => {
                    if t.attr(c, &n.wn("type")).unwrap_or("textWrapping") == "textWrapping" {
                        out.push('\n');
                    }
                }
                "noBreakHyphen" => out.push('\u{2011}'),
                "sym" => {
                    let code = t.attr(c, &n.wn("char")).and_then(|h| u32::from_str_radix(h, 16).ok()).filter(|c| *c != 0);
                    // JavaScript's String.fromCharCode takes the low 16 bits.
                    out.push(code.and_then(|c| char::from_u32(c & 0xFFFF)).unwrap_or('\u{25A1}'));
                }
                "del" | "delText" | "instrText" | "moveFrom" => {}
                _ => walk(t, n, c, out),
            }
        }
    }
    walk(t, &n, p, &mut out);
    out
}

/// `paragraphs`: the paragraphs a translator works on (not Fallback copies).
pub fn paragraphs(t: &Tree) -> Vec<Id> {
    let n = Names::of(t);
    all(t, &n, t.root, "p").into_iter().filter(|&p| !t.ancestors(p).any(|a| n.is_fallback(t, a))).collect()
}

/// Word's "Characters (with spaces)": text only, no tabs or paragraph marks.
pub fn count_chars(text: &str) -> usize {
    text.chars().filter(|c| *c != '\t' && *c != '\n').count()
}

/// `countWords`: runs of anything but white space and non-breaking hyphens.
pub fn count_words(text: &str) -> usize {
    text.split(|c: char| c.is_whitespace() || c == '\u{2011}').filter(|w| !w.is_empty()).count()
}

// ── Relationships and links ────────────────────────────────────────────────

/// `relsPathFor`: word/document.xml → word/_rels/document.xml.rels.
pub fn rels_path_for(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => format!("{}_rels/{}.rels", &path[..=i], &path[i + 1..]),
        None => format!("_rels/{path}.rels"),
    }
}

/// `relsMap`: relationship id → target.
pub fn rels_map(rels_xml: Option<&str>) -> HashMap<String, String> {
    let mut m = HashMap::new();
    let Some(xml) = rels_xml else { return m };
    let Ok(t) = Tree::parse(xml) else { return m };
    for r in t.descendants(t.root) {
        if t.local_name(r) == Some("Relationship") {
            if let (Some(id), Some(target)) = (t.attr(r, "Id"), t.attr(r, "Target")) {
                m.insert(id.to_owned(), target.to_owned());
            }
        }
    }
    m
}

/// An empty relationships part.
pub fn empty_rels() -> String {
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>{}<Relationships xmlns="{PKG_REL}"/>"#, "\r\n")
}

/// `addLinkRel`: the id of an external hyperlink relationship to `url`, added
/// to `rels` (a whole part) if there is none yet.
pub fn add_link_rel(rels: &mut Part, url: &str) -> String {
    let t = &mut rels.tree;
    let rs: Vec<Id> = t.descendants(t.root).into_iter().filter(|&r| t.local_name(r) == Some("Relationship")).collect();
    for &r in &rs {
        if t.attr(r, "Type") == Some(REL_HYPERLINK) && t.attr(r, "Target") == Some(url) && t.attr(r, "TargetMode") == Some("External") {
            return t.attr(r, "Id").unwrap_or("").to_owned();
        }
    }
    let ids: HashSet<String> = rs.iter().filter_map(|&r| t.attr(r, "Id").map(str::to_owned)).collect();
    let mut i = 1;
    while ids.contains(&format!("rIdBabel{i}")) {
        i += 1;
    }
    let id = format!("rIdBabel{i}");
    // Same prefix as the root (Relationships parts normally use a default namespace).
    let name = match t.name(t.root).and_then(|n| n.split_once(':')) {
        Some((p, _)) => format!("{p}:Relationship"),
        None => "Relationship".to_owned(),
    };
    let el = t.new_element(&name, &[("Id", &id), ("Type", REL_HYPERLINK), ("Target", url), ("TargetMode", "External")]);
    let root = t.root;
    t.append(root, el);
    id
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub anchor: Option<String>,
}

impl Link {
    /// How it reads in messages: the address, or the bookmark with `#`.
    pub fn label(&self) -> String {
        match (&self.url, &self.anchor) {
            (Some(u), _) => u.clone(),
            (None, Some(a)) => format!("#{a}"),
            _ => String::new(),
        }
    }
}

/// `parseHyperlinkInstr`: the link of a HYPERLINK field's instruction.
fn parse_hyperlink_instr(instr: &str) -> Option<Link> {
    let s = instr.trim_start();
    if !s.get(..9).is_some_and(|h| h.eq_ignore_ascii_case("HYPERLINK")) {
        return None;
    }
    let rest = &s[9..];
    if rest.chars().next().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    // \l "anchor"
    let anchor = {
        let lower = rest.to_ascii_lowercase();
        lower.find("\\l").and_then(|i| {
            let after = rest[i + 2..].trim_start();
            let after = after.strip_prefix('"')?;
            Some(after[..after.find('"')?].to_owned())
        })
    };
    // Without the switches (with or without their quoted arguments).
    let mut plain = String::new();
    let mut it = rest.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if c == '\\' && rest[i + 1..].chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            it.next();
            let after = &rest[i + 2..];
            let trimmed = after.trim_start();
            if trimmed.starts_with('"') {
                let skip_ws = after.len() - trimmed.len();
                if let Some(close) = trimmed[1..].find('"') {
                    for _ in 0..(skip_ws + close + 2) {
                        it.next();
                    }
                    continue;
                }
            }
            continue;
        }
        plain.push(c);
    }
    let url = match plain.find('"') {
        Some(a) => plain[a + 1..].find('"').map(|b| plain[a + 1..a + 1 + b].to_owned()).unwrap_or_default(),
        None => plain.split_whitespace().next().unwrap_or("").to_owned(),
    };
    if !url.is_empty() {
        let url = match &anchor {
            Some(a) => format!("{url}#{a}"),
            None => url,
        };
        return Some(Link { url: Some(url), anchor: None });
    }
    anchor.map(|a| Link { url: None, anchor: Some(a) })
}

/// `linkOfRun`: the link a run is part of: a `w:hyperlink`, a HYPERLINK
/// `w:fldSimple` or the result of a complex HYPERLINK field.
pub fn link_of_run(t: &Tree, run: Id, rels: &HashMap<String, String>) -> Option<Link> {
    let n = Names::of(t);
    if let Some(h) = t.ancestors(run).find(|&a| n.is_w(t, a, "hyperlink")) {
        let id = t.attr(h, "r:id");
        let anchor = t.attr(h, &n.wn("anchor")).map(str::to_owned);
        if let Some(target) = id.and_then(|i| rels.get(i)) {
            let url = match &anchor {
                Some(a) if !a.is_empty() => format!("{target}#{a}"),
                _ => target.clone(),
            };
            return Some(Link { url: Some(url), anchor: None });
        }
        return anchor.filter(|a| !a.is_empty()).map(|a| Link { url: None, anchor: Some(a) });
    }
    if let Some(fs) = t.ancestors(run).find(|&a| n.is_w(t, a, "fldSimple")) {
        return parse_hyperlink_instr(t.attr(fs, &n.wn("instr")).unwrap_or(""));
    }
    let p = n.owner_para(t, run)?;
    struct F {
        instr: String,
        result: bool,
    }
    let mut stack: Vec<F> = vec![];
    for r in all(t, &n, p, "r") {
        if n.owner_para(t, r) != Some(p) {
            continue;
        }
        if r == run {
            break;
        }
        let kids = t.elements(r);
        let ty = kids.iter().copied().find(|&k| n.is_w(t, k, "fldChar")).and_then(|f| t.attr(f, &n.wn("fldCharType")));
        match ty {
            Some("begin") => stack.push(F { instr: String::new(), result: false }),
            Some("separate") => {
                if let Some(f) = stack.last_mut() {
                    f.result = true;
                }
            }
            Some("end") => {
                stack.pop();
            }
            _ => {
                if let Some(it) = kids.iter().copied().find(|&k| n.is_w(t, k, "instrText")) {
                    if let Some(f) = stack.last_mut().filter(|f| !f.result) {
                        let s = t.text_content(it);
                        f.instr.push_str(&s);
                    }
                }
            }
        }
    }
    stack.iter().rev().filter(|f| f.result).find_map(|f| parse_hyperlink_instr(&f.instr))
}

/// One `w:rPr` child: its local name and its `w:` attributes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StyleBit {
    pub name: String,
    pub attrs: Vec<(String, String)>,
}

/// `linkStyleOf`: the parts of a run's formatting that make it look like a link.
pub fn link_style_of(t: &Tree, run: Id) -> Vec<StyleBit> {
    let n = Names::of(t);
    let Some(rpr) = t.elements(run).into_iter().find(|&k| n.is_w(t, k, "rPr")) else { return vec![] };
    t.elements(rpr)
        .into_iter()
        .filter(|&k| ["rStyle", "color", "u"].iter().any(|l| n.is_w(t, k, l)))
        .map(|k| {
            let attrs = match t.kind(k) {
                Kind::Element { attrs, .. } => attrs
                    .iter()
                    .filter_map(|(a, v)| a.strip_prefix(&format!("{}:", n.w)).map(|l| (l.to_owned(), v.clone())))
                    .collect(),
                _ => vec![],
            };
            StyleBit { name: t.local_name(k).unwrap_or("").to_owned(), attrs }
        })
        .collect()
}

/// The style a pasted link gets when the copied one had none: the document's
/// Hyperlink character style if it has one, else blue and underlined.
pub fn default_link_style(styles_xml: Option<&str>) -> Vec<StyleBit> {
    let has = styles_xml.is_some_and(|x| {
        Tree::parse(x).is_ok_and(|t| {
            let n = Names::of(&t);
            all(&t, &n, t.root, "style").into_iter().any(|s| t.attr(s, &n.wn("styleId")) == Some("Hyperlink") && t.attr(s, &n.wn("type")) == Some("character"))
        })
    });
    let bit = |name: &str, val: &str| StyleBit { name: name.into(), attrs: vec![("val".into(), val.into())] };
    if has { vec![bit("rStyle", "Hyperlink")] } else { vec![bit("color", "0563C1"), bit("u", "single")] }
}

struct RunOff {
    run: Id,
    start: usize,
    end: usize,
    text: Option<String>,
}

fn inside_skipped(t: &Tree, n: &Names, el: Id, stop: Id) -> bool {
    for a in t.ancestors(el) {
        if a == stop {
            return false;
        }
        if n.is_w(t, a, "del") || n.is_w(t, a, "moveFrom") || n.is_fallback(t, a) {
            return true;
        }
    }
    false
}

/// `runOffsets`: the paragraph's runs with where each one's text sits in the
/// text the editor shows (text, tabs and line breaks; byte offsets here).
fn run_offsets(t: &Tree, n: &Names, p: Id) -> (Vec<RunOff>, String) {
    let mut runs = vec![];
    let mut text = String::new();
    for r in all(t, n, p, "r") {
        if n.owner_para(t, r) != Some(p) || inside_skipped(t, n, r, p) {
            continue;
        }
        let c: Vec<Id> = t.elements(r).into_iter().filter(|&k| !n.is_w(t, k, "rPr")).collect();
        let (mut piece, mut tx) = (String::new(), None);
        if t.attr(r, "data-bk").is_none() && c.len() == 1 {
            let k = c[0];
            if n.is_w(t, k, "t") {
                piece = t.text_content(k);
                tx = Some(piece.clone());
            } else if n.is_w(t, k, "tab") {
                piece = "\t".into();
            } else if n.is_w(t, k, "br") && t.attr(k, &n.wn("type")).unwrap_or("textWrapping") == "textWrapping" {
                piece = "\n".into();
            }
        }
        runs.push(RunOff { run: r, start: text.len(), end: text.len() + piece.len(), text: tx });
        text.push_str(&piece);
    }
    (runs, text)
}

/// The paragraph's text as `runOffsets` sees it.
pub fn run_text(t: &Tree, p: Id) -> String {
    run_offsets(t, &Names::of(t), p).1
}

fn set_run_text(t: &mut Tree, n: &Names, r: Id, text: &str) {
    let Some(el) = t.elements(r).into_iter().find(|&k| n.is_w(t, k, "t")) else { return };
    for c in t.children(el).to_vec() {
        t.detach(c);
    }
    if !text.is_empty() {
        let tx = t.new_text(text);
        t.append(el, tx);
    }
    t.set_attr(el, "xml:space", "preserve");
}

/// Where a link points: a relationship (external address) or a bookmark.
pub enum LinkTarget {
    Rel(String),
    Anchor(String),
}

/// `linkRange`: makes `start..end` of the paragraph's run text a hyperlink
/// with `style`; returns why not, if it can't.
pub fn link_range(t: &mut Tree, p: Id, start: usize, end: usize, target: &LinkTarget, style: &[StyleBit]) -> Option<&'static str> {
    if end <= start {
        return Some("Select the text to link first.");
    }
    let n = Names::of(t);
    let (mut runs, _) = run_offsets(t, &n, p);
    let split_at = |t: &mut Tree, runs: &mut Vec<RunOff>, pos: usize| {
        let Some(e) = runs.iter().find(|x| x.text.is_some() && x.start < pos && pos < x.end) else { return };
        let (run, text, s) = (e.run, e.text.clone().unwrap(), e.start);
        let clone = t.deep_clone(run);
        t.remove_attr(clone, "data-br");
        t.remove_attr(clone, "data-bgen");
        set_run_text(t, &n, run, &text[..pos - s]);
        set_run_text(t, &n, clone, &text[pos - s..]);
        t.insert_after(run, clone);
        *runs = run_offsets(t, &n, p).0;
    };
    split_at(t, &mut runs, end);
    split_at(t, &mut runs, start);
    let covered: Vec<Id> = runs.iter().filter(|x| x.start >= start && x.end <= end && x.end > x.start).map(|x| x.run).collect();
    let (Some(&first), Some(&last)) = (covered.first(), covered.last()) else { return Some("There is no text in the selection to link.") };
    let parent = t.parent(first).expect("a run has a parent");
    const CROSSES: &str = "The selection crosses a link, field or tracked change. Select text inside one of them.";
    if t.parent(last) != Some(parent) {
        return Some(CROSSES);
    }
    let h = if n.is_w(t, parent, "hyperlink") {
        // Already a link: give it the new address.
        t.remove_attr(parent, "r:id");
        t.remove_attr(parent, &n.wn("anchor"));
        parent
    } else {
        if !["p", "ins", "smartTag", "sdtContent", "customXml"].iter().any(|l| n.is_w(t, parent, l)) {
            return Some(CROSSES);
        }
        let kids = t.children(parent).to_vec();
        let a = kids.iter().position(|&k| k == first).expect("first is a child");
        let b = kids.iter().position(|&k| k == last).expect("last is a child");
        let nodes: Vec<Id> = kids[a..=b].to_vec();
        let bad = nodes.iter().any(|&k| {
            n.is_w(t, k, "hyperlink") || n.is_w(t, k, "fldSimple") || (n.is_w(t, k, "r") && t.elements(k).iter().any(|&c| n.is_w(t, c, "fldChar") || n.is_w(t, c, "instrText")))
        });
        if bad {
            return Some(CROSSES);
        }
        let h = t.new_element(&n.wn("hyperlink"), &[]);
        t.insert_before(first, h);
        for k in nodes {
            t.append(h, k);
        }
        h
    };
    match target {
        LinkTarget::Rel(id) => {
            t.set_attr(h, "r:id", id);
            t.set_attr(h, &n.wn("history"), "1");
        }
        LinkTarget::Anchor(a) => t.set_attr(h, &n.wn("anchor"), a),
    }
    let cx = crate::writeback::RprEditor::new(&n.w);
    for r in all(t, &n, h, "r") {
        if !t.elements(r).iter().any(|&k| n.is_w(t, k, "t")) {
            continue;
        }
        let rpr = match t.elements(r).into_iter().find(|&k| n.is_w(t, k, "rPr")) {
            Some(rp) => rp,
            None => {
                let rp = t.new_element(&n.wn("rPr"), &[]);
                match t.children(r).first().copied() {
                    Some(f) => t.insert_before(f, rp),
                    None => t.append(r, rp),
                }
                rp
            }
        };
        for bit in style {
            let attrs: Vec<(&str, &str)> = bit.attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            cx.set_rpr_child(t, rpr, &bit.name, &attrs);
        }
    }
    None
}

/// The `w:p` with `data-bp` = `pid` in a parsed part.
pub fn find(t: &Tree, pid: &str) -> Option<Id> {
    let n = Names::of(t);
    all(t, &n, t.root, "p").into_iter().find(|&p| t.attr(p, "data-bp") == Some(pid))
}

/// The run with `data-br` = `rid`.
pub fn find_run(t: &Tree, rid: &str) -> Option<Id> {
    let n = Names::of(t);
    all(t, &n, t.root, "r").into_iter().find(|&r| t.attr(r, "data-br") == Some(rid))
}
