//! Prints a .docx as Babel's segment file body (no ids), for comparing with
//! skills/babel-translate/scripts/babel_segments.py.
use babel_editor::segments;
fn main() {
    let path = std::env::args().nth(1).expect("a .docx");
    let bytes = std::fs::read(&path).unwrap();
    let imp = match babel_docx::package::import(&bytes) { Ok(i) => i, Err(e) => { eprintln!("import: {e}"); std::process::exit(2) } };
    for (n, (_, items)) in segments::of_parts(&imp.parts, &imp.package.text_parts).iter().enumerate() {
        print!("{}", segments::write_segment(n + 1, None, &segments::markup(items), ""));
    }
}
