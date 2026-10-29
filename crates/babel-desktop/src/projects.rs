//! Translations as wholes: opening a .docx as a new one (or pairing it with
//! an earlier translation), exporting the translated .docx, versions, and
//! the target languages. Projects are made exactly as the web editor makes
//! them (main.js startNew), so they sync and open alike everywhere.

use serde_json::{json, Value};

use babel_core::store::{now_ms, Version};
use babel_docx::package;

use crate::app::app;

/// The target languages (web/js/langs.js): [{code, name, en, lt}].
pub fn languages() -> Value {
    let field = |l: &str, key: &str| -> Option<Value> {
        let i = l.find(&format!("{key}: "))? + key.len() + 2;
        let rest = &l[i..];
        if rest.starts_with("null") {
            return Some(Value::Null);
        }
        let rest = rest.strip_prefix('\'')?;
        Some(json!(&rest[..rest.find('\'')?]))
    };
    let list: Vec<Value> = include_str!("../../../web/js/langs.js")
        .lines()
        .filter_map(|l| {
            Some(json!({ "code": field(l, "code")?, "name": field(l, "name")?, "en": field(l, "en")?, "lt": field(l, "lt").unwrap_or(Value::Null) }))
        })
        .collect();
    Value::Array(list)
}

/// A language's own name.
pub fn language_name(code: &str) -> Option<String> {
    languages().as_array()?.iter().find(|l| l["code"] == code).and_then(|l| l["name"].as_str().map(str::to_owned))
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "document.docx".into())
}

/// Opens a .docx as a new translation into `lang`; with `translated`, pairs it
/// with an earlier translation of it. Returns the new project's id.
pub fn import(path: &std::path::Path, lang: &str, translated: Option<&std::path::Path>) -> anyhow::Result<String> {
    let name = file_name(path);
    if !name.to_lowercase().ends_with(".docx") {
        anyhow::bail!("“{name}” is not a .docx file. Open it in Word and save it as .docx first.");
    }
    let bytes = std::fs::read(path)?;
    let imported = package::import(&bytes)?;
    let paired = match translated {
        Some(t) => Some(package::pair(&imported, &std::fs::read(t)?)?),
        None => None,
    };
    let now = now_ms();
    let id = uuid::Uuid::new_v4().to_string();
    let meta = json!({
        "id": id,
        "name": name.trim_end_matches(".docx").trim_end_matches(".DOCX"),
        "fileName": name,
        "targetLang": lang,
        "createdAt": now,
        "updatedAt": now,
        "textParts": imported.package.text_parts,
        "confirmed": [],
        "ignored": [],
        "stats": {},
    });
    let a = app();
    a.db(|s| {
        s.create_project(&a.user, &meta, &bytes, &imported.parts)?;
        if let Some(parts) = &paired {
            s.save_parts(&a.user, &id, 0, Some(&meta), parts, Some("native"))?;
        }
        Ok(())
    })?;
    a.state.notify(babel_server::Change { user: a.user.id, project: id.clone(), rev: 0, origin: Some("native".into()), deleted: false });
    Ok(id)
}

/// The translated .docx of a project (or of one of its versions) at `to`.
pub fn export(id: &str, version: Option<&str>, to: &std::path::Path) -> anyhow::Result<()> {
    let a = app();
    let pid = id.to_owned();
    let (docx, meta, parts) = a.db(|s| {
        let docx = s.source_docx(&a.user, &pid)?;
        let meta = s.get_project(&a.user, &pid)?;
        let parts = match version {
            Some(sid) => s.get_version(&a.user, sid)?.1.parts.unwrap_or_default(),
            None => s.parts(&a.user, &pid)?.1,
        };
        Ok((docx, meta, parts))
    })?;
    let out = package::build_docx(&docx, &parts, meta["targetLang"].as_str())?;
    let tmp = to.with_extension("docx.part");
    std::fs::write(&tmp, out)?;
    std::fs::rename(&tmp, to)?;
    Ok(())
}

/// The file name an export gets (as the web editor names downloads): name_SV.docx.
pub fn export_name(id: &str, version_time: Option<i64>) -> String {
    let a = app();
    let pid = id.to_owned();
    let meta = a.db(|s| s.get_project(&a.user, &pid)).unwrap_or(Value::Null);
    let lang = meta["targetLang"].as_str().unwrap_or("");
    let suffix = lang.split('-').next().unwrap_or(lang).to_uppercase();
    let when = version_time
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|t| format!("_{}", t.with_timezone(&chrono::Local).format("%Y-%m-%d-%H-%M")))
        .unwrap_or_default();
    format!("{}_{suffix}{when}.docx", meta["name"].as_str().unwrap_or("translation"))
}

/// Versions, newest first: [{sid, time, reason, stats}].
pub fn versions(id: &str) -> Value {
    let a = app();
    let pid = id.to_owned();
    json!(a.db(|s| s.list_versions(&a.user, &pid)).unwrap_or_default())
}

/// Keeps the stored translation as a version.
pub fn add_version(id: &str, reason: &str, stats: Value) -> anyhow::Result<()> {
    let a = app();
    let pid = id.to_owned();
    let parts = a.db(|s| s.parts(&a.user, &pid))?.1;
    if parts.is_empty() {
        return Ok(()); // nothing translated yet
    }
    let v = Version { sid: String::new(), time: now_ms(), reason: reason.to_owned(), stats, parts: Some(parts) };
    a.db(|s| s.add_version(&a.user, &pid, &v))?;
    Ok(())
}

/// Puts a version back (the current state is kept as a version first).
pub fn restore(id: &str, sid: &str, stats: Value) -> anyhow::Result<()> {
    add_version(id, "before restore", stats)?;
    let a = app();
    let pid = id.to_owned();
    let rev = a.db(|s| {
        let parts = s.get_version(&a.user, sid)?.1.parts.unwrap_or_default();
        let (rev, _) = s.parts(&a.user, &pid)?;
        let mut meta = s.get_project(&a.user, &pid)?;
        meta["updatedAt"] = json!(now_ms());
        Ok(s.save_parts(&a.user, &pid, rev, Some(&meta), &parts, Some("restore"))?.rev)
    })?;
    a.state.notify(babel_server::Change { user: a.user.id, project: id.to_owned(), rev, origin: Some("restore".into()), deleted: false });
    Ok(())
}

/// Changes the project's target language.
pub fn set_language(id: &str, lang: &str) -> anyhow::Result<()> {
    let a = app();
    let pid = id.to_owned();
    a.db(|s| {
        let mut meta = s.get_project(&a.user, &pid)?;
        meta["targetLang"] = json!(lang);
        meta["updatedAt"] = json!(now_ms());
        s.save_meta(&a.user, &pid, &meta)
    })?;
    Ok(())
}
