//! How closely the native layout's page breaks follow Word's.
//!
//! A `.docx` saved by Word records where Word last broke its pages
//! (`w:lastRenderedPageBreak`). For each file this gives every top-level block of
//! the body (paragraph or table) an id, lays the file out, and compares the
//! blocks where a new page starts.
//!   cargo run --release -p babel-pages --example score -- FILE.docx…
//! One tab-separated line per file: name, Word pages, native pages, Word breaks,
//! breaks matched exactly, matched within one block.

use std::collections::{BTreeSet, HashMap};
use std::io::Read;

use quick_xml::events::Event;

/// Gives every `w:p` of a body part a `data-bp` id (`b{block}.{n}`, block = the
/// top-level body element it is in) and returns the blocks that hold a
/// `w:lastRenderedPageBreak`.
fn anchor(xml: &str) -> anyhow::Result<(String, BTreeSet<usize>, usize)> {
    let mut r = quick_xml::Reader::from_str(xml);
    let mut out = String::with_capacity(xml.len() + xml.len() / 8);
    let mut at = 0;
    let (mut depth, mut body_depth, mut block, mut blocks, mut n) = (0usize, None, 0usize, 0usize, 0usize);
    let mut breaks = BTreeSet::new();
    loop {
        let start = r.buffer_position() as usize;
        let ev = r.read_event()?;
        let end = r.buffer_position() as usize;
        match &ev {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => {
                let empty = matches!(ev, Event::Empty(_));
                let name = e.name();
                let name = name.into_inner();
                if name == "w:body" {
                    body_depth = Some(depth + 1);
                } else if Some(depth) == body_depth && name != "w:sectPr" {
                    block = blocks;
                    blocks += 1;
                }
                if name == "w:p" && body_depth.is_some() {
                    // Insert the id just after the element name.
                    let tag_end = start + 1 + name.len();
                    out.push_str(&xml[at..tag_end]);
                    out.push_str(&format!(r#" data-bp="b{block}.{n}""#));
                    at = tag_end;
                    n += 1;
                }
                if name == "w:lastRenderedPageBreak" && body_depth.is_some() {
                    breaks.insert(block);
                }
                if !empty {
                    depth += 1;
                }
            }
            Event::End(_) => depth -= 1,
            _ => {}
        }
        let _ = end;
    }
    out.push_str(&xml[at..]);
    Ok((out, breaks, blocks))
}

fn block_of(id: &str) -> Option<usize> {
    id.strip_prefix('b')?.split('.').next()?.parse().ok()
}

fn score(path: &str) -> anyhow::Result<String> {
    let bytes = std::fs::read(path)?;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes))?;
    let mut xml = String::new();
    zip.by_name("word/document.xml")?.read_to_string(&mut xml)?;
    let (anchored, word, nblocks) = anchor(&xml)?;
    let parts = babel_pages::Parts::from([("word/document.xml".to_owned(), anchored)]);
    let laid = babel_pages::lay_out(&babel_pages::renderable(&bytes, &parts)?)?;

    // Pages each top-level block is drawn on.
    let mut spans: HashMap<usize, (usize, usize)> = HashMap::new();
    for (id, pages) in laid.pages_of.iter() {
        let Some(b) = block_of(id) else { continue };
        let (lo, hi) = (*pages.first().unwrap_or(&0), *pages.last().unwrap_or(&0));
        let e = spans.entry(b).or_insert((lo, hi));
        e.0 = e.0.min(lo);
        e.1 = e.1.max(hi);
    }
    // A block starts a page (or breaks across one) natively.
    let mut native = BTreeSet::new();
    let mut prev_hi = 0;
    for b in 0..nblocks {
        let Some(&(lo, hi)) = spans.get(&b) else { continue };
        if lo > prev_hi || hi > lo {
            native.insert(b);
        }
        prev_hi = prev_hi.max(hi);
    }
    // Word's first block on page 1 is no break.
    let word: BTreeSet<usize> = word.into_iter().filter(|&b| b > 0).collect();
    let exact = word.iter().filter(|b| native.contains(b)).count();
    let near = word
        .iter()
        .filter(|&&b| native.contains(&b) || native.contains(&(b + 1)) || (b > 0 && native.contains(&(b - 1))))
        .count();
    let name = std::path::Path::new(path).file_name().unwrap_or_default().to_string_lossy().into_owned();
    Ok(format!("{name}\t{}\t{}\t{}\t{exact}\t{near}", word.len() + 1, laid.layout.pages.len(), word.len()))
}

fn main() {
    println!("file\tword_pages\tnative_pages\tword_breaks\texact\twithin1");
    for path in std::env::args().skip(1) {
        match std::panic::catch_unwind(|| score(&path)) {
            Ok(Ok(line)) => println!("{line}"),
            Ok(Err(e)) => eprintln!("{path}: {e}"),
            Err(_) => eprintln!("{path}: panicked"),
        }
    }
}
