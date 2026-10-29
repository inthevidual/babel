//! Translations, their revisions and versions, users and sessions, in SQLite.
//!
//! The server keeps one of these for all accounts; the desktop app keeps one
//! on the computer with a single local user. Both serve the same HTTP API
//! from it, so the editor does not know which it is talking to.
//!
//! A project is the original `.docx`, its normalised text parts, and a
//! numbered sequence of revisions of the translated parts. Revision 0 is
//! the untouched original; every save makes the next one. Part contents are
//! stored once per distinct text (content-addressed, zstd-compressed), so
//! revisions and versions share whatever they have in common. Only the most
//! recent revisions are kept: they are the bases for merging saves made from
//! an older revision.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::Error;
use crate::merge::{self, Kind};

pub type Parts = BTreeMap<String, String>;

/// Revisions kept per project, for merging saves based on older ones.
const KEEP_REVISIONS: i64 = 100;
/// Versions kept per project; only automatic ones are pruned.
const KEEP_VERSIONS: i64 = 60;

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub name: String,
}

/// A project as the sync engine sees it, including deleted ones.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub rev: i64,
    pub updated_at: i64,
    pub deleted: bool,
    pub meta: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub rev: i64,
    /// The stored state had moved on from the save's base: the result
    /// includes edits the saver has not seen.
    pub merged: bool,
    /// Paragraphs edited on both sides; the save won and the stored state
    /// from before it was kept as a version.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub conflicts: Vec<String>,
    /// The full translated parts after a merge.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parts: Option<Parts>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    /// Empty when a client adds one; the store assigns it.
    #[serde(default)]
    pub sid: String,
    pub time: i64,
    pub reason: String,
    #[serde(default)]
    pub stats: Value,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parts: Option<Parts>,
}

pub struct Store {
    conn: Connection,
}

const SCHEMA: &str = r#"
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE COLLATE NOCASE,
  pw_hash TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
  token_hash TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  label TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  last_seen INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS blobs (
  hash TEXT PRIMARY KEY,
  data BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS projects (
  id TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  meta TEXT NOT NULL,
  rev INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL,
  deleted INTEGER NOT NULL DEFAULT 0,
  docx TEXT
);
CREATE INDEX IF NOT EXISTS projects_by_user ON projects(user_id);
CREATE TABLE IF NOT EXISTS source_parts (
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  path TEXT NOT NULL,
  hash TEXT NOT NULL,
  PRIMARY KEY (project_id, path)
);
CREATE TABLE IF NOT EXISTS revisions (
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  rev INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  origin TEXT,
  PRIMARY KEY (project_id, rev)
);
CREATE TABLE IF NOT EXISTS rev_parts (
  project_id TEXT NOT NULL,
  rev INTEGER NOT NULL,
  path TEXT NOT NULL,
  hash TEXT NOT NULL,
  PRIMARY KEY (project_id, rev, path),
  FOREIGN KEY (project_id, rev) REFERENCES revisions(project_id, rev) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS versions (
  sid TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  time INTEGER NOT NULL,
  reason TEXT NOT NULL,
  stats TEXT
);
CREATE INDEX IF NOT EXISTS versions_by_project ON versions(project_id, time);
CREATE TABLE IF NOT EXISTS version_parts (
  sid TEXT NOT NULL REFERENCES versions(sid) ON DELETE CASCADE,
  path TEXT NOT NULL,
  hash TEXT NOT NULL,
  PRIMARY KEY (sid, path)
);
CREATE TABLE IF NOT EXISTS kv (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;

fn hash(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn put_blob(tx: &Transaction, data: &[u8]) -> Result<String, Error> {
    let h = hash(data);
    let exists: bool = tx
        .query_row("SELECT 1 FROM blobs WHERE hash = ?1", [&h], |_| Ok(true))
        .optional()?
        .unwrap_or(false);
    if !exists {
        let z = zstd::encode_all(data, 3).map_err(|e| Error::Internal(e.to_string()))?;
        tx.execute(
            "INSERT INTO blobs (hash, data) VALUES (?1, ?2)",
            params![h, z],
        )?;
    }
    Ok(h)
}

fn get_blob(conn: &Connection, h: &str) -> Result<Vec<u8>, Error> {
    let z: Vec<u8> = conn
        .query_row("SELECT data FROM blobs WHERE hash = ?1", [h], |r| r.get(0))
        .optional()?
        .ok_or_else(|| Error::Internal(format!("missing blob {h}")))?;
    zstd::decode_all(&z[..]).map_err(|e| Error::Internal(e.to_string()))
}

fn get_text(conn: &Connection, h: &str) -> Result<String, Error> {
    String::from_utf8(get_blob(conn, h)?).map_err(|e| Error::Internal(e.to_string()))
}

fn hashes(
    conn: &Connection,
    sql: &str,
    p: impl rusqlite::Params,
) -> Result<BTreeMap<String, String>, Error> {
    let mut st = conn.prepare(sql)?;
    let rows = st.query_map(p, |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn texts(conn: &Connection, h: &BTreeMap<String, String>) -> Result<Parts, Error> {
    h.iter()
        .map(|(p, h)| Ok((p.clone(), get_text(conn, h)?)))
        .collect()
}

fn validate_meta(id: &str, meta: &Value) -> Result<(), Error> {
    if !meta.is_object() {
        return Err(Error::BadRequest("project must be an object".into()));
    }
    if meta.get("id").and_then(Value::as_str) != Some(id) {
        return Err(Error::BadRequest("project id does not match".into()));
    }
    Ok(())
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl Store {
    pub fn open(path: &Path) -> Result<Store, Error> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Store, Error> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store, Error> {
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn })
    }

    // ── Users and sessions ────────────────────────────────────────────────

    pub fn create_user(&self, name: &str, password: &str) -> Result<User, Error> {
        let name = name.trim();
        if name.is_empty() || name.len() > 64 {
            return Err(Error::BadRequest("user names are 1–64 characters".into()));
        }
        let h = crate::auth::hash_password(password)?;
        self.conn
            .execute(
                "INSERT INTO users (name, pw_hash, created_at) VALUES (?1, ?2, ?3)",
                params![name, h, now_ms()],
            )
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(f, _)
                    if f.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    Error::Conflict(format!("there is already a user called {name}"))
                }
                e => e.into(),
            })?;
        Ok(User {
            id: self.conn.last_insert_rowid(),
            name: name.to_owned(),
        })
    }

    pub fn set_password(&self, name: &str, password: &str) -> Result<(), Error> {
        let h = crate::auth::hash_password(password)?;
        let n = self.conn.execute(
            "UPDATE users SET pw_hash = ?1 WHERE name = ?2",
            params![h, name],
        )?;
        if n == 0 {
            return Err(Error::NotFound);
        }
        // A new password signs out every device.
        self.conn.execute(
            "DELETE FROM sessions WHERE user_id = (SELECT id FROM users WHERE name = ?1)",
            [name],
        )?;
        Ok(())
    }

    pub fn delete_user(&self, name: &str) -> Result<(), Error> {
        if self
            .conn
            .execute("DELETE FROM users WHERE name = ?1", [name])?
            == 0
        {
            return Err(Error::NotFound);
        }
        self.collect_garbage()
    }

    pub fn users(&self) -> Result<Vec<User>, Error> {
        let mut st = self
            .conn
            .prepare("SELECT id, name FROM users ORDER BY name")?;
        let rows = st.query_map([], |r| {
            Ok(User {
                id: r.get(0)?,
                name: r.get(1)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn user_by_name(&self, name: &str) -> Result<Option<User>, Error> {
        Ok(self
            .conn
            .query_row("SELECT id, name FROM users WHERE name = ?1", [name], |r| {
                Ok(User {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })
            .optional()?)
    }

    pub fn verify_user(&self, name: &str, password: &str) -> Result<User, Error> {
        let row: Option<(i64, String, String)> = self
            .conn
            .query_row(
                "SELECT id, name, pw_hash FROM users WHERE name = ?1",
                [name.trim()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        match row {
            Some((id, name, h)) if crate::auth::verify_password(password, &h) => {
                Ok(User { id, name })
            }
            Some(_) => Err(Error::Auth),
            None => {
                // Same cost as a wrong password, so user names can't be probed by timing.
                crate::auth::verify_password(password, crate::auth::dummy_hash());
                Err(Error::Auth)
            }
        }
    }

    /// A new session for the user; returns the token to hand to the client.
    pub fn create_session(&self, user: &User, label: &str) -> Result<String, Error> {
        let token = crate::auth::new_token();
        let t = now_ms();
        self.conn.execute(
            "INSERT INTO sessions (token_hash, user_id, label, created_at, last_seen) VALUES (?1, ?2, ?3, ?4, ?4)",
            params![hash(token.as_bytes()), user.id, label, t],
        )?;
        Ok(token)
    }

    pub fn session_user(&self, token: &str) -> Result<Option<User>, Error> {
        let h = hash(token.as_bytes());
        let user = self
            .conn
            .query_row(
                "SELECT u.id, u.name, s.last_seen FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.token_hash = ?1",
                [&h],
                |r| Ok((User { id: r.get(0)?, name: r.get(1)? }, r.get::<_, i64>(2)?)),
            )
            .optional()?;
        let Some((user, last_seen)) = user else {
            return Ok(None);
        };
        let t = now_ms();
        if t - last_seen > 3_600_000 {
            self.conn.execute(
                "UPDATE sessions SET last_seen = ?1 WHERE token_hash = ?2",
                params![t, h],
            )?;
        }
        Ok(Some(user))
    }

    pub fn delete_session(&self, token: &str) -> Result<(), Error> {
        self.conn.execute(
            "DELETE FROM sessions WHERE token_hash = ?1",
            [hash(token.as_bytes())],
        )?;
        Ok(())
    }

    // ── Projects ──────────────────────────────────────────────────────────

    fn owned(&self, user: &User, id: &str) -> Result<(i64, bool), Error> {
        let row: Option<(i64, i64, bool)> = self
            .conn
            .query_row(
                "SELECT user_id, rev, deleted FROM projects WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        match row {
            Some((uid, rev, deleted)) if uid == user.id => Ok((rev, deleted)),
            _ => Err(Error::NotFound),
        }
    }

    fn live(&self, user: &User, id: &str) -> Result<i64, Error> {
        match self.owned(user, id)? {
            (_, true) => Err(Error::NotFound),
            (rev, false) => Ok(rev),
        }
    }

    /// Live projects, most recently updated first.
    pub fn list_projects(&self, user: &User) -> Result<Vec<Value>, Error> {
        let mut st = self.conn.prepare(
            "SELECT meta FROM projects WHERE user_id = ?1 AND deleted = 0 ORDER BY updated_at DESC",
        )?;
        let rows = st.query_map([user.id], |r| r.get::<_, String>(0))?;
        rows.map(|m| serde_json::from_str(&m?).map_err(|e| Error::Internal(e.to_string())))
            .collect()
    }

    /// All projects including deleted ones, for syncing.
    pub fn summaries(&self, user: &User) -> Result<Vec<Summary>, Error> {
        let mut st = self.conn.prepare(
            "SELECT id, rev, updated_at, deleted, meta FROM projects WHERE user_id = ?1 ORDER BY updated_at DESC",
        )?;
        let rows = st.query_map([user.id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        rows.map(|row| {
            let (id, rev, updated_at, deleted, meta) = row?;
            let meta = serde_json::from_str(&meta).map_err(|e| Error::Internal(e.to_string()))?;
            Ok(Summary {
                id,
                rev,
                updated_at,
                deleted,
                meta,
            })
        })
        .collect()
    }

    pub fn get_project(&self, user: &User, id: &str) -> Result<Value, Error> {
        self.live(user, id)?;
        let meta: String =
            self.conn
                .query_row("SELECT meta FROM projects WHERE id = ?1", [id], |r| {
                    r.get(0)
                })?;
        serde_json::from_str(&meta).map_err(|e| Error::Internal(e.to_string()))
    }

    /// Creates a project from the original file and its normalised text
    /// parts. Creating one that exists with the same original is a no-op, so
    /// a retried upload or a second sync is harmless.
    pub fn create_project(
        &mut self,
        user: &User,
        meta: &Value,
        docx: &[u8],
        source: &Parts,
    ) -> Result<(), Error> {
        let id = meta
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if !valid_id(&id) {
            return Err(Error::BadRequest("invalid project id".into()));
        }
        validate_meta(&id, meta)?;
        let updated_at = meta
            .get("updatedAt")
            .and_then(Value::as_i64)
            .unwrap_or_else(now_ms);
        let tx = self.conn.transaction()?;
        let existing: Option<(i64, Option<String>, bool)> = tx
            .query_row(
                "SELECT user_id, docx, deleted FROM projects WHERE id = ?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let docx_hash = put_blob(&tx, docx)?;
        match existing {
            Some((uid, Some(h), false)) if uid == user.id && h == docx_hash => return Ok(()),
            Some((uid, _, true)) if uid == user.id => {
                return Err(Error::Conflict("that translation was deleted".into()));
            }
            Some(_) => return Err(Error::Conflict("a project with that id exists".into())),
            None => {}
        }
        tx.execute(
            "INSERT INTO projects (id, user_id, meta, rev, updated_at, docx) VALUES (?1, ?2, ?3, 0, ?4, ?5)",
            params![id, user.id, meta.to_string(), updated_at, docx_hash],
        )?;
        for (path, xml) in source {
            let h = put_blob(&tx, xml.as_bytes())?;
            tx.execute(
                "INSERT INTO source_parts (project_id, path, hash) VALUES (?1, ?2, ?3)",
                params![id, path, h],
            )?;
        }
        tx.execute(
            "INSERT INTO revisions (project_id, rev, created_at) VALUES (?1, 0, ?2)",
            params![id, now_ms()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn source_parts(&self, user: &User, id: &str) -> Result<Parts, Error> {
        self.live(user, id)?;
        let h = hashes(
            &self.conn,
            "SELECT path, hash FROM source_parts WHERE project_id = ?1",
            [id],
        )?;
        texts(&self.conn, &h)
    }

    pub fn source_docx(&self, user: &User, id: &str) -> Result<Vec<u8>, Error> {
        self.live(user, id)?;
        let h: String =
            self.conn
                .query_row("SELECT docx FROM projects WHERE id = ?1", [id], |r| {
                    r.get(0)
                })?;
        get_blob(&self.conn, &h)
    }

    /// The translated parts at the latest revision (parts never translated
    /// are absent; the source stands for them).
    pub fn parts(&self, user: &User, id: &str) -> Result<(i64, Parts), Error> {
        let rev = self.live(user, id)?;
        Ok((rev, self.parts_at_unchecked(id, rev)?))
    }

    /// The translated parts at a given revision, if it is still kept.
    pub fn parts_at(&self, user: &User, id: &str, rev: i64) -> Result<Option<Parts>, Error> {
        self.live(user, id)?;
        if !self.has_rev(id, rev)? {
            return Ok(None);
        }
        Ok(Some(self.parts_at_unchecked(id, rev)?))
    }

    fn has_rev(&self, id: &str, rev: i64) -> Result<bool, Error> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM revisions WHERE project_id = ?1 AND rev = ?2",
                params![id, rev],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false))
    }

    fn rev_hashes(&self, id: &str, rev: i64) -> Result<BTreeMap<String, String>, Error> {
        hashes(
            &self.conn,
            "SELECT path, hash FROM rev_parts WHERE project_id = ?1 AND rev = ?2",
            params![id, rev],
        )
    }

    fn parts_at_unchecked(&self, id: &str, rev: i64) -> Result<Parts, Error> {
        let h = self.rev_hashes(id, rev)?;
        texts(&self.conn, &h)
    }

    /// Saves translated parts made from revision `base_rev`. Paths not in
    /// `parts` are unchanged. If other saves came in since `base_rev`, the
    /// parts are merged paragraph by paragraph (see [`merge`]); a paragraph
    /// edited on both sides takes this save's text, and the stored state from
    /// before is kept as a version.
    pub fn save_parts(
        &mut self,
        user: &User,
        id: &str,
        base_rev: i64,
        meta: Option<&Value>,
        parts: &Parts,
        origin: Option<&str>,
    ) -> Result<Saved, Error> {
        let head = self.live(user, id)?;
        if let Some(m) = meta {
            validate_meta(id, m)?;
        }
        if base_rev > head || base_rev < 0 {
            return Err(Error::BadRequest(format!(
                "revision {base_rev} is ahead of the server ({head})"
            )));
        }
        let head_h = self.rev_hashes(id, head)?;
        let mut out_h = head_h.clone();
        let mut conflicts = vec![];
        let merged = base_rev != head;

        // What each path held before any translation: the normalised source.
        let src_h = hashes(
            &self.conn,
            "SELECT path, hash FROM source_parts WHERE project_id = ?1",
            [id],
        )?;
        let base_h = if merged && self.has_rev(id, base_rev)? {
            Some(self.rev_hashes(id, base_rev)?)
        } else {
            None
        };

        let mut resolved: Parts = BTreeMap::new();
        for (path, ours) in parts {
            let ours_h = hash(ours.as_bytes());
            let theirs_h = head_h.get(path).or_else(|| src_h.get(path));
            if !merged || theirs_h.is_none() {
                resolved.insert(path.clone(), ours.clone());
                continue;
            }
            let theirs_h = theirs_h.unwrap();
            if *theirs_h == ours_h {
                continue;
            }
            let base_hash = match &base_h {
                Some(b) => b.get(path).or_else(|| src_h.get(path)).cloned(),
                None => None,
            };
            if base_hash.as_deref() == Some(theirs_h) {
                // Nobody else touched this part.
                resolved.insert(path.clone(), ours.clone());
                continue;
            }
            let theirs = get_text(&self.conn, theirs_h)?;
            let base = base_hash.map(|h| get_text(&self.conn, &h)).transpose()?;
            let m = merge::merge3(base.as_deref(), ours, &theirs, Kind::for_path(path))
                .map_err(|e| Error::BadRequest(e.to_string()))?;
            conflicts.extend(m.conflicts.into_iter().map(|c| format!("{path}: {c}")));
            resolved.insert(path.clone(), m.xml);
        }

        let t = now_ms();
        let tx = self.conn.transaction()?;
        if !conflicts.is_empty() {
            // Keep what this save overwrote.
            let sid = uuid::Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO versions (sid, project_id, time, reason, stats) VALUES (?1, ?2, ?3, ?4, NULL)",
                params![sid, id, t, "before merging edits from another device"],
            )?;
            for (path, h) in &head_h {
                tx.execute(
                    "INSERT INTO version_parts (sid, path, hash) VALUES (?1, ?2, ?3)",
                    params![sid, path, h],
                )?;
            }
        }
        for (path, xml) in &resolved {
            out_h.insert(path.clone(), put_blob(&tx, xml.as_bytes())?);
        }
        let rev = head + 1;
        tx.execute(
            "INSERT INTO revisions (project_id, rev, created_at, origin) VALUES (?1, ?2, ?3, ?4)",
            params![id, rev, t, origin],
        )?;
        for (path, h) in &out_h {
            tx.execute(
                "INSERT INTO rev_parts (project_id, rev, path, hash) VALUES (?1, ?2, ?3, ?4)",
                params![id, rev, path, h],
            )?;
        }
        // "Updated" is the editor's notion (it is in the metadata); a save
        // without metadata (syncing) leaves it alone.
        match meta {
            Some(m) => {
                let updated_at = m.get("updatedAt").and_then(Value::as_i64).unwrap_or(t);
                tx.execute(
                    "UPDATE projects SET rev = ?1, meta = ?2, updated_at = ?3 WHERE id = ?4",
                    params![rev, m.to_string(), updated_at, id],
                )?
            }
            None => tx.execute("UPDATE projects SET rev = ?1 WHERE id = ?2", params![rev, id])?,
        };
        tx.execute(
            "DELETE FROM revisions WHERE project_id = ?1 AND rev <= ?2",
            params![id, rev - KEEP_REVISIONS],
        )?;
        tx.commit()?;
        if rev % 50 == 0 {
            self.collect_garbage()?;
        }
        let parts = if merged {
            Some(self.parts_at_unchecked(id, rev)?)
        } else {
            None
        };
        Ok(Saved {
            rev,
            merged,
            conflicts,
            parts,
        })
    }

    /// Replaces the project's metadata (name, language, done paragraphs…).
    pub fn save_meta(&self, user: &User, id: &str, meta: &Value) -> Result<(), Error> {
        self.live(user, id)?;
        validate_meta(id, meta)?;
        let updated_at = meta
            .get("updatedAt")
            .and_then(Value::as_i64)
            .unwrap_or_else(now_ms);
        self.conn.execute(
            "UPDATE projects SET meta = ?1, updated_at = ?2 WHERE id = ?3",
            params![meta.to_string(), updated_at, id],
        )?;
        Ok(())
    }

    /// Deletes everything but a tombstone, so the deletion reaches other
    /// devices when they sync.
    pub fn delete_project(&mut self, user: &User, id: &str) -> Result<(), Error> {
        let (_, deleted) = self.owned(user, id)?;
        if deleted {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM versions WHERE project_id = ?1", [id])?;
        tx.execute("DELETE FROM revisions WHERE project_id = ?1", [id])?;
        tx.execute("DELETE FROM source_parts WHERE project_id = ?1", [id])?;
        tx.execute(
            "UPDATE projects SET deleted = 1, docx = NULL, updated_at = ?1 WHERE id = ?2",
            params![now_ms(), id],
        )?;
        tx.commit()?;
        self.collect_garbage()
    }

    /// Drops deleted projects' tombstones entirely (local store only).
    pub fn forget_project(&mut self, user: &User, id: &str) -> Result<(), Error> {
        self.owned(user, id)?;
        self.conn
            .execute("DELETE FROM projects WHERE id = ?1", [id])?;
        self.collect_garbage()
    }

    // ── Versions ──────────────────────────────────────────────────────────

    /// Adds a version. With `sid` (from another device) adding the same one
    /// twice is a no-op.
    pub fn add_version(&mut self, user: &User, id: &str, v: &Version) -> Result<String, Error> {
        self.live(user, id)?;
        let parts = v
            .parts
            .as_ref()
            .ok_or_else(|| Error::BadRequest("a version needs parts".into()))?;
        let sid = if v.sid.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            v.sid.clone()
        };
        if !valid_id(&sid) {
            return Err(Error::BadRequest("invalid version id".into()));
        }
        let tx = self.conn.transaction()?;
        let owner: Option<String> = tx
            .query_row(
                "SELECT project_id FROM versions WHERE sid = ?1",
                [&sid],
                |r| r.get(0),
            )
            .optional()?;
        match owner {
            Some(p) if p == id => return Ok(sid),
            Some(_) => return Err(Error::Conflict("a version with that id exists".into())),
            None => {}
        }
        tx.execute(
            "INSERT INTO versions (sid, project_id, time, reason, stats) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![sid, id, v.time, v.reason, v.stats.to_string()],
        )?;
        for (path, xml) in parts {
            let h = put_blob(&tx, xml.as_bytes())?;
            tx.execute(
                "INSERT INTO version_parts (sid, path, hash) VALUES (?1, ?2, ?3)",
                params![sid, path, h],
            )?;
        }
        // Prune the oldest automatic versions beyond the limit; named ones stay.
        tx.execute(
            "DELETE FROM versions WHERE sid IN (
               SELECT sid FROM versions WHERE project_id = ?1 AND reason = 'auto' ORDER BY time
               LIMIT max(0, (SELECT count(*) FROM versions WHERE project_id = ?1) - ?2))",
            params![id, KEEP_VERSIONS],
        )?;
        tx.commit()?;
        Ok(sid)
    }

    /// Newest first, without parts.
    pub fn list_versions(&self, user: &User, id: &str) -> Result<Vec<Version>, Error> {
        self.live(user, id)?;
        let mut st = self
            .conn
            .prepare("SELECT sid, time, reason, stats FROM versions WHERE project_id = ?1 ORDER BY time DESC")?;
        let rows = st.query_map([id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (sid, time, reason, stats) = row?;
            let stats = stats
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(Value::Null);
            Ok(Version {
                sid,
                time,
                reason,
                stats,
                parts: None,
            })
        })
        .collect()
    }

    /// One version with its parts. Returns the project id too.
    pub fn get_version(&self, user: &User, sid: &str) -> Result<(String, Version), Error> {
        let row: Option<(String, i64, String, Option<String>)> = self
            .conn
            .query_row(
                "SELECT project_id, time, reason, stats FROM versions WHERE sid = ?1",
                [sid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let (pid, time, reason, stats) = row.ok_or(Error::NotFound)?;
        self.live(user, &pid)?;
        let h = hashes(
            &self.conn,
            "SELECT path, hash FROM version_parts WHERE sid = ?1",
            [sid],
        )?;
        let stats = stats
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        let parts = Some(texts(&self.conn, &h)?);
        Ok((
            pid,
            Version {
                sid: sid.to_owned(),
                time,
                reason,
                stats,
                parts,
            },
        ))
    }

    // ── Housekeeping ──────────────────────────────────────────────────────

    /// Removes part contents nothing refers to any more.
    pub fn collect_garbage(&self) -> Result<(), Error> {
        self.conn.execute(
            "DELETE FROM blobs WHERE hash NOT IN (
               SELECT hash FROM rev_parts UNION SELECT hash FROM version_parts
               UNION SELECT hash FROM source_parts UNION SELECT docx FROM projects WHERE docx IS NOT NULL)",
            [],
        )?;
        Ok(())
    }

    pub fn get_kv(&self, key: &str) -> Result<Option<String>, Error> {
        Ok(self
            .conn
            .query_row("SELECT value FROM kv WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    pub fn set_kv(&self, key: &str, value: Option<&str>) -> Result<(), Error> {
        match value {
            Some(v) => self.conn.execute(
                "INSERT INTO kv (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, v],
            )?,
            None => self.conn.execute("DELETE FROM kv WHERE key = ?1", [key])?,
        };
        Ok(())
    }
}
