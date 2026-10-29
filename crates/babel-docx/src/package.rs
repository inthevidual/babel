//! The .docx package: opening it (`openPackage`), importing it as a new
//! translation (normalised text parts), pairing it with an earlier
//! translation, and exporting the translated file (`buildDocx`): the
//! original zip with the translated text parts swapped in, Babel's ids and
//! proofing marks gone, runs merged again, the target language set. Every
//! other part is copied byte for byte. Each follows its namesake in
//! web/js/ooxml.js; tests/export_parity.rs holds them to it.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use crate::ooxml::{self, Names, Part};
use crate::tree::{Id, Tree};
use crate::writeback::RprEditor;

pub type Parts = BTreeMap<String, String>;

#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("This is not a Word document (.docx). Older .doc files must be saved as .docx in Word first.")]
    NotZip,
    #[error("The file is a zip archive but not a Word document.")]
    NoRootRels,
    #[error("No main document part was found in this file.")]
    NoMain,
    #[error("The main document part is missing.")]
    MainMissing,
    #[error("{0}")]
    Structure(String),
    #[error("{0}")]
    Other(String),
}

impl From<crate::tree::XmlError> for PackageError {
    fn from(e: crate::tree::XmlError) -> Self {
        PackageError::Other(e.to_string())
    }
}

/// `resolve`: a relationship target against the part it is relative to.
fn resolve(base: &str, target: &str) -> String {
    if let Some(t) = target.strip_prefix('/') {
        return t.to_owned();
    }
    let mut parts: Vec<&str> = base.split('/').collect();
    parts.pop();
    for seg in target.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// The parts of a package that matter to Babel.
#[derive(Debug, Clone)]
pub struct Package {
    pub main: String,
    /// The parts with translatable paragraphs: the main part first, then
    /// headers, footers, footnotes and endnotes.
    pub text_parts: Vec<String>,
    pub styles: Option<String>,
    pub settings: Option<String>,
}

fn read(z: &mut zip::ZipArchive<Cursor<&[u8]>>, path: &str) -> Option<String> {
    let mut f = z.by_name(path).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

fn has(z: &zip::ZipArchive<Cursor<&[u8]>>, path: &str) -> bool {
    z.index_for_name(path).is_some()
}

fn relationships(xml: &str) -> Vec<(String, String, Option<String>)> {
    let Ok(t) = Tree::parse(xml) else { return vec![] };
    t.descendants(t.root)
        .into_iter()
        .filter(|&r| t.local_name(r) == Some("Relationship"))
        .map(|r| (t.attr(r, "Type").unwrap_or("").to_owned(), t.attr(r, "Target").unwrap_or("").to_owned(), t.attr(r, "TargetMode").map(str::to_owned)))
        .collect()
}

/// `openPackage`: finds the main part and the text parts.
pub fn open_package(bytes: &[u8]) -> Result<Package, PackageError> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| PackageError::NotZip)?;
    let root = read(&mut z, "_rels/.rels").ok_or(PackageError::NoRootRels)?;
    let main = relationships(&root).into_iter().find(|(ty, _, _)| ty.ends_with("/officeDocument")).ok_or(PackageError::NoMain)?;
    let main = resolve("", &main.1);
    if !has(&z, &main) {
        return Err(PackageError::MainMissing);
    }
    let mut text_parts = vec![main.clone()];
    if let Some(rels) = read(&mut z, &ooxml::rels_path_for(&main)) {
        for (ty, target, mode) in relationships(&rels) {
            if mode.as_deref() == Some("External") {
                continue;
            }
            let kind = ty.rsplit('/').next().unwrap_or("");
            if !matches!(kind, "header" | "footer" | "footnotes" | "endnotes") {
                continue;
            }
            let p = resolve(&main, &target);
            if has(&z, &p) && !text_parts.contains(&p) {
                text_parts.push(p);
            }
        }
    }
    let styles = Some(resolve(&main, "styles.xml")).filter(|p| has(&z, p));
    let settings = Some(resolve(&main, "settings.xml")).filter(|p| has(&z, p));
    Ok(Package { main, text_parts, styles, settings })
}

/// The part key of a path (word/header1.xml → header1): the prefix of its ids.
pub fn part_key(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".xml").unwrap_or(name)
}

/// A .docx ready to become a translation: its text parts normalised.
pub struct Imported {
    pub package: Package,
    /// Text part path → normalised XML.
    pub parts: Parts,
}

/// Opens a .docx and normalises its text parts (as `startNew` does).
pub fn import(bytes: &[u8]) -> Result<Imported, PackageError> {
    let package = open_package(bytes)?;
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| PackageError::NotZip)?;
    let mut parts = Parts::new();
    for path in &package.text_parts {
        let orig = read(&mut z, path).ok_or_else(|| PackageError::Other(format!("{path} is unreadable")))?;
        let mut part = Part::parse(&orig)?;
        ooxml::normalize(&mut part.tree, part_key(path));
        parts.insert(path.clone(), part.serialize());
    }
    Ok(Imported { package, parts })
}

/// "Continue from a translated file": the translation's text parts,
/// normalised, if its paragraphs match the original's one for one.
pub fn pair(original: &Imported, translated: &[u8]) -> Result<Parts, PackageError> {
    let mut z = zip::ZipArchive::new(Cursor::new(translated)).map_err(|_| PackageError::NotZip)?;
    let mut out = Parts::new();
    for path in &original.package.text_parts {
        let orig = read(&mut z, path).ok_or_else(|| PackageError::Structure(format!("The translation has no {path}; it doesn’t share the original’s structure.")))?;
        let mut part = Part::parse(&orig)?;
        ooxml::normalize(&mut part.tree, part_key(path));
        let count = |t: &Tree| {
            let n = Names::of(t);
            t.descendants(t.root).into_iter().filter(|&e| n.is_w(t, e, "p")).count() + usize::from(n.is_w(t, t.root, "p"))
        };
        let a = count(&Part::parse(&original.parts[path])?.tree);
        let b = count(&part.tree);
        if a != b {
            return Err(PackageError::Structure(format!(
                "The translation’s structure differs from the original ({}: {a} vs {b} paragraphs), so they can’t be paired.",
                part_key(path)
            )));
        }
        out.insert(path.clone(), part.serialize());
    }
    Ok(out)
}

// ── Export ─────────────────────────────────────────────────────────────────

/// `dropOrphanLinks`: a hyperlink whose relationship is gone keeps its text, not the link.
fn drop_orphan_links(t: &mut Tree, n: &Names, rels: &std::collections::HashMap<String, String>) {
    let links: Vec<Id> = t.descendants(t.root).into_iter().filter(|&e| n.is_w(t, e, "hyperlink")).collect();
    for h in links {
        let Some(id) = t.attr(h, "r:id").filter(|i| !i.is_empty()).map(str::to_owned) else { continue };
        if rels.contains_key(&id) {
            continue;
        }
        for c in t.children(h).to_vec() {
            t.insert_before(h, c);
        }
        t.detach(h);
    }
}

/// `strip`: Babel's ids go, and the proofing marks of the source language.
fn strip(t: &mut Tree, n: &Names) {
    for e in t.descendants(t.root).into_iter().chain(std::iter::once(t.root)) {
        for a in ["data-bp", "data-br", "data-bk", "data-bgen"] {
            t.remove_attr(e, a);
        }
    }
    for e in t.descendants(t.root) {
        if n.is_w(t, e, "proofErr") {
            t.detach(e);
        }
    }
}

/// A run of one `w:t` (and its `w:rPr`): (rPr, t).
fn text_only(t: &Tree, n: &Names, r: Id) -> Option<(Option<Id>, Id)> {
    let kids = t.elements(r);
    let rpr = kids.iter().copied().find(|&k| n.is_w(t, k, "rPr"));
    let rest: Vec<Id> = kids.into_iter().filter(|&k| Some(k) != rpr).collect();
    (rest.len() == 1 && n.is_w(t, rest[0], "t")).then(|| (rpr, rest[0]))
}

fn set_text(t: &mut Tree, el: Id, text: &str) {
    for c in t.children(el).to_vec() {
        t.detach(c);
    }
    if !text.is_empty() {
        let tx = t.new_text(text);
        t.append(el, tx);
    }
}

/// `mergeRuns`: neighbouring text runs with the same formatting become one
/// (normalising split every run; Word would have them whole).
fn merge_runs(t: &mut Tree, n: &Names) {
    let ps: Vec<Id> = std::iter::once(t.root).chain(t.descendants(t.root)).filter(|&e| n.is_w(t, e, "p")).collect();
    for p in ps {
        let mut parents: Vec<Id> = vec![];
        for r in t.descendants(p).into_iter().filter(|&e| n.is_w(t, e, "r")) {
            if let Some(par) = t.parent(r) {
                if !parents.contains(&par) {
                    parents.push(par);
                }
            }
        }
        for parent in parents {
            let mut prev: Option<(Option<Id>, Id)> = None;
            let mut prev_key: Option<String> = None;
            for r in t.elements(parent) {
                let a = if n.is_w(t, r, "r") { text_only(t, n, r) } else { None };
                let k = a.map(|(rpr, _)| rpr.map(|rp| t.serialize_node(rp)).unwrap_or_default());
                if let (Some((_, at)), Some((_, pt))) = (a, prev) {
                    if k == prev_key {
                        let joined = format!("{}{}", t.text_content(pt), t.text_content(at));
                        set_text(t, pt, &joined);
                        t.detach(r);
                        continue;
                    }
                }
                prev = a;
                prev_key = k;
            }
            for r in t.elements(parent) {
                if !n.is_w(t, r, "r") {
                    continue;
                }
                if let Some((_, tx)) = text_only(t, n, r) {
                    let s = t.text_content(tx);
                    if s.starts_with(char::is_whitespace) || s.ends_with(char::is_whitespace) {
                        t.set_attr(tx, "xml:space", "preserve");
                    }
                }
            }
        }
    }
}

/// `setLanguage`: every run's language, and the document's default.
fn set_language(t: &mut Tree, n: &Names, lang: &str) {
    let val = n.wn("val");
    let langs: Vec<Id> = t.descendants(t.root).into_iter().filter(|&e| n.is_w(t, e, "lang")).collect();
    for l in langs {
        if t.parent(l).is_some_and(|p| n.is_w(t, p, "rPr")) {
            t.set_attr(l, &val, lang);
        }
    }
    let Some(defaults) = t.descendants(t.root).into_iter().find(|&e| n.is_w(t, e, "rPrDefault")) else { return };
    let rpr = match t.elements(defaults).into_iter().find(|&k| n.is_w(t, k, "rPr")) {
        Some(r) => r,
        None => {
            let r = t.new_element(&n.wn("rPr"), &[]);
            t.append(defaults, r);
            r
        }
    };
    match t.elements(rpr).into_iter().find(|&k| n.is_w(t, k, "lang")) {
        Some(l) => t.set_attr(l, &val, lang),
        None => RprEditor::new(&n.w).set_rpr_child(t, rpr, "lang", &[("val", lang)]),
    }
}

/// `buildDocx`: the translated file. `parts` are the translation's text
/// parts and any relationship parts Babel changed; `lang` the Word language code.
pub fn build_docx(original: &[u8], parts: &Parts, lang: Option<&str>) -> Result<Vec<u8>, PackageError> {
    let pkg = open_package(original)?;
    let mut z = zip::ZipArchive::new(Cursor::new(original)).map_err(|_| PackageError::NotZip)?;
    let mut out = Parts::new();
    for (path, xml) in parts {
        if !pkg.text_parts.contains(path) {
            out.insert(path.clone(), xml.clone()); // relationship parts
        }
    }
    for path in &pkg.text_parts {
        let orig = read(&mut z, path).ok_or_else(|| PackageError::Other(format!("{path} is unreadable")))?;
        let mut part = Part::parse(parts.get(path).unwrap_or(&orig))?;
        // The original's declaration, as serializeXml keeps it.
        part.decl = Part::parse(&orig)?.decl;
        let rels_path = ooxml::rels_path_for(path);
        let rels_xml = parts.get(&rels_path).cloned().or_else(|| read(&mut z, &rels_path));
        let rels = ooxml::rels_map(rels_xml.as_deref());
        let n = Names::of(&part.tree);
        drop_orphan_links(&mut part.tree, &n, &rels);
        strip(&mut part.tree, &n);
        merge_runs(&mut part.tree, &n);
        if let Some(l) = lang {
            set_language(&mut part.tree, &n, l);
        }
        out.insert(path.clone(), part.serialize());
    }
    if let (Some(l), Some(sp)) = (lang, &pkg.styles) {
        let orig = read(&mut z, sp).unwrap_or_default();
        let mut part = Part::parse(&orig)?;
        let n = Names::of(&part.tree);
        set_language(&mut part.tree, &n, l);
        out.insert(sp.clone(), part.serialize());
    }
    if let Some(sp) = &pkg.settings {
        let orig = read(&mut z, sp).unwrap_or_default();
        let mut part = Part::parse(&orig)?;
        let n = Names::of(&part.tree);
        for e in part.tree.descendants(part.tree.root) {
            if n.is_w(&part.tree, e, "proofState") {
                part.tree.detach(e);
            }
        }
        out.insert(sp.clone(), part.serialize());
    }
    write_zip(original, &out)
}

/// The original zip with `parts` in place of (or added to) its entries;
/// every other entry copied as it was, compressed bytes and all.
pub fn write_zip(original: &[u8], parts: &Parts) -> Result<Vec<u8>, PackageError> {
    let err = |e: zip::result::ZipError| PackageError::Other(e.to_string());
    let mut zin = zip::ZipArchive::new(Cursor::new(original)).map_err(|_| PackageError::NotZip)?;
    let mut zout = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).compression_level(Some(6));
    let mut written = std::collections::HashSet::new();
    for i in 0..zin.len() {
        let name = zin.by_index_raw(i).map_err(err)?.name().map_err(err)?.into_owned();
        match parts.get(&name) {
            Some(xml) => {
                zout.start_file(&name, opts).map_err(err)?;
                zout.write_all(xml.as_bytes()).map_err(|e| PackageError::Other(e.to_string()))?;
            }
            None => {
                let f = zin.by_index_raw(i).map_err(err)?;
                zout.raw_copy_file(f).map_err(err)?;
            }
        }
        written.insert(name);
    }
    for (name, xml) in parts {
        if !written.contains(name) {
            zout.start_file(name, opts).map_err(err)?;
            zout.write_all(xml.as_bytes()).map_err(|e| PackageError::Other(e.to_string()))?;
        }
    }
    Ok(zout.finish().map_err(err)?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_language_reaches_runs_and_defaults() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:b/><w:lang w:val="en-GB" w:eastAsia="ja-JP"/></w:rPr><w:t>x</w:t></w:r></w:p></w:body></w:document>"#;
        let mut part = Part::parse(xml).unwrap();
        let n = Names::of(&part.tree);
        set_language(&mut part.tree, &n, "sv-SE");
        let out = part.tree.serialize();
        assert!(out.contains(r#"<w:lang w:val="sv-SE" w:eastAsia="ja-JP"/>"#), "{out}");

        let styles = r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/></w:rPr></w:rPrDefault></w:docDefaults></w:styles>"#;
        let mut part = Part::parse(styles).unwrap();
        let n = Names::of(&part.tree);
        set_language(&mut part.tree, &n, "sv-SE");
        let out = part.tree.serialize();
        assert!(out.contains(r#"<w:sz w:val="22"/><w:lang w:val="sv-SE"/>"#), "the default gets a language, in schema order: {out}");
    }

    #[test]
    fn merging_keeps_differently_formatted_runs_apart() {
        let xml = r#"<w:p xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:r><w:t>a</w:t></w:r><w:r><w:t xml:space="preserve"> b</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>c</w:t></w:r><w:bookmarkStart/><w:r><w:t>d</w:t></w:r></w:p>"#;
        let mut t = Tree::parse(xml).unwrap();
        let n = Names::of(&t);
        merge_runs(&mut t, &n);
        let out = t.serialize();
        assert!(out.contains("<w:t>a b</w:t>"), "merged; no xml:space needed inside: {out}");
        assert!(out.contains("<w:b/></w:rPr><w:t>c</w:t>") && out.contains("<w:bookmarkStart/><w:r><w:t>d</w:t>"), "{out}");
    }
}
