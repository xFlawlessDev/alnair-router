//! Credential encryption at rest.
//!
//! Upstream `api_key` values are encrypted with AES-256-GCM before they reach
//! SQLite. The key comes from `secrets.key` (64 hex characters or base64 of 32
//! bytes); the router refuses to start without one. Stored values carry an
//! `enc:v1:` marker so legacy plaintext rows can be detected and rewritten by
//! [`crate::db::Db::migrate_credentials`].

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;

use crate::config::SecretsConfig;
use crate::error::{Error, Result};

const MARKER: &str = "enc:v1:";
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;

/// Encrypts and decrypts upstream credentials.
#[derive(Clone)]
pub struct CredentialCipher {
    key: Option<Key<Aes256Gcm>>,
}

impl CredentialCipher {
    /// Builds a cipher from configuration. A missing key yields a disabled
    /// cipher: reads still work for legacy plaintext, writes are refused.
    pub fn from_config(secrets: &SecretsConfig) -> Result<Self> {
        match &secrets.key {
            Some(raw) => Ok(Self {
                key: Some(parse_key(raw)?.into()),
            }),
            None => Ok(Self { key: None }),
        }
    }

    /// A random key that lives only for this process. Intended for in-memory
    /// databases and tests.
    pub fn ephemeral() -> Self {
        Self {
            key: Some(Aes256Gcm::generate_key(OsRng)),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.key.is_some()
    }

    /// True when `value` was produced by [`Self::encrypt`].
    pub fn is_encrypted(value: &str) -> bool {
        value.starts_with(MARKER)
    }

    /// Encrypts a plaintext credential. Fails when no key is configured rather
    /// than silently falling back to plaintext.
    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        let Some(key) = &self.key else {
            return Err(Error::Config(
                "secrets.key is not configured; refusing to store a credential in plaintext"
                    .to_string(),
            ));
        };

        let cipher = Aes256Gcm::new(key);
        let nonce = Aes256Gcm::generate_nonce(OsRng);
        let sealed = cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|_| Error::Config("failed to encrypt credential".to_string()))?;

        let mut payload = nonce.to_vec();
        payload.extend(sealed);
        Ok(format!(
            "{MARKER}{}",
            base64::engine::general_purpose::STANDARD.encode(payload)
        ))
    }

    /// Decrypts a stored credential.
    ///
    /// Unmarked values pass through unchanged so legacy plaintext rows stay
    /// readable until the boot migration rewrites them.
    pub fn decrypt(&self, stored: &str) -> Result<String> {
        if !Self::is_encrypted(stored) {
            return Ok(stored.to_string());
        }

        let Some(key) = &self.key else {
            return Err(Error::Config(
                "found an encrypted credential but secrets.key is not configured".to_string(),
            ));
        };

        let payload = base64::engine::general_purpose::STANDARD
            .decode(stored.trim_start_matches(MARKER))
            .map_err(|error| Error::Config(format!("credential is not valid base64: {error}")))?;
        if payload.len() <= NONCE_BYTES {
            return Err(Error::Config("credential payload is truncated".to_string()));
        }

        let (nonce, ciphertext) = payload.split_at(NONCE_BYTES);
        let cipher = Aes256Gcm::new(key);
        let plaintext = cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| {
                Error::Config(
                    "failed to decrypt credential: wrong secrets.key or corrupted database"
                        .to_string(),
                )
            })?;

        String::from_utf8(plaintext)
            .map_err(|error| Error::Config(format!("credential is not valid UTF-8: {error}")))
    }
}

/// Parses a 32-byte key from 64 hex characters or standard base64.
pub fn parse_key(raw: &str) -> Result<[u8; KEY_BYTES]> {
    let trimmed = raw.trim();

    if trimmed.len() == KEY_BYTES * 2 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        let mut bytes = [0u8; KEY_BYTES];
        for (index, pair) in trimmed.as_bytes().chunks(2).enumerate() {
            let pair = std::str::from_utf8(pair)
                .map_err(|error| Error::Config(format!("invalid secrets.key: {error}")))?;
            bytes[index] = u8::from_str_radix(pair, 16)
                .map_err(|error| Error::Config(format!("invalid secrets.key hex: {error}")))?;
        }
        return Ok(bytes);
    }

    let decoded = base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .map_err(|_| {
            Error::Config(
                "secrets.key must be 64 hex characters or base64-encoded 32 bytes".to_string(),
            )
        })?;

    decoded
        .try_into()
        .map_err(|_| Error::Config("secrets.key must decode to exactly 32 bytes".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher(raw: &str) -> CredentialCipher {
        CredentialCipher::from_config(&SecretsConfig {
            key: Some(raw.to_string()),
        })
        .expect("valid key")
    }

    #[test]
    fn round_trip_recovers_the_plaintext() {
        let cipher = cipher(&"ab".repeat(32));
        let encrypted = cipher.encrypt("sk-secret").expect("encrypt");

        assert!(CredentialCipher::is_encrypted(&encrypted));
        assert_ne!(encrypted, "sk-secret");
        assert_eq!(cipher.decrypt(&encrypted).expect("decrypt"), "sk-secret");
    }

    #[test]
    fn a_different_key_fails_to_decrypt() {
        let encrypted = cipher(&"ab".repeat(32))
            .encrypt("sk-secret")
            .expect("encrypt");
        let other = cipher(&"cd".repeat(32));

        assert!(other.decrypt(&encrypted).is_err());
    }

    #[test]
    fn unmarked_values_pass_through_as_legacy_plaintext() {
        let cipher = cipher(&"ab".repeat(32));
        assert_eq!(cipher.decrypt("sk-plain").expect("decrypt"), "sk-plain");
    }

    #[test]
    fn disabled_cipher_refuses_to_encrypt() {
        let cipher = CredentialCipher::from_config(&SecretsConfig { key: None }).expect("cipher");
        assert!(!cipher.is_enabled());
        assert!(cipher.encrypt("sk-secret").is_err());
    }

    #[test]
    fn base64_keys_are_accepted() {
        let raw = base64::engine::general_purpose::STANDARD.encode([7u8; KEY_BYTES]);
        assert_eq!(parse_key(&raw).expect("parse"), [7u8; KEY_BYTES]);
    }

    #[test]
    fn wrong_length_keys_are_rejected() {
        assert!(parse_key("abcd").is_err());
        assert!(parse_key(&"ab".repeat(16)).is_err());
    }
}
