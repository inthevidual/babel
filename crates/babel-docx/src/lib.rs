//! Babel's Word format: the parts of web/js/ooxml.js the native editor needs,
//! in Rust. The original package stays the source of truth; an edit rewrites
//! the text of runs inside one `w:p` and nothing else.

pub mod ooxml;
pub mod package;
pub mod para;
pub mod tree;
pub mod writeback;

pub use writeback::{apply_paragraph, Info, Templates, Token};
