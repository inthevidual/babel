//! The pages as QML loads them: PNG files in the runtime directory
//! (memory-backed), one per page and generation, the one before removed.

use std::path::PathBuf;

use babel_app::session::{Pages, Side};
use babel_editor::Editor;

/// Pixels per point the pages are drawn at.
const SCALE: f32 = 2.0;

pub struct Png {
    dir: PathBuf,
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("babel-pages")
}

fn write_png(file: &std::path::Path, w: u32, h: u32, px: &[u8]) -> anyhow::Result<()> {
    use image::ImageEncoder;
    let mut out = std::io::BufWriter::new(std::fs::File::create(file)?);
    image::codecs::png::PngEncoder::new_with_quality(&mut out, image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::NoFilter)
        .write_image(px, w, h, image::ExtendedColorType::Rgba8)?;
    Ok(())
}

impl Png {
    /// A fresh directory for the project's pages.
    pub fn new(project: &str) -> anyhow::Result<Png> {
        let dir = runtime_dir().join(project);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        Ok(Png { dir })
    }

    fn file(&self, side: Side, page: usize, version: u32) -> PathBuf {
        self.dir.join(format!("{}{page}-v{version}.png", side.letter()))
    }
}

impl Pages for Png {
    fn drawn(&mut self, editor: &Editor, side: Side, page: usize, version: u32) -> String {
        let (w, h, px) = match side {
            Side::Target => editor.render(page, SCALE),
            Side::Source => editor.render_source(page, SCALE),
        };
        let file = self.file(side, page, version);
        let _ = write_png(&file, w, h, &px);
        if version > 1 {
            let _ = std::fs::remove_file(self.file(side, page, version - 1));
        }
        format!("file://{}", file.display())
    }

    fn close(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
