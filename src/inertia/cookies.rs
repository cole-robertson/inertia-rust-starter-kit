//! Cookie keys and helpers shared by the flash, CSRF, and session layers.
//!
//! Every purpose gets its own 64-byte key, derived from `secret_key_base` with
//! HMAC-SHA256 and a label, so a key leaked or misused for one cookie can't
//! forge another.

use axum::http::{header, HeaderMap, HeaderValue};
use cookie::{time::Duration, Cookie, CookieJar, Key, SameSite};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use super::config::Settings;

pub const FLASH_COOKIE: &str = "_flash";
pub const CSRF_SECRET_COOKIE: &str = "_csrf";
pub const XSRF_COOKIE: &str = "XSRF-TOKEN";
pub const SESSION_COOKIE: &str = "session_token";

/// Rails `cookies.permanent`: 20 years.
pub const PERMANENT_MAX_AGE: Duration = Duration::days(365 * 20);

/// 64 bytes of key material for `label`: HMAC-SHA256(secret_key_base, "inertia/<label>/<n>") for n = 0, 1.
pub fn derive_key(secret_key_base: &str, label: &str) -> Key {
    let mut material = [0u8; 64];
    for (n, chunk) in material.chunks_mut(32).enumerate() {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret_key_base.as_bytes())
            .expect("HMAC accepts keys of any length");
        mac.update(format!("inertia/{label}/{n}").as_bytes());
        chunk.copy_from_slice(&mac.finalize().into_bytes());
    }
    Key::from(&material)
}

pub fn flash_key(settings: &Settings) -> Key {
    settings.cookie_keys().flash.clone()
}

pub fn csrf_key(settings: &Settings) -> Key {
    settings.cookie_keys().csrf.clone()
}

pub fn session_cookie_key(settings: &Settings) -> Key {
    settings.cookie_keys().session.clone()
}

/// Cookies get `Secure` whenever the app is served over https, and in production
/// unless `allow_insecure_http` was set for local http testing — browsers drop
/// `Secure` cookies on http, which would break sessions and CSRF there.
pub fn is_secure(settings: &Settings) -> bool {
    let https = settings
        .app_url
        .trim()
        .to_ascii_lowercase()
        .starts_with("https://");
    https || (settings.production && !settings.allow_insecure_http)
}

/// Parses every `Cookie` request header into a jar (as originals, so only
/// cookies added later show up in `delta()`). Unparseable pairs are skipped.
pub fn request_jar(headers: &HeaderMap) -> CookieJar {
    let mut jar = CookieJar::new();
    for value in headers.get_all(header::COOKIE) {
        let Ok(value) = value.to_str() else { continue };
        for cookie in Cookie::split_parse_encoded(value.to_owned()).flatten() {
            jar.add_original(cookie.into_owned());
        }
    }
    jar
}

/// Appends one `Set-Cookie` header for `cookie` (value percent-encoded).
pub fn append_set_cookie(headers: &mut HeaderMap, cookie: &Cookie<'_>) {
    let value = HeaderValue::from_str(&cookie.encoded().to_string())
        .expect("an encoded cookie is a valid header value");
    headers.append(header::SET_COOKIE, value);
}

/// Base attributes every app cookie shares: Path=/, SameSite=Lax, Secure in
/// production or on https.
pub fn base_cookie(settings: &Settings, name: &'static str, value: String) -> Cookie<'static> {
    Cookie::build((name, value))
        .path("/")
        .same_site(SameSite::Lax)
        .secure(is_secure(settings))
        .build()
}

/// A cookie that deletes `name` (same Path, empty value, expired).
pub fn removal_cookie(settings: &Settings, name: &'static str) -> Cookie<'static> {
    let mut cookie = base_cookie(settings, name, String::new());
    cookie.set_http_only(true);
    cookie.make_removal();
    cookie
}

/// Signs `cookie`'s value with `key` (HMAC, value stays readable).
pub fn sign(key: &Key, cookie: Cookie<'static>) -> Cookie<'static> {
    let name = cookie.name().to_owned();
    let mut jar = CookieJar::new();
    jar.signed_mut(key).add(cookie);
    jar.get(&name).cloned().expect("cookie was just added")
}

/// Verifies a signed cookie from the request; `None` if absent or tampered.
pub fn read_signed(headers: &HeaderMap, key: &Key, name: &str) -> Option<String> {
    request_jar(headers)
        .signed(key)
        .get(name)
        .map(|c| c.value().to_owned())
}

/// Encrypts + authenticates `cookie`'s value with `key`.
pub fn encrypt(key: &Key, cookie: Cookie<'static>) -> Cookie<'static> {
    let name = cookie.name().to_owned();
    let mut jar = CookieJar::new();
    jar.private_mut(key).add(cookie);
    jar.get(&name).cloned().expect("cookie was just added")
}

/// Decrypts a private cookie from the request; `None` if absent or tampered.
pub fn read_private(headers: &HeaderMap, key: &Key, name: &str) -> Option<String> {
    request_jar(headers)
        .private(key)
        .get(name)
        .map(|c| c.value().to_owned())
}

/// The signed, permanent `session_token` cookie: HttpOnly, SameSite=Lax,
/// Path=/, Secure on https, Max-Age 20 years.
pub fn session_token_cookie(settings: &Settings, token: &str) -> Cookie<'static> {
    let mut cookie = base_cookie(settings, SESSION_COOKIE, token.to_owned());
    cookie.set_http_only(true);
    cookie.set_max_age(PERMANENT_MAX_AGE);
    sign(&session_cookie_key(settings), cookie)
}

/// Adds the signed `session_token` Set-Cookie to response headers.
pub fn set_session_token(headers: &mut HeaderMap, settings: &Settings, token: &str) {
    append_set_cookie(headers, &session_token_cookie(settings, token));
}

/// The verified `session_token` from the request, if present and untampered.
pub fn read_session_token(headers: &HeaderMap, settings: &Settings) -> Option<String> {
    read_signed(headers, &session_cookie_key(settings), SESSION_COOKIE)
}

/// Adds a Set-Cookie that deletes `session_token` (sign-out).
pub fn clear_session_token(headers: &mut HeaderMap, settings: &Settings) {
    append_set_cookie(headers, &removal_cookie(settings, SESSION_COOKIE));
}
