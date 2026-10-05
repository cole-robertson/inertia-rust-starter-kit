//! Stateless, purpose-scoped, expiring tokens — the equivalent of Rails'
//! `generates_token_for`.
//!
//! A token is `base64url(payload) "." base64url(hmac)` where
//!
//! ```text
//! payload = {"id": <user id>, "purpose": "<purpose>", "exp": <unix seconds>, "fp": "<fingerprint>"}
//! hmac    = HMAC-SHA256(key, payload bytes)
//! key     = HMAC-SHA256(secret_key_base, "tokens/" + purpose)
//! ```
//!
//! Nothing is stored in the database. A token stops working when
//!
//! * it expires (`exp` is checked against an injectable [`Clock`]),
//! * it is presented for a different purpose (the purpose is part of both the
//!   derived key and the payload), or
//! * the record's fingerprint changes — for email verification the fingerprint is
//!   the current email, for password resets the last 10 characters of the
//!   password hash (inside the salt/hash segment), so a reset token dies as soon
//!   as the password changes. This mirrors the Rails kit's
//!   `generates_token_for ... { password_salt.last(10) }`.
//!
//! The MAC is compared in constant time, and the fingerprint is compared in
//! constant time as well.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Source of "now". Production code uses [`SystemClock`]; tests use
/// [`FixedClock`] to travel through time like Rails' `travel 3.days`.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FixedClock(pub DateTime<Utc>);

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

/// What a token is for. Each purpose has its own lifetime and its own derived
/// key, so a token minted for one purpose can never be replayed for another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    /// 2 days, bound to the user's current email.
    EmailVerification,
    /// 20 minutes, bound to the tail of the password hash.
    PasswordReset,
}

impl Purpose {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EmailVerification => "email_verification",
            Self::PasswordReset => "password_reset",
        }
    }

    #[must_use]
    pub fn expires_in(self) -> Duration {
        match self {
            Self::EmailVerification => Duration::days(2),
            Self::PasswordReset => Duration::minutes(20),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Payload {
    id: i64,
    purpose: String,
    exp: i64,
    fp: String,
}

/// Why a token was rejected. Callers normally treat every variant the same
/// ("That link is invalid"); the variants exist for logging and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenError {
    Malformed,
    BadSignature,
    WrongPurpose,
    Expired,
    FingerprintMismatch,
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Malformed => "malformed token",
            Self::BadSignature => "bad token signature",
            Self::WrongPurpose => "token purpose mismatch",
            Self::Expired => "token expired",
            Self::FingerprintMismatch => "token no longer valid for this record",
        };
        f.write_str(s)
    }
}

impl std::error::Error for TokenError {}

fn derived_key(secret_key_base: &[u8], purpose: Purpose) -> HmacSha256 {
    let mut kdf = HmacSha256::new_from_slice(secret_key_base).expect("HMAC accepts any key size");
    kdf.update(b"tokens/");
    kdf.update(purpose.as_str().as_bytes());
    let key = kdf.finalize().into_bytes();
    HmacSha256::new_from_slice(&key).expect("HMAC accepts any key size")
}

/// Mint a token for record `id` with the given fingerprint.
#[must_use]
pub fn generate(
    secret_key_base: &[u8],
    purpose: Purpose,
    id: i64,
    fingerprint: &str,
    clock: &dyn Clock,
) -> String {
    let payload = Payload {
        id,
        purpose: purpose.as_str().to_string(),
        exp: (clock.now() + purpose.expires_in()).timestamp(),
        fp: fingerprint.to_string(),
    };
    // Serializing a struct of plain strings/ints cannot fail.
    let bytes = serde_json::to_vec(&payload).unwrap_or_default();
    let mut mac = derived_key(secret_key_base, purpose);
    mac.update(&bytes);
    let sig = mac.finalize().into_bytes();
    format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(&bytes),
        URL_SAFE_NO_PAD.encode(sig)
    )
}

/// Verified-but-not-yet-fingerprint-checked token contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claims {
    pub id: i64,
    fingerprint: String,
}

impl Claims {
    /// Check the fingerprint carried by the token against the record's current
    /// fingerprint, in constant time.
    ///
    /// # Errors
    /// [`TokenError::FingerprintMismatch`] when the record changed since minting.
    pub fn check_fingerprint(&self, current: &str) -> Result<(), TokenError> {
        let a = self.fingerprint.as_bytes();
        let b = current.as_bytes();
        if a.len() == b.len() && bool::from(a.ct_eq(b)) {
            Ok(())
        } else {
            Err(TokenError::FingerprintMismatch)
        }
    }
}

/// Verify signature, purpose and expiry. The caller must still load the record
/// by `claims.id` and call [`Claims::check_fingerprint`].
///
/// # Errors
/// Any [`TokenError`] except `FingerprintMismatch`.
pub fn verify(
    secret_key_base: &[u8],
    purpose: Purpose,
    token: &str,
    clock: &dyn Clock,
) -> Result<Claims, TokenError> {
    let (payload_b64, sig_b64) = token.split_once('.').ok_or(TokenError::Malformed)?;
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|_| TokenError::Malformed)?;
    let sig = URL_SAFE_NO_PAD
        .decode(sig_b64)
        .map_err(|_| TokenError::Malformed)?;

    let mut mac = derived_key(secret_key_base, purpose);
    mac.update(&payload_bytes);
    // `verify_slice` is constant time.
    mac.verify_slice(&sig)
        .map_err(|_| TokenError::BadSignature)?;

    let payload: Payload =
        serde_json::from_slice(&payload_bytes).map_err(|_| TokenError::Malformed)?;
    if payload.purpose != purpose.as_str() {
        return Err(TokenError::WrongPurpose);
    }
    if clock.now().timestamp() >= payload.exp {
        return Err(TokenError::Expired);
    }
    Ok(Claims {
        id: payload.id,
        fingerprint: payload.fp,
    })
}
