//! Spelling and grammar for the native editor, through LanguageTool, as the
//! web editor does it (web/js/proof.js): only edited paragraphs, batched and
//! paced for the free API's limits, with back-off; issue kinds and filters
//! the same (words also in the original paragraph are no typos; issues
//! ignored in the web editor stay ignored here, they share the project's
//! `ignored` list).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

/// Characters per request (the free API takes 20 kB).
const MAX_BATCH: usize = 12_000;
/// Between requests (20 a minute at most).
const MIN_INTERVAL: Duration = Duration::from_millis(3200);
pub const ENDPOINT: &str = "https://api.languagetool.org";

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    /// Byte range in the paragraph's text.
    pub a: usize,
    pub b: usize,
    pub kind: &'static str,
    pub message: String,
    pub short: String,
    pub replacements: Vec<String>,
    pub rule: String,
    pub word: String,
}

/// What the proofer knows about one translation.
#[derive(Default)]
pub struct Proofing {
    /// LanguageTool's code for the target language, if it proofs it.
    pub lang: Option<String>,
    pub issues: HashMap<String, Vec<Issue>>,
    pub queue: BTreeSet<String>,
    /// Original paragraph text by id (its words are no typos).
    pub source: HashMap<String, String>,
    /// `rule|word`, as the web editor stores them in the project.
    pub ignored: HashSet<String>,
    pub status: String,
}

/// LanguageTool's code for a Word language code (web/js/langs.js).
pub fn lt_code(word_code: &str) -> Option<String> {
    for l in include_str!("../../../web/js/langs.js").lines() {
        let field = |key: &str| {
            let i = l.find(&format!("{key}: "))? + key.len() + 2;
            let rest = &l[i..];
            if rest.starts_with("null") {
                return Some(None);
            }
            let rest = rest.strip_prefix('\'')?;
            Some(Some(rest[..rest.find('\'')?].to_owned()))
        };
        if field("code").flatten().as_deref() == Some(word_code) {
            return field("lt").flatten();
        }
    }
    None
}

fn kind_of(rule: &Value) -> &'static str {
    match rule["issueType"].as_str() {
        Some("misspelling") => "spelling",
        Some("style" | "locale-violation" | "register") => "style",
        _ if rule["category"]["id"] == "STYLE" => "style",
        Some("typographical" | "whitespace") => "typography",
        _ => "grammar",
    }
}

fn words(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’' || c == '-'))
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Byte offsets for UTF-16 offsets into `text`.
fn utf16_to_byte(text: &str) -> Vec<usize> {
    let mut out = Vec::with_capacity(text.len() + 1);
    for (i, c) in text.char_indices() {
        for _ in 0..c.len_utf16() {
            out.push(i);
        }
    }
    out.push(text.len());
    out
}

pub struct Batch {
    pub items: Vec<(String, String)>,
}

impl Proofing {
    /// Takes paragraphs off the queue for one request.
    pub fn next_batch(&mut self, text_of: impl Fn(&str) -> Option<String>) -> Option<Batch> {
        self.lang.as_ref()?;
        let mut items = vec![];
        let mut size = 0;
        while let Some(pid) = self.queue.iter().next().cloned() {
            let Some(text) = text_of(&pid) else {
                self.queue.remove(&pid);
                continue;
            };
            if !text.chars().any(char::is_alphabetic) {
                self.queue.remove(&pid);
                self.issues.remove(&pid);
                continue;
            }
            if size > 0 && size + text.len() > MAX_BATCH {
                break;
            }
            size += text.len() + 2;
            self.queue.remove(&pid);
            items.push((pid, text));
            if size > MAX_BATCH {
                break;
            }
        }
        (!items.is_empty()).then_some(Batch { items })
    }

    /// Stores a batch's results for paragraphs whose text is still what was sent.
    pub fn accept(&mut self, batch: &Batch, matches: &[Value], text_now: impl Fn(&str) -> Option<String>) {
        let joined: String = batch.items.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n\n");
        let map = utf16_to_byte(&joined);
        let mut start = 0usize;
        for (pid, text) in &batch.items {
            let end = start + text.len();
            if text_now(pid).as_deref() != Some(text.as_str()) {
                self.queue.insert(pid.clone());
                start = end + 2;
                continue;
            }
            let src = words(self.source.get(pid).map(String::as_str).unwrap_or(""));
            let mut list = vec![];
            for m in matches {
                let (Some(o), Some(l)) = (m["offset"].as_u64(), m["length"].as_u64()) else { continue };
                let (Some(&a), Some(&b)) = (map.get(o as usize), map.get((o + l) as usize)) else { continue };
                if a < start || b > end || a >= b {
                    continue;
                }
                let (a, b) = (a - start, b - start);
                let word = text[a..b].to_owned();
                let kind = kind_of(&m["rule"]);
                let rule = m["rule"]["id"].as_str().unwrap_or("").to_owned();
                if kind == "spelling" && src.contains(&word.to_lowercase()) {
                    continue;
                }
                if self.ignored.contains(&format!("{rule}|{word}")) {
                    continue;
                }
                list.push(Issue {
                    a,
                    b,
                    kind,
                    message: m["message"].as_str().unwrap_or("").to_owned(),
                    short: m["shortMessage"].as_str().unwrap_or("").to_owned(),
                    replacements: m["replacements"].as_array().map(|r| r.iter().take(6).filter_map(|x| x["value"].as_str().map(str::to_owned)).collect()).unwrap_or_default(),
                    rule,
                    word,
                });
            }
            if list.is_empty() {
                self.issues.remove(pid);
            } else {
                self.issues.insert(pid.clone(), list);
            }
            start = end + 2;
        }
    }
}

pub enum Failure {
    /// Rate limited or busy: try again later.
    Busy,
    Other(String),
}

/// One request to LanguageTool.
pub async fn check(http: &reqwest::Client, lang: &str, text: &str) -> Result<Vec<Value>, Failure> {
    let res = http
        .post(format!("{ENDPOINT}/v2/check"))
        .form(&[("text", text), ("language", lang), ("level", "default")])
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| Failure::Other(e.to_string()))?;
    match res.status().as_u16() {
        200 => {}
        429 | 503 => return Err(Failure::Busy),
        s => return Err(Failure::Other(format!("LanguageTool answered {s}"))),
    }
    let v: Value = res.json().await.map_err(|e| Failure::Other(e.to_string()))?;
    Ok(v["matches"].as_array().cloned().unwrap_or_default())
}

pub fn pace() -> Duration {
    MIN_INTERVAL
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn knows_languagetool_codes() {
        assert_eq!(lt_code("sv-SE").as_deref(), Some("sv-SE"));
        assert_eq!(lt_code("nb-NO").as_deref(), Some("nb"));
        assert_eq!(lt_code("fi-FI"), None);
    }

    #[test]
    fn maps_offsets_and_filters_like_the_web_editor() {
        let mut p = Proofing { lang: Some("sv-SE".into()), ..Default::default() };
        p.source.insert("d:1".into(), "Report for Babel".into());
        p.ignored.insert("STYLE_X|bra".into());
        let batch = Batch { items: vec![("d:0".into(), "Ett 😀 felstavt ord".into()), ("d:1".into(), "Rapport om Babel bra".into())] };
        // "felstavt" in d:0 (UTF-16 offsets: the emoji is two units), "Babel" and "bra" in d:1.
        let m = |o: u64, l: u64, t: &str, id: &str| json!({ "offset": o, "length": l, "message": "m", "rule": { "id": id, "issueType": t }, "replacements": [{ "value": "felstavat" }] });
        let d1 = "Ett 😀 felstavt ord".encode_utf16().count() as u64 + 2;
        let matches = vec![m(7, 8, "misspelling", "SPELL"), m(d1 + 11, 5, "misspelling", "SPELL"), m(d1 + 17, 3, "style", "STYLE_X")];
        let texts: HashMap<String, String> = batch.items.iter().cloned().collect();
        p.accept(&batch, &matches, |pid| texts.get(pid).cloned());
        let i = &p.issues["d:0"][0];
        assert_eq!((i.word.as_str(), i.kind, i.replacements[0].as_str()), ("felstavt", "spelling", "felstavat"));
        assert!(!p.issues.contains_key("d:1"), "Babel is in the original, bra is ignored");
    }

    #[test]
    fn a_paragraph_edited_meanwhile_is_checked_again() {
        let mut p = Proofing { lang: Some("sv-SE".into()), ..Default::default() };
        let batch = Batch { items: vec![("d:0".into(), "Gammal".into())] };
        p.accept(&batch, &[], |_| Some("Ny".into()));
        assert!(p.queue.contains("d:0"));
    }
}
