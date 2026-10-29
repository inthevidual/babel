//! Babel's storage and merging, shared by the server and the desktop app.

pub mod auth;
pub mod merge;
pub mod store;

pub use store::{Parts, Saved, Store, Summary, User, Version};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not found")]
    NotFound,
    #[error("wrong user name or password")]
    Auth,
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Internal(String),
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
}
