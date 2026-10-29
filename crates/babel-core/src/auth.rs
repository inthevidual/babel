//! Passwords and session tokens.

use std::sync::OnceLock;

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use base64::Engine;

use crate::Error;

/// A real hash of an unguessable password, verified against when the user
/// doesn't exist so that the answer takes as long either way.
pub fn dummy_hash() -> &'static str {
    static H: OnceLock<String> = OnceLock::new();
    H.get_or_init(|| hash_password(&new_token()).expect("hashing a random password"))
}

pub fn hash_password(password: &str) -> Result<String, Error> {
    if password.chars().count() < 8 {
        return Err(Error::BadRequest(
            "passwords are at least 8 characters".into(),
        ));
    }
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h: PasswordHash| h.to_string())
        .map_err(|e| Error::Internal(e.to_string()))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|h| {
            Argon2::default()
                .verify_password(password.as_bytes(), &h)
                .is_ok()
        })
        .unwrap_or(false)
}

/// 256 random bits, URL-safe.
pub fn new_token() -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random::<32>())
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("the OS random number generator failed");
    b
}
