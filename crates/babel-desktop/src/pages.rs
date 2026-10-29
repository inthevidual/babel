//! Pages drawn by the native layout engine (babel-pages), for the read-only
//! native view of a translation. Rendered to PNG in the app's cache, keyed by
//! the revision, so reopening an unchanged translation draws nothing.

use std::path::PathBuf;

use serde::Serialize;

use crate::app::app;

/// Pixels per point: 2 = 144 dpi, sharp at 100 % zoom on a high-density screen.
const SCALE: f32 = 2.0;

#[derive(Serialize)]
pub struct PageImage {
    /// file:// URL of the PNG.
    pub url: String,
    /// Page size in points.
    pub w: f32,
    pub h: f32,
}

#[derive(Serialize)]
pub struct Rendered {
    pub source: Vec<PageImage>,
    pub target: Vec<PageImage>,
    pub ms: u128,
}

fn render_side(original: &[u8], parts: &babel_pages::Parts, dir: PathBuf) -> anyhow::Result<Vec<PageImage>> {
    let laid = babel_pages::lay_out(&babel_pages::renderable(original, parts)?)?;
    std::fs::create_dir_all(&dir)?;
    let mut out = Vec::with_capacity(laid.layout.pages.len());
    for (i, page) in laid.layout.pages.iter().enumerate() {
        let file = dir.join(format!("page-{:03}.png", i + 1));
        if !file.exists() {
            let (w, h, px) = laid.render(i, SCALE);
            let img = image::RgbaImage::from_raw(w, h, px).ok_or_else(|| anyhow::anyhow!("raster size"))?;
            let tmp = file.with_extension("tmp");
            img.save_with_format(&tmp, image::ImageFormat::Png)?;
            std::fs::rename(&tmp, &file)?;
        }
        out.push(PageImage { url: format!("file://{}", file.display()), w: page.w, h: page.h });
    }
    Ok(out)
}

/// Lays out and draws the original and the translation as stored.
pub fn render(id: &str) -> anyhow::Result<Rendered> {
    let t = std::time::Instant::now();
    let a = app();
    let pid = id.to_owned();
    let (docx, source, (rev, translated)) =
        a.db(|s| Ok((s.source_docx(&a.user, &pid)?, s.source_parts(&a.user, &pid)?, s.parts(&a.user, &pid)?)))?;
    let mut target = source.clone();
    target.extend(translated);
    let base = a.data_dir.join("cache/pages").join(id);
    // Older revisions' pages are of no further use.
    if let Ok(entries) = std::fs::read_dir(&base) {
        for e in entries.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("target-") && n != format!("target-{rev}") {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    let src = render_side(&docx, &source, base.join("source"))?;
    let tgt = render_side(&docx, &target, base.join(format!("target-{rev}")))?;
    Ok(Rendered { source: src, target: tgt, ms: t.elapsed().as_millis() })
}
