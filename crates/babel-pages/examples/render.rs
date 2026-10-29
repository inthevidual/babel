//! Renders every page of a .docx to PNG with WordCraft's layout engine.
//!   cargo run --release -p babel-pages --example render -- IN.docx OUT_DIR [DPI]

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (input, out) = (&args[1], std::path::Path::new(&args[2]));
    let dpi: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(96.0);
    std::fs::create_dir_all(out)?;
    let t = std::time::Instant::now();
    let doc = wordcraft_docx::read(&std::fs::read(input)?)?;
    let mut cache = wordcraft_layout::LayoutCache::default();
    let laid = wordcraft_layout::layout(&doc, &mut cache, &Default::default());
    println!("read + layout: {:?}, {} pages, {} paragraphs placed", t.elapsed(), laid.pages.len(), laid.index.len());
    for (i, page) in laid.pages.iter().enumerate() {
        let r = wordcraft_render::render_page(&doc, page, dpi / 72.0, &Default::default());
        let img = image::RgbaImage::from_raw(r.width, r.height, r.to_straight()).expect("raster size");
        img.save(out.join(format!("page-{:02}.png", i + 1)))?;
    }
    println!("total: {:?}", t.elapsed());
    Ok(())
}
