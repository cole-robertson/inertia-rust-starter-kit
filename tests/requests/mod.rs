//! Request tests: every spec of the Rails kit (`spec/requests`, `spec/mailers`, and the system
//! spec as a request test), plus the extras listed in each file's header.

mod accounts;
#[cfg(feature = "bench")]
mod bench;
mod budget;
mod email_verifications;
mod inertia_pages;
mod invitations;
mod live;
mod mailers;
mod members;
mod password_resets;
mod precognition;
mod protection;
mod sessions;
mod settings;
mod users;

use std::sync::Arc;

use axum::http::{header, HeaderValue};
use axum_test::{TestResponse, TestServer};
use inertia_rust_starter_kit::{
    app::App,
    controllers,
    inertia::{
        config::{Settings, SignUp},
        cookies, vite,
    },
    models::{
        sessions::{self as session_model, RequestDetails},
        tokens::{Clock, FixedClock, Purpose},
        users as user_model,
    },
};
use loco_rs::{app::AppContext, testing::prelude::*};
use serde_json::Value;

pub const PASSWORD: &str = "Secret1*3*5*";
pub const ONE: &str = "one@example.com";
pub const ONE_SESSION: &str = "11111111-1111-4111-8111-111111111111";
pub const TWO_SESSION: &str = "22222222-2222-4222-8222-222222222222";

/// Run `f` against a freshly seeded app with a cookie-saving client.
pub async fn with_app<F, Fut>(f: F)
where
    F: FnOnce(TestServer, AppContext) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let config = RequestConfigBuilder::new().save_cookies(true).build();
    request_with_config::<App, _, _>(config, |server, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        f(server, ctx).await;
    })
    .await;
}

/// [`with_app`] on a config changed by `configure` (e.g. a middleware turned on).
pub async fn with_app_config<C, F, Fut>(configure: C, f: F)
where
    C: FnOnce(&mut loco_rs::config::Config),
    F: FnOnce(TestServer, AppContext) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    use loco_rs::{app::Hooks, boot::StartMode, environment::Environment};
    let mut config = App::load_config(&Environment::Test).await.unwrap();
    configure(&mut config);
    let boot = App::boot(StartMode::ServerOnly, &Environment::Test, config)
        .await
        .unwrap();
    let ctx = boot.app_context.clone();
    seed::<App>(&ctx).await.unwrap();
    let server = TestServer::new_with_config(
        boot.router
            .unwrap()
            .into_make_service_with_connect_info::<std::net::SocketAddr>(),
        RequestConfigBuilder::new().save_cookies(true).build(),
    )
    .unwrap();
    f(server, ctx).await;
}

/// [`with_app`] with `settings.sign_up` set to `mode`, whatever config/test.yaml (or `SIGN_UP`)
/// says. Tests that depend on who can sign up pin it here, so an app can switch its default
/// without rewriting them.
pub async fn with_sign_up<F, Fut>(mode: SignUp, f: F)
where
    F: FnOnce(TestServer, AppContext) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mode = match mode {
        SignUp::Open => "open",
        SignUp::InvitationOnly => "invitation_only",
    };
    with_app_config(
        |config| {
            config.settings.as_mut().expect("a settings: block")["sign_up"] = mode.into();
        },
        f,
    )
    .await;
}

/// Every session and user row, for "nothing was written" assertions.
pub async fn db_snapshot(ctx: &AppContext) -> (Vec<session_model::Model>, Vec<user_model::Model>) {
    use sea_orm::{EntityTrait, QueryOrder};
    let sessions = session_model::Entity::find()
        .order_by_asc(session_model::Column::Id)
        .all(&ctx.db)
        .await
        .unwrap();
    let users = user_model::Entity::find()
        .order_by_asc(user_model::Column::Id)
        .all(&ctx.db)
        .await
        .unwrap();
    (sessions, users)
}

pub fn settings(ctx: &AppContext) -> Arc<Settings> {
    controllers::settings(ctx).unwrap()
}

pub async fn user(ctx: &AppContext, email: &str) -> user_model::Model {
    user_model::Model::find_by_email(&ctx.db, email)
        .await
        .unwrap()
}

/// `sign_in users(:one)` from the Rails helpers: create a session and set its signed cookie.
pub async fn sign_in(server: &mut TestServer, ctx: &AppContext, email: &str) -> String {
    let user = user(ctx, email).await;
    let session = session_model::Model::create_for_user(&ctx.db, &user, &RequestDetails::default())
        .await
        .unwrap();
    sign_in_with_token(server, ctx, &session.token);
    session.token
}

pub fn sign_in_with_token(server: &mut TestServer, ctx: &AppContext, token: &str) {
    let cookie = cookies::session_token_cookie(&settings(ctx), token);
    server.add_cookie(cookie);
}

pub fn token_for(ctx: &AppContext, user: &user_model::Model, purpose: Purpose) -> String {
    user.generate_token_for(
        purpose,
        settings(ctx).secret_key_base.as_bytes(),
        &*controllers::clock(ctx),
    )
}

/// `travel duration`: move the app's clock `by` from now.
pub fn travel(ctx: &AppContext, by: chrono::Duration) {
    let clock: Arc<dyn Clock> = Arc::new(FixedClock(chrono::Utc::now() + by));
    controllers::set_clock(ctx, clock);
}

/// `expect(response).to redirect_to(path)`.
#[track_caller]
pub fn assert_redirect(res: &TestResponse, path: &str) {
    assert!(
        res.status_code().is_redirection(),
        "expected a redirect to {path}, got {}: {}",
        res.status_code(),
        res.text()
    );
    assert_eq!(
        res.header(header::LOCATION),
        HeaderValue::from_str(path).unwrap()
    );
}

/// The current asset version, which every Inertia GET must echo in `X-Inertia-Version`.
pub fn asset_version(ctx: &AppContext) -> String {
    vite::shared(&settings(ctx).vite).version().to_owned()
}

/// Follow up with an Inertia GET of `path` and return the page object — how the Rails specs'
/// `flash[:alert]` and `session[:inertia_errors]` assertions are observed here.
pub async fn inertia_get(server: &TestServer, ctx: &AppContext, path: &str) -> Value {
    let res = server
        .get(path)
        .add_header("x-inertia", "true")
        .add_header("x-inertia-version", asset_version(ctx))
        .add_header("x-requested-with", "XMLHttpRequest")
        .await;
    assert_eq!(res.status_code(), 200, "GET {path}: {}", res.text());
    res.json::<Value>()
}

pub fn deliveries(ctx: &AppContext) -> Vec<String> {
    ctx.mailer.as_ref().unwrap().deliveries().messages
}

/// The `sid` from the (quoted-printable-decoded) link in a delivered mail.
pub fn sid_from_mail(message: &str) -> String {
    let body = decode_qp(message);
    let start = body.find("sid=").expect("mail has a sid link") + 4;
    body[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '%'))
        .collect::<String>()
        .replace("%2E", ".")
}

/// Undo what lettre does to a message: unfold the headers and decode their RFC 2047 encoded
/// words ([`decode_headers`]), then quoted-printable soft line breaks and `=XX` escapes (lettre
/// encodes long lines).
pub fn decode_qp(message: &str) -> String {
    let message = decode_headers(message);
    let joined = message.replace("=\r\n", "").replace("=\n", "");
    let bytes = joined.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&joined[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The headers (up to the first blank line) unfolded, a long header's continuation lines start
/// with whitespace, and their RFC 2047 encoded words decoded: lettre writes any non-ASCII header
/// (a subject with "·" or an accent) as `=?utf-8?b?<base64>?=`, split into several words when
/// long. The whitespace between two adjacent encoded words is not part of the text.
pub fn decode_headers(message: &str) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use regex::{Captures, Regex};

    let end = Regex::new(r"\r?\n\r?\n")
        .unwrap()
        .find(message)
        .map_or(message.len(), |m| m.start());
    let (headers, body) = message.split_at(end);
    let unfolded = Regex::new(r"\r?\n([ \t])")
        .unwrap()
        .replace_all(headers, "$1");
    let adjacent = Regex::new(r"\?=[ \t]+=\?")
        .unwrap()
        .replace_all(&unfolded, "?==?");
    let word = Regex::new(r"(?i)=\?utf-8\?b\?([A-Za-z0-9+/=]*)\?=").unwrap();
    let decoded = Regex::new(r"(?i)(?:=\?utf-8\?b\?[A-Za-z0-9+/=]*\?=)+")
        .unwrap()
        .replace_all(&adjacent, |run: &Captures| {
            let bytes: Vec<u8> = word
                .captures_iter(&run[0])
                .flat_map(|w| STANDARD.decode(&w[1]).expect("base64 in an encoded word"))
                .collect();
            String::from_utf8_lossy(&bytes).into_owned()
        });
    format!("{decoded}{body}")
}
