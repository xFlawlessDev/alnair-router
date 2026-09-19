//! PKCE (RFC 7636) helpers and the random nonces the login flows need.
//!
//! Randomness comes from the OS CSPRNG reached through `argon2`'s `rand_core`
//! re-export, the same source `auth.rs` uses for password salts — no extra
//! dependency is pulled in for this.

use argon2::password_hash::rand_core::{OsRng, RngCore};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

/// 32 random bytes as unpadded base64url (43 characters).
fn random_bytes() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

/// Opaque identifier for a pending login.
pub fn random_id() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes())
}

/// The `state` parameter, binding a callback to the login that started it.
pub fn random_state() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes())
}

/// A PKCE code verifier: 43 characters from RFC 7636's unreserved set, inside
/// the 43–128 range the spec requires.
pub fn code_verifier() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes())
}

/// The `S256` challenge for `verifier`: `base64url(sha256(verifier))`, unpadded.
pub fn code_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}
