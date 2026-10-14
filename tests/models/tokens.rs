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

/// The user tokens' format: a token minted before the signed-token helpers were added still
/// verifies (computed independently: HMAC-SHA256 under HMAC(KEY, "tokens/email_verification")).
#[test]
fn the_user_token_format_is_unchanged() {
    let token = tokens::generate(
        KEY,
        Purpose::EmailVerification,
        42,
        "one@example.com",
        &at(0),
    );
    assert_eq!(
        token,
        "eyJpZCI6NDIsInB1cnBvc2UiOiJlbWFpbF92ZXJpZmljYXRpb24iLCJleHAiOjE3Njc0NDE2MDAsImZwIjoib25lQGV4YW1wbGUuY29tIn0.RCbFBu7cBonBf02at6UHkrA_WFLgu_HlcPEbmKkPQfU"
    );
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Vote {
    item_id: i64,
    vote: i8,
}

const VOTE: Vote = Vote {
    item_id: 7,
    vote: -1,
};

/// `tokens::sign`: a one-click link for a record and an action, with no user (a digest's vote).
#[test]
fn signed_data_round_trips_until_it_expires() {
    let token = tokens::sign(KEY, "vote", &VOTE, Duration::days(30), &at(0)).unwrap();
    let thirty_days = 30 * 24 * 60;
    assert_eq!(
        tokens::verify_signed::<Vote>(KEY, "vote", &token, &at(thirty_days - 1)),
        Ok(VOTE)
    );
    assert_eq!(
        tokens::verify_signed::<Vote>(KEY, "vote", &token, &at(thirty_days)),
        Err(TokenError::Expired)
    );
}

#[test]
fn signed_data_is_bound_to_its_purpose_secret_and_bytes() {
    let token = tokens::sign(KEY, "vote", &VOTE, Duration::days(1), &at(0)).unwrap();
    assert_eq!(
        tokens::verify_signed::<Vote>(KEY, "unsubscribe", &token, &at(0)),
        Err(TokenError::BadSignature)
    );
    assert_eq!(
        tokens::verify_signed::<Vote>(b"another-secret", "vote", &token, &at(0)),
        Err(TokenError::BadSignature)
    );
    let (_, sig) = token.split_once('.').unwrap();
    let forged = format!(
        "{}.{sig}",
        base64_url(br#"{"purpose":"vote","exp":9999999999,"data":{"item_id":8,"vote":1}}"#)
    );
    assert_eq!(
        tokens::verify_signed::<Vote>(KEY, "vote", &forged, &at(0)),
        Err(TokenError::BadSignature)
    );
    assert_eq!(
        tokens::verify_signed::<Vote>(KEY, "vote", "garbage", &at(0)),
        Err(TokenError::Malformed)
    );
}

/// A signed token and a user token never stand in for each other, even under the same purpose
/// name: their keys are derived apart ("signed/" and "tokens/").
#[test]
fn signed_and_user_tokens_have_separate_keys() {
    let user = tokens::generate(KEY, Purpose::PasswordReset, 1, "fp", &at(0));
    assert_eq!(
        tokens::verify_signed::<serde_json::Value>(KEY, "password_reset", &user, &at(0)),
        Err(TokenError::BadSignature)
    );
    let signed = tokens::sign(
        KEY,
        "password_reset",
        &serde_json::json!({ "id": 1, "fp": "fp" }),
        Duration::minutes(20),
        &at(0),
    )
    .unwrap();
    assert_eq!(
        tokens::verify(KEY, Purpose::PasswordReset, &signed, &at(0)),
        Err(TokenError::BadSignature)
    );
}
