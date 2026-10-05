//! The stateless token primitive, independent of the database.

use chrono::{Duration, TimeZone, Utc};
use inertia_rust_starter_kit::models::tokens::{self, FixedClock, Purpose, TokenError};

const KEY: &[u8] = b"test-secret-key-base-that-is-long-enough-for-hmac-sha256-0123456789";

fn at(minutes: i64) -> FixedClock {
    FixedClock(Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap() + Duration::minutes(minutes))
}

#[test]
fn round_trips_id_and_fingerprint() {
    let token = tokens::generate(
        KEY,
        Purpose::EmailVerification,
        42,
        "one@example.com",
        &at(0),
    );
    let claims = tokens::verify(KEY, Purpose::EmailVerification, &token, &at(1)).unwrap();
    assert_eq!(claims.id, 42);
    assert_eq!(claims.check_fingerprint("one@example.com"), Ok(()));
    assert_eq!(
        claims.check_fingerprint("changed@example.com"),
        Err(TokenError::FingerprintMismatch)
    );
}

#[test]
fn email_verification_lasts_two_days() {
    let token = tokens::generate(KEY, Purpose::EmailVerification, 1, "fp", &at(0));
    let two_days = 2 * 24 * 60;
    assert!(tokens::verify(KEY, Purpose::EmailVerification, &token, &at(two_days - 1)).is_ok());
    assert_eq!(
        tokens::verify(KEY, Purpose::EmailVerification, &token, &at(two_days)),
        Err(TokenError::Expired)
    );
}

#[test]
fn password_reset_lasts_twenty_minutes() {
    let token = tokens::generate(KEY, Purpose::PasswordReset, 1, "fp", &at(0));
    assert!(tokens::verify(KEY, Purpose::PasswordReset, &token, &at(19)).is_ok());
    assert_eq!(
        tokens::verify(KEY, Purpose::PasswordReset, &token, &at(20)),
        Err(TokenError::Expired)
    );
}

#[test]
fn a_token_cannot_be_used_for_another_purpose() {
    let token = tokens::generate(KEY, Purpose::EmailVerification, 1, "fp", &at(0));
    // The purpose is folded into the derived key, so the MAC fails first.
    assert_eq!(
        tokens::verify(KEY, Purpose::PasswordReset, &token, &at(0)),
        Err(TokenError::BadSignature)
    );
}

#[test]
fn a_different_secret_rejects_the_token() {
    let token = tokens::generate(KEY, Purpose::PasswordReset, 1, "fp", &at(0));
    assert_eq!(
        tokens::verify(b"another-secret", Purpose::PasswordReset, &token, &at(0)),
        Err(TokenError::BadSignature)
    );
}

#[test]
fn tampering_with_the_payload_is_detected() {
    let token = tokens::generate(KEY, Purpose::PasswordReset, 1, "fp", &at(0));
    let (_, sig) = token.split_once('.').unwrap();
    // Forge a payload for user 2 and reuse the original signature.
    let forged_payload =
        base64_url(br#"{"id":2,"purpose":"password_reset","exp":9999999999,"fp":"fp"}"#);
    let forged = format!("{forged_payload}.{sig}");
    assert_eq!(
        tokens::verify(KEY, Purpose::PasswordReset, &forged, &at(0)),
        Err(TokenError::BadSignature)
    );
}

#[test]
fn garbage_is_malformed() {
    for garbage in ["", "invalid", "a.b.c", "!!!.???"] {
        assert_eq!(
            tokens::verify(KEY, Purpose::PasswordReset, garbage, &at(0)),
            Err(TokenError::Malformed),
            "{garbage:?}"
        );
    }
}

fn base64_url(bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    URL_SAFE_NO_PAD.encode(bytes)
}
