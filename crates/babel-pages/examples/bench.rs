//! Time for one keystroke: edit + layout, draw a page, encode it.
fn main() -> anyhow::Result<()> {
    let dir = env!("CARGO_MANIFEST_DIR");
    let original = std::fs::read(format!("{dir}/../../test/fixtures/sample.docx"))?;
    let parts: babel_pages::Parts = serde_json::from_str(&std::fs::read_to_string(format!("{dir}/tests/sample-parts.json"))?)?;
    let t = std::time::Instant::now();
    let mut s = babel_pages::edit::Session::open(original, parts)?;
    println!("open: {:?}", t.elapsed());
    let mut pos = s.locate("document:2").unwrap();
    for ch in "Denna rapport ".chars() {
        let t = std::time::Instant::now();
        let (p, pages) = s.insert(&pos, &ch.to_string())?;
        let edit = t.elapsed();
        let t = std::time::Instant::now();
        let (w, h, px) = s.render(pages[0], 2.0);
        let draw = t.elapsed();
        let t = std::time::Instant::now();
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new_with_quality(&mut out, image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::NoFilter)
            .write_image(&px, w, h, image::ExtendedColorType::Rgba8)?;
        println!("edit+layout {edit:?}  draw {draw:?}  png {:?} ({} kB)  pages {pages:?}", t.elapsed(), out.len() / 1024);
        pos = p;
    }
    Ok(())
}
use image::ImageEncoder;
