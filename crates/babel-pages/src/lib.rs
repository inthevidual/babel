//! Native page layout for Babel.
//!
//! Lays out a Word document with WordCraft's engine (the `inthevidual/wordcraft`
//! fork, whose reader keeps each paragraph's `data-bp` id) and ties every placed
//! paragraph back to Babel's paragraph id, so that what is drawn on the pages can
//! be edited and written back into the original package.
//!
//! The input is a *renderable* package: the original `.docx` with its text parts
//! replaced by Babel's normalised ones (every `w:p` carries `data-bp`). The
//! original file itself is never laid out or written by this crate.

use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Read, Write};

use wordcraft_doc::{Document, StoryRef};
use wordcraft_layout::{DocLayout, LayoutCache, LayoutOptions};

pub type Parts = BTreeMap<String, String>;

/// A laid-out document.
pub struct Laid {
    pub doc: Document,
    pub layout: DocLayout,
    /// Babel paragraph id → the pages it is drawn on, in order.
    pub pages_of: HashMap<String, Vec<usize>>,
}

/// The original package with `parts` (path → XML) in place of its own.
pub fn renderable(original: &[u8], parts: &Parts) -> anyhow::Result<Vec<u8>> {
    let mut zin = zip::ZipArchive::new(Cursor::new(original))?;
    let mut zout = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for i in 0..zin.len() {
        let mut f = zin.by_index(i)?;
        let name = f.name()?.into_owned();
        zout.start_file(&name, opts)?;
        match parts.get(&name) {
            Some(xml) => zout.write_all(xml.as_bytes())?,
            None => {
                let mut buf = Vec::new();
                f.read_to_end(&mut buf)?;
                zout.write_all(&buf)?;
            }
        }
    }
    Ok(zout.finish()?.into_inner())
}

/// What Babel shows: the text as it reads with tracked changes accepted.
pub fn options() -> LayoutOptions {
    LayoutOptions { hide_deleted: true, ..Default::default() }
}

/// Reads and lays out a renderable package.
pub fn lay_out(docx: &[u8]) -> anyhow::Result<Laid> {
    let doc = wordcraft_docx::read(docx)?;
    let mut cache = LayoutCache::default();
    let layout = wordcraft_layout::layout(&doc, &mut cache, &options());
    let mut pages_of: HashMap<String, Vec<usize>> = HashMap::new();
    for ((story, path), placed) in &layout.index {
        let Some(id) = doc.para(*story, path).and_then(|p| p.source.as_deref()) else { continue };
        let pages = pages_of.entry(id.to_owned()).or_default();
        pages.extend(placed.iter().map(|(page, _)| *page));
        pages.sort_unstable();
        pages.dedup();
    }
    Ok(Laid { doc, layout, pages_of })
}

impl Laid {
    /// The body's paragraphs in document order, with their Babel ids where they have one.
    pub fn body_paragraphs(&self) -> Vec<(Option<String>, Vec<usize>)> {
        let mut body: Vec<_> = self.layout.index.iter().filter(|((s, _), _)| *s == StoryRef::Body).collect();
        body.sort_by(|a, b| a.0 .1.cmp(&b.0 .1));
        body.into_iter()
            .map(|((s, path), placed)| {
                let id = self.doc.para(*s, path).and_then(|p| p.source.as_deref()).map(str::to_owned);
                let mut pages: Vec<usize> = placed.iter().map(|(p, _)| *p).collect();
                pages.sort_unstable();
                pages.dedup();
                (id, pages)
            })
            .collect()
    }

    /// Page `i` as straight-alpha RGBA, `scale` pixels per point.
    pub fn render(&self, i: usize, scale: f32) -> (u32, u32, Vec<u8>) {
        let r = wordcraft_render::render_page(&self.doc, &self.layout.pages[i], scale, &Default::default());
        (r.width, r.height, r.to_straight())
    }
}
