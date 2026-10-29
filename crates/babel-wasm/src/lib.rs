//! Babel's editor in the browser: babel-editor compiled to WebAssembly.
//! web/js/wasm-view.js drives it: pages come back as RGBA for a canvas, the
//! rest as JSON strings (the same answers the desktop app gets).

use babel_editor::{proof, Editor, Key};
use babel_pages::Parts;
use serde_json::{json, Value};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

/// Gives the layout engine a font (TTF, OTF or WOFF2). The browser has no
/// system fonts to look in: the page passes the ones Babel serves.
#[wasm_bindgen(js_name = addFont)]
pub fn add_font(bytes: Vec<u8>) -> Result<usize, JsError> {
    let raw = wordcraft_fonts::decode_font_file(&bytes).map_err(|e| JsError::new(&format!("{e:?}")))?;
    Ok(wordcraft_fonts::FontDb::global().add_font(raw))
}

fn parts(json: &str) -> Result<Parts, JsError> {
    serde_json::from_str(json).map_err(|e| JsError::new(&format!("parts: {e}")))
}

#[wasm_bindgen]
pub struct WasmEditor {
    editor: Editor,
    batch: Option<proof::Batch>,
}

#[wasm_bindgen]
impl WasmEditor {
    /// Opens a translation: the original .docx, its normalised text parts
    /// (JSON path → XML), the stored translated parts, the project's metadata.
    #[wasm_bindgen(constructor)]
    pub fn new(original: Vec<u8>, source_parts: &str, translated: &str, project: &str) -> Result<WasmEditor, JsError> {
        let project: Value = serde_json::from_str(project).map_err(|e| JsError::new(&e.to_string()))?;
        let editor = Editor::open(original, parts(source_parts)?, parts(translated)?, &project).map_err(|e| JsError::new(&e.to_string()))?;
        Ok(WasmEditor { editor, batch: None })
    }

    #[wasm_bindgen(js_name = setProofing)]
    pub fn set_proofing(&mut self, picky: bool, dictionary: &str) {
        let words: Vec<String> = serde_json::from_str(dictionary).unwrap_or_default();
        self.editor.set_proofing(picky, words);
    }

    #[wasm_bindgen(js_name = setLanguage)]
    pub fn set_language(&mut self, code: &str) {
        self.editor.set_language(code);
    }

    // ── Pages ─────────────────────────────────────────────────────────────

    #[wasm_bindgen(js_name = pageSizes)]
    pub fn page_sizes(&self) -> String {
        self.editor.page_sizes().to_string()
    }

    /// {all, pages}: the translation's pages to draw again.
    #[wasm_bindgen(js_name = takeRedraw)]
    pub fn take_redraw(&mut self) -> String {
        let (all, pages) = self.editor.take_redraw();
        json!({ "all": all, "pages": pages }).to_string()
    }

    /// A page as [width u32 LE, height u32 LE, RGBA…] at `scale` pixels per point.
    pub fn render(&self, page: usize, scale: f32) -> Vec<u8> {
        pack(self.editor.render(page, scale))
    }

    #[wasm_bindgen(js_name = renderSource)]
    pub fn render_source(&self, page: usize, scale: f32) -> Vec<u8> {
        pack(self.editor.render_source(page, scale))
    }

    // ── State ─────────────────────────────────────────────────────────────

    pub fn state(&mut self) -> String {
        self.editor.state().to_string()
    }

    pub fn stats(&self) -> String {
        self.editor.stats().to_string()
    }

    /// Caret geometry of paragraph `pid` at byte offset `off` (JSON or "null").
    #[wasm_bindgen(js_name = caretAt)]
    pub fn caret_at(&self, pid: &str, off: usize) -> String {
        self.editor.caret_at(pid, off).to_string()
    }

    pub fn marks(&self) -> String {
        self.editor.marks().to_string()
    }

    pub fn issues(&self) -> String {
        self.editor.issues().to_string()
    }

    // ── Input ─────────────────────────────────────────────────────────────

    pub fn click(&mut self, page: usize, x: f32, y: f32, extend: bool) -> String {
        self.editor.click(page, x, y, extend).to_string()
    }

    pub fn word(&mut self, page: usize, x: f32, y: f32) -> String {
        self.editor.select_word(page, x, y).to_string()
    }

    #[wasm_bindgen(js_name = sourceClick)]
    pub fn source_click(&mut self, page: usize, x: f32, y: f32, extend: bool) -> String {
        self.editor.source_click(page, x, y, extend).to_string()
    }

    #[wasm_bindgen(js_name = sourceWord)]
    pub fn source_word(&mut self, page: usize, x: f32, y: f32) -> String {
        self.editor.source_word(page, x, y).to_string()
    }

    #[wasm_bindgen(js_name = sourceMenu)]
    pub fn source_menu(&mut self, page: usize, x: f32, y: f32) -> String {
        self.editor.source_menu(page, x, y).to_string()
    }

    /// A key with Qt's codes and modifier bits (the view maps browser keys); "null" if not handled.
    pub fn key(&mut self, key: i32, text: &str, modifiers: i32) -> String {
        match Key::from_qt(key, text, modifiers) {
            Some(k) => self.editor.key(k, modifiers & 0x0200_0000 != 0).to_string(),
            None => "null".into(),
        }
    }

    /// Text typed or committed by an input method (or pasted).
    pub fn text(&mut self, text: &str) -> String {
        if text.is_empty() {
            return "null".into();
        }
        self.editor.key(Key::Text(text.to_owned()), false).to_string()
    }

    #[wasm_bindgen(js_name = selectedText)]
    pub fn selected_text(&self) -> String {
        self.editor.selected_text()
    }

    #[wasm_bindgen(js_name = sourceSelectedText)]
    pub fn source_selected_text(&self) -> String {
        self.editor.source_selected_text()
    }

    #[wasm_bindgen(js_name = originalText)]
    pub fn original_text(&self) -> Option<String> {
        self.editor.original_text()
    }

    #[wasm_bindgen(js_name = copySourceLink)]
    pub fn copy_source_link(&mut self) -> Option<String> {
        self.editor.copy_source_link()
    }

    pub fn revert(&mut self) -> String {
        self.editor.revert_paragraph().to_string()
    }

    #[wasm_bindgen(js_name = pasteLink)]
    pub fn paste_link(&mut self, clipboard: &str) -> String {
        self.editor.paste_link(clipboard).to_string()
    }

    // ── Proofing ──────────────────────────────────────────────────────────

    /// The next LanguageTool request as form fields ({field: value}), or "null".
    #[wasm_bindgen(js_name = proofRequest)]
    pub fn proof_request(&mut self) -> String {
        if self.batch.is_some() {
            return "null".into();
        }
        match self.editor.proof_batch() {
            Some(b) => {
                let form: serde_json::Map<String, Value> = self.editor.proof.request(&b).into_iter().map(|(k, v)| (k.to_owned(), json!(v))).collect();
                self.batch = Some(b);
                Value::Object(form).to_string()
            }
            None => "null".into(),
        }
    }

    /// LanguageTool's answer (its JSON) to the last request.
    #[wasm_bindgen(js_name = proofResult)]
    pub fn proof_result(&mut self, answer: &str) {
        let Some(b) = self.batch.take() else { return };
        let v: Value = serde_json::from_str(answer).unwrap_or(Value::Null);
        let matches = v["matches"].as_array().cloned().unwrap_or_default();
        self.editor.proof_result(&b, &matches);
    }

    #[wasm_bindgen(js_name = proofFailed)]
    pub fn proof_failed(&mut self, status: &str) {
        if let Some(b) = self.batch.take() {
            self.editor.proof_failed(&b, status);
        }
    }

    #[wasm_bindgen(js_name = applySuggestion)]
    pub fn apply_suggestion(&mut self, pid: &str, n: usize, value: &str) -> String {
        self.editor.apply_suggestion(pid, n, value).to_string()
    }

    pub fn ignore(&mut self, pid: &str, n: usize) {
        self.editor.ignore(pid, n);
    }

    /// Adds an issue's word to the dictionary; returns the word.
    #[wasm_bindgen(js_name = addToDictionary)]
    pub fn add_to_dictionary(&mut self, pid: &str, n: usize) -> Option<String> {
        self.editor.add_to_dictionary(pid, n)
    }

    // ── Saving ────────────────────────────────────────────────────────────

    /// Edited paragraphs and relationship parts since the last call, for the journal.
    #[wasm_bindgen(js_name = takeJournal)]
    pub fn take_journal(&mut self) -> String {
        self.editor.take_journal().to_string()
    }

    /// Changed parts (JSON path → XML) since the last call.
    #[wasm_bindgen(js_name = takeDirty)]
    pub fn take_dirty(&mut self) -> String {
        json!(self.editor.take_dirty()).to_string()
    }

    /// The project's metadata with this session's done marks, ignored issues, statistics.
    #[wasm_bindgen(js_name = projectMeta)]
    pub fn project_meta(&mut self, project: &str) -> String {
        self.editor.meta_dirty = false;
        let p: Value = serde_json::from_str(project).unwrap_or(Value::Null);
        self.editor.project_meta(&p).to_string()
    }

    /// Whether done marks or ignored issues changed (the metadata needs saving).
    #[wasm_bindgen(js_name = metaDirty)]
    pub fn meta_dirty(&self) -> bool {
        self.editor.meta_dirty
    }
}

fn pack((w, h, px): (u32, u32, Vec<u8>)) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + px.len());
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(&px);
    out
}
