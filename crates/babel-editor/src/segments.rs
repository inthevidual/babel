//! Segment files: a translation made elsewhere (by Claude, with the
//! babel-translate skill), paragraph by paragraph, in plain Markdown.
//!
//! ```text
//! ---
//! babel-segments: 1
//! document: Report.docx
//! target-language: sv-SE
//! ---
//!
//! ## 1 · document:2
//! > This report is **confidential**.{1}
//!
//! Denna rapport är **konfidentiell**.{1}
//! ```
//!
//! Each segment is one paragraph of the original with a translatable letter
//! in it: `## N` (and Babel's paragraph id, when Babel wrote the file), the
//! original quoted with `>`, and the translation on the lines below. A new
//! line is a line break within the paragraph. `{1}`, `{2}`… stand for what
//! can't be typed (footnote marks, pictures, fields, symbols), `{tab}` for a
//! tab; `**bold**` and `*italic*` are the original's direct formatting, and
//! `[…]` a link's text.
//!
//! Segments are matched to paragraphs by their text (the id, when there is
//! one, settles which of several), so the file works for the document it was
//! made from whether Babel or the skill's script wrote it.
//!
//! The walk over a paragraph here is also in the skill's script
//! (skills/babel-translate/scripts/babel_segments.py); keep the two alike.

use babel_docx::tree::{Id, Tree};

/// One paragraph's segment.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub n: usize,
    pub id: Option<String>,
    /// The original, as markup.
    pub source: String,
    /// The translation, as markup ("" when not translated yet).
    pub target: String,
}

/// Reads a segment file.
pub fn parse(md: &str) -> Result<Vec<Segment>, String> {
    let mut lines = md.lines().peekable();
    // Front matter.
    if lines.peek().is_some_and(|l| l.trim() == "---") {
        lines.next();
        for l in lines.by_ref() {
            if l.trim() == "---" {
                break;
            }
        }
    }
    let mut out: Vec<Segment> = vec![];
    let mut cur: Option<(Segment, bool, Vec<String>, Vec<String>)> = None;
    let finish = |c: Option<(Segment, bool, Vec<String>, Vec<String>)>, out: &mut Vec<Segment>| {
        if let Some((mut s, _, src, mut tgt)) = c {
            while tgt.last().is_some_and(|l| l.is_empty()) {
                tgt.pop();
            }
            s.source = src.join("\n");
            s.target = tgt.join("\n");
            out.push(s);
        }
    };
    let mut in_comment = false;
    for line in lines {
        let l = line.trim_end();
        if in_comment {
            if l.contains("-->") {
                in_comment = false;
            }
            continue;
        }
        if l.trim_start().starts_with("<!--") {
            in_comment = !l.contains("-->");
            continue;
        }
        if let Some((n, id)) = heading(l) {
            finish(cur.take(), &mut out);
            cur = Some((Segment { n, id, source: String::new(), target: String::new() }, false, vec![], vec![]));
            continue;
        }
        let Some((_, past_source, src, tgt)) = cur.as_mut() else { continue };
        if !*past_source && let Some(q) = l.strip_prefix('>') {
            src.push(q.strip_prefix(' ').unwrap_or(q).to_owned());
            continue;
        }
        if !src.is_empty() || !l.trim().is_empty() {
            *past_source = true;
        }
        // Blank lines before the translation separate it; within it, they are empty lines.
        if *past_source && (!l.trim().is_empty() || !tgt.is_empty()) {
            tgt.push(l.trim().to_owned());
        }
    }
    finish(cur, &mut out);
    if out.is_empty() {
        return Err("That file has no segments (headings like “## 1” with the original quoted below).".into());
    }
    Ok(out)
}

/// `## 12` or `## 12 · document:4`.
fn heading(l: &str) -> Option<(usize, Option<String>)> {
    let rest = l.strip_prefix("## ")?.trim();
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let n = digits.parse().ok()?;
    let tail = rest[digits.len()..].trim().trim_start_matches(['·', '|', '-', ':']).trim();
    let id = (!tail.is_empty() && !tail.contains(char::is_whitespace)).then(|| tail.to_owned());
    if !rest[digits.len()..].is_empty() && !rest[digits.len()..].starts_with(char::is_whitespace) {
        return None;
    }
    Some((n, id))
}

/// A piece of markup.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    /// `l`: a link's text (`[…]`).
    Text { s: String, b: bool, i: bool, l: bool },
    /// `{n}`: the n-th object (from 1).
    Obj(usize),
    /// `{tab}`.
    Tab,
}

/// Markup to text with formatting and objects.
pub fn tokens(markup: &str) -> Vec<Tok> {
    let chars: Vec<char> = markup.chars().collect();
    let (mut b, mut i, mut l) = (false, false, false);
    let mut out: Vec<Tok> = vec![];
    let mut text = String::new();
    let flush = |text: &mut String, out: &mut Vec<Tok>, b: bool, i: bool, l: bool| {
        if !text.is_empty() {
            out.push(Tok::Text { s: std::mem::take(text), b, i, l });
        }
    };
    let mut k = 0;
    while k < chars.len() {
        let c = chars[k];
        match c {
            '\\' if k + 1 < chars.len() => {
                text.push(chars[k + 1]);
                k += 2;
            }
            '*' => {
                let mut run = 0;
                while k + run < chars.len() && chars[k + run] == '*' {
                    run += 1;
                }
                let before = k.checked_sub(1).map(|j| chars[j]);
                let after = chars.get(k + run).copied();
                // A star between spaces is just a star ("5 * 3").
                let space = |c: Option<char>| c.is_none_or(char::is_whitespace);
                if run == 1 && space(before) && space(after) {
                    text.push('*');
                    k += 1;
                    continue;
                }
                flush(&mut text, &mut out, b, i, l);
                let mut left = run;
                while left > 0 {
                    if left >= 3 {
                        b = !b;
                        i = !i;
                        left -= 3;
                    } else if left == 2 {
                        b = !b;
                        left = 0;
                    } else {
                        i = !i;
                        left = 0;
                    }
                }
                k += run;
            }
            '{' => {
                let close = chars[k + 1..].iter().position(|&c| c == '}').map(|p| k + 1 + p);
                let inner: Option<String> = close.map(|e| chars[k + 1..e].iter().collect());
                match inner.as_deref() {
                    Some("tab") => {
                        flush(&mut text, &mut out, b, i, l);
                        out.push(Tok::Tab);
                        k = close.unwrap() + 1;
                    }
                    Some(d) if !d.is_empty() && d.len() <= 4 && d.chars().all(|c| c.is_ascii_digit()) => {
                        flush(&mut text, &mut out, b, i, l);
                        out.push(Tok::Obj(d.parse().unwrap()));
                        k = close.unwrap() + 1;
                    }
                    _ => {
                        text.push('{');
                        k += 1;
                    }
                }
            }
            '[' | ']' => {
                flush(&mut text, &mut out, b, i, l);
                l = c == '[';
                k += 1;
            }
            _ => {
                text.push(c);
                k += 1;
            }
        }
    }
    flush(&mut text, &mut out, b, i, l);
    out
}

/// What a segment is matched on: the words, with objects, tabs and line
/// breaks as spaces.
pub fn key_of_markup(markup: &str) -> String {
    let mut s = String::new();
    for t in tokens(markup) {
        match t {
            Tok::Text { s: x, .. } => s.push_str(&x),
            Tok::Obj(_) | Tok::Tab => s.push(' '),
        }
    }
    normalise(&s)
}

fn normalise(s: &str) -> String {
    s.split(|c: char| c.is_whitespace() || c == '\u{FFFC}').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

/// A key without any spaces: for text whose spacing around objects differs.
pub fn tight(key: &str) -> String {
    key.chars().filter(|c| !c.is_whitespace()).collect()
}

/// What a paragraph of the original holds, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// `l`: in a link.
    Text { s: String, b: bool, i: bool, l: bool },
    Tab,
    Break,
    /// Something that can't be typed; the ids of the runs it is made of.
    Obj(Vec<String>),
}

struct Field {
    instr: String,
    separated: bool,
    link: bool,
    /// Its runs so far, and its object in the output once it has one.
    runs: Vec<String>,
    obj: Option<usize>,
}

/// Walks paragraph `p` (not the paragraphs inside its text boxes).
pub fn walk(t: &Tree, p: Id) -> Vec<Item> {
    walk_with_links(t, p).0
}

/// [`walk`], and the ids of the runs that are a link's text.
pub fn walk_with_links(t: &Tree, p: Id) -> (Vec<Item>, std::collections::HashSet<String>) {
    let mut w = Walk { fields: vec![], out: vec![], links: Default::default() };
    visit(t, p, false, &mut w);
    (w.out, w.links)
}

struct Walk {
    fields: Vec<Field>,
    out: Vec<Item>,
    links: std::collections::HashSet<String>,
}

fn local(t: &Tree, id: Id) -> &str {
    t.local_name(id).unwrap_or("")
}

/// Within a field's result that Babel shows as one object.
fn hidden(fields: &[Field]) -> bool {
    fields.iter().any(|f| f.separated && !f.link)
}

fn visit(t: &Tree, el: Id, link: bool, w: &mut Walk) {
    for c in t.elements(el) {
        match local(t, c) {
            "p" | "pPr" | "rPr" | "Fallback" | "del" | "moveFrom" => {}
            "hyperlink" => visit(t, c, true, w),
            "fldSimple" => {
                let instr = t.attr(c, "w:instr").unwrap_or("");
                if babel_docx::ooxml::parse_hyperlink_instr(instr).is_some() {
                    visit(t, c, true, w);
                } else if !hidden(&w.fields) {
                    let rids = t.descendants(c).into_iter().filter(|&r| local(t, r) == "r").filter_map(|r| t.attr(r, "data-br").map(str::to_owned)).collect();
                    w.out.push(Item::Obj(rids));
                }
            }
            "r" => run(t, c, link, w),
            _ => visit(t, c, link, w),
        }
    }
}

fn on(t: &Tree, rpr: Option<Id>, name: &str) -> bool {
    let Some(rpr) = rpr else { return false };
    t.elements(rpr).into_iter().find(|&e| local(t, e) == name).is_some_and(|e| !matches!(t.attr(e, "w:val"), Some("0" | "false" | "off")))
}

fn run(t: &Tree, r: Id, link: bool, w: &mut Walk) {
    let kids = t.elements(r);
    let rpr = kids.iter().copied().find(|&k| local(t, k) == "rPr");
    let (b, i) = (on(t, rpr, "b"), on(t, rpr, "i"));
    let rid = t.attr(r, "data-br").map(str::to_owned);
    // A run within a field belongs to it (and to its object, once it has one).
    for f in w.fields.iter_mut() {
        if let Some(rid) = &rid {
            f.runs.push(rid.clone());
            if let Some(Item::Obj(rids)) = f.obj.and_then(|o| w.out.get_mut(o)) {
                rids.push(rid.clone());
            }
        }
    }
    for k in kids {
        let name = local(t, k);
        // In a link: a hyperlink, or a HYPERLINK field's result.
        let l = link || w.fields.iter().any(|f| f.separated && f.link);
        match name {
            "fldChar" => match t.attr(k, "w:fldCharType") {
                Some("begin") => w.fields.push(Field { instr: String::new(), separated: false, link: false, runs: rid.iter().cloned().collect(), obj: None }),
                Some("separate") => {
                    let was_hidden = hidden(&w.fields);
                    let at = w.out.len();
                    if let Some(f) = w.fields.last_mut() {
                        f.separated = true;
                        f.link = babel_docx::ooxml::parse_hyperlink_instr(&f.instr).is_some();
                        if !f.link && !was_hidden {
                            f.obj = Some(at);
                            w.out.push(Item::Obj(f.runs.clone()));
                        }
                    }
                }
                Some("end") => {
                    w.fields.pop();
                }
                _ => {}
            },
            "instrText" => {
                if let Some(f) = w.fields.last_mut().filter(|f| !f.separated) {
                    f.instr.push_str(&t.text_content(k));
                }
            }
            _ if hidden(&w.fields) => {}
            "t" => {
                if l && let Some(rid) = &rid {
                    w.links.insert(rid.clone());
                }
                w.out.push(Item::Text { s: t.text_content(k), b, i, l });
            }
            "tab" | "ptab" => w.out.push(Item::Tab),
            "br" | "cr" => {
                if t.attr(k, "w:type").unwrap_or("textWrapping") == "textWrapping" {
                    w.out.push(Item::Break);
                }
            }
            "noBreakHyphen" => w.out.push(Item::Text { s: "\u{2011}".into(), b, i, l }),
            "sym" | "drawing" | "pict" | "object" | "footnoteReference" | "endnoteReference" | "footnoteRef" | "endnoteRef" | "AlternateContent" => w.out.push(Item::Obj(rid.iter().cloned().collect())),
            _ => {}
        }
    }
}

/// The paragraphs of normalised text parts that are segments, in order:
/// their ids and what they hold.
pub fn of_parts(parts: &std::collections::BTreeMap<String, String>, text_parts: &[String]) -> Vec<(String, Vec<Item>)> {
    let mut out = vec![];
    for path in text_parts {
        let Some(xml) = parts.get(path) else { continue };
        let Ok(part) = babel_docx::ooxml::Part::parse(xml) else { continue };
        for p in babel_docx::ooxml::paragraphs(&part.tree) {
            let Some(pid) = part.tree.attr(p, "data-bp") else { continue };
            let items = walk(&part.tree, p);
            if translatable(&items) {
                out.push((pid.to_owned(), items));
            }
        }
    }
    out
}

/// The matching key of a walked paragraph.
pub fn key_of_items(items: &[Item]) -> String {
    let mut s = String::new();
    for it in items {
        match it {
            Item::Text { s: x, .. } => s.push_str(x),
            _ => s.push(' '),
        }
    }
    normalise(&s)
}

/// Whether a paragraph is a segment: it has a letter to translate.
pub fn translatable(items: &[Item]) -> bool {
    items.iter().any(|it| matches!(it, Item::Text { s, .. } if s.chars().any(char::is_alphabetic)))
}

/// A walked paragraph as markup.
pub fn markup(items: &[Item]) -> String {
    let mut s = String::new();
    let mut objects = 0;
    let mut k = 0;
    let mut in_link = false;
    while k < items.len() {
        match &items[k] {
            Item::Text { b, i, l, .. } => {
                // Adjacent text with the same formatting is one span.
                let (b, i, l) = (*b, *i, *l);
                if l != in_link {
                    s.push(if l { '[' } else { ']' });
                    in_link = l;
                }
                let mut text = String::new();
                while let Some(Item::Text { s: x, b: b2, i: i2, l: l2 }) = items.get(k) {
                    if (*b2, *i2, *l2) != (b, i, l) {
                        break;
                    }
                    text.push_str(x);
                    k += 1;
                }
                s.push_str(&span(&text, b, i));
                continue;
            }
            Item::Tab => s.push_str("{tab}"),
            Item::Break => s.push('\n'),
            Item::Obj(_) => {
                objects += 1;
                s.push_str(&format!("{{{objects}}}"));
            }
        }
        k += 1;
    }
    if in_link {
        s.push(']');
    }
    // A line that would read as a heading or a quote.
    s.split('\n').map(|l| if l.starts_with(['#', '>']) { format!("\\{l}") } else { l.to_owned() }).collect::<Vec<_>>().join("\n")
}

fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '*' | '{' | '[' | ']') {
            o.push('\\');
        }
        o.push(c);
    }
    o
}

/// Text with its emphasis; spaces stay outside the stars, and each line of
/// it is marked on its own.
fn span(text: &str, b: bool, i: bool) -> String {
    let mark = match (b, i) {
        (true, true) => "***",
        (true, false) => "**",
        (false, true) => "*",
        _ => "",
    };
    text.split('\n')
        .map(|line| {
            let core = line.trim();
            if mark.is_empty() || core.is_empty() {
                return escape(line);
            }
            let lead = &line[..line.len() - line.trim_start().len()];
            let trail = &line[line.trim_end().len()..];
            format!("{}{mark}{}{mark}{}", escape(lead), escape(core), escape(trail))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The file's front matter and the note on its format.
pub fn header(document: &str, target_lang: &str) -> String {
    format!(
        "---\nbabel-segments: 1\ndocument: {document}\ntarget-language: {target_lang}\n---\n\n\
<!-- One segment per paragraph of the original, in order: its number, the original quoted with \">\", and the translation on the lines below it. Keep {{1}}, {{2}}… (footnote marks, pictures, fields) and {{tab}} where they belong in the translation; **bold** and *italic* as in the original; a new line is a line break within the paragraph. -->\n"
    )
}

/// One segment, as written.
pub fn write_segment(n: usize, id: Option<&str>, source: &str, target: &str) -> String {
    let mut s = match id {
        Some(id) => format!("\n## {n} · {id}\n"),
        None => format!("\n## {n}\n"),
    };
    for l in source.split('\n') {
        s.push_str("> ");
        s.push_str(l);
        s.push('\n');
    }
    s.push('\n');
    if !target.is_empty() {
        s.push_str(target);
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_file() {
        let md = "---\nbabel-segments: 1\n---\n\n<!-- a note\nover lines -->\n\n# Title\n\n## 1 · document:2\n> This is **bold**.{1}\n> Second line\n\nDetta är **fetstil**.{1}\nAndra raden\n\n## 2\n> Untranslated\n\n## 3\n> Third\nTredje\n";
        let s = parse(md).unwrap();
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].id.as_deref(), Some("document:2"));
        assert_eq!(s[0].source, "This is **bold**.{1}\nSecond line");
        assert_eq!(s[0].target, "Detta är **fetstil**.{1}\nAndra raden");
        assert_eq!(s[1].target, "");
        assert_eq!(s[2].target, "Tredje", "the translation may follow the quote directly");
    }

    #[test]
    fn markup_tokens() {
        let t = tokens("A **b** *i* ***bi*** {2} 5 * 3 \\*x\\* {tab}{x}");
        assert_eq!(
            t,
            vec![
                Tok::Text { s: "A ".into(), b: false, i: false, l: false },
                Tok::Text { s: "b".into(), b: true, i: false, l: false },
                Tok::Text { s: " ".into(), b: false, i: false, l: false },
                Tok::Text { s: "i".into(), b: false, i: true, l: false },
                Tok::Text { s: " ".into(), b: false, i: false, l: false },
                Tok::Text { s: "bi".into(), b: true, i: true, l: false },
                Tok::Text { s: " ".into(), b: false, i: false, l: false },
                Tok::Obj(2),
                Tok::Text { s: " 5 * 3 *x* ".into(), b: false, i: false, l: false },
                Tok::Tab,
                Tok::Text { s: "{x}".into(), b: false, i: false, l: false },
            ]
        );
        assert_eq!(key_of_markup("Hello{1} **world**{tab}x"), "Hello world x");
    }

    #[test]
    fn links_are_bracketed() {
        let items = vec![
            Item::Text { s: "See ".into(), b: false, i: false, l: false },
            Item::Text { s: "the ".into(), b: false, i: false, l: true },
            Item::Text { s: "site".into(), b: true, i: false, l: true },
            Item::Text { s: " [now]".into(), b: false, i: false, l: false },
        ];
        let m = markup(&items);
        assert_eq!(m, "See [the **site**] \\[now\\]");
        let t = tokens(&m);
        assert_eq!(t[1], Tok::Text { s: "the ".into(), b: false, i: false, l: true });
        assert_eq!(t[2], Tok::Text { s: "site".into(), b: true, i: false, l: true });
        assert_eq!(t[3], Tok::Text { s: " [now]".into(), b: false, i: false, l: false });
        assert_eq!(key_of_markup(&m), "See the site [now]");
    }

    #[test]
    fn written_markup_reads_back() {
        let items = vec![
            Item::Text { s: "# Not a heading, ".into(), b: false, i: false, l: false },
            Item::Text { s: "bold ".into(), b: true, i: false, l: false },
            Item::Obj(vec![]),
            Item::Text { s: "a*b {c}".into(), b: false, i: true, l: false },
            Item::Break,
            Item::Tab,
            Item::Text { s: "end".into(), b: false, i: false, l: false },
        ];
        let m = markup(&items);
        assert_eq!(m, "\\# Not a heading, **bold** {1}*a\\*b \\{c}*\n{tab}end");
        assert_eq!(key_of_markup(&m), key_of_items(&items));
        let back: Vec<Tok> = tokens(&m);
        assert!(back.contains(&Tok::Text { s: "a*b {c}".into(), b: false, i: true, l: false }));
    }
}
