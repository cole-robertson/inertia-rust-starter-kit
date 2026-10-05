//! Live updates: the kit's Action Cable.
//!
//! | Action Cable | here |
//! |---|---|
//! | `class ProjectsChannel < ApplicationCable::Channel` | a type implementing [`Channel`] in `src/channels/` (`cargo loco generate channel projects`) |
//! | `def subscribed; reject unless …; stream_for project; end` | [`Channel::subscribed`] returning [`Subscription::stream_for`] (or `Err`: rejected) |
//! | `ProjectsChannel.broadcast_to(project, data)` | [`broadcast_to`]`(ProjectsChannel::NAME, project.id, data)`, or the generated `ProjectsChannel::broadcast_to` |
//! | `def perform_action(data)` (client `perform`) | [`Channel::perform`], over `POST /live/perform` |
//! | the `/cable` connection, cookie-authenticated | one SSE stream per tab, `GET /live?s=[identifiers]`, signed-in users only |
//! | the async adapter (one process) | an in-process [`tokio::sync::broadcast`] hub |
//!
//! **Transport.** Server-Sent Events: one connection per browser tab carrying every
//! subscription of that tab (their identifiers are in the query; the client reconnects with
//! the new set when it changes). Each subscription is confirmed or rejected first, the way
//! Action Cable's are; then the stream carries `{identifier, message, tab_id}` frames. `retry:
//! 1000` brings a dropped connection back within a second, and a comment every 15 s keeps
//! proxies from closing an idle one. Loco's `timeout_request` only bounds the time to the
//! response headers, so it never cuts the stream (`tests/requests/live.rs` checks it).
//!
//! **Client to server** (`perform`) is a plain `POST /live/perform`, not a WebSocket: it rides
//! the same session cookie, CSRF check and proxies as every other write, and the subscription is
//! authorized again by [`Channel::subscribed`] before [`Channel::perform`] runs.
//!
//! **Revocation.** An open stream authorizes again every [`RECHECK_EVERY`]: it ends when its
//! session is gone and sends `reject_subscription` for a subscription its channel no longer
//! accepts (Action Cable's `remote_connections.where(…).disconnect`, done by polling).
//!
//! **Echo suppression.** Every request carries the browser tab's `X-Tab-Id` (frontend/lib/live.ts
//! sets it); [`layer`] keeps it in a task-local for the request, and [`broadcast_to`] stamps it
//! on what it sends, so the tab that caused a change can skip it (Rails' `Current.tab_id`).
//!
//! **Presence** ([`presence`]): a channel that returns `true` from [`Channel::tracks_presence`]
//! accepts heartbeats on `POST /live/presence` (every 15 s, expiring after 30 s) and sends
//! `{identifier, type: "presence", present}` to its subscribers when who is there changes.
//!
//! The hub is a `static` rather than a `shared_store` entry because models broadcast too (after
//! their transaction commits) and models don't hold the `AppContext`. It lives in one process:
//! running several app servers needs a shared backend (see the live-updates recipe).

pub mod presence;

use std::{
    collections::HashMap,
    convert::Infallible,
    fmt::Display,
    future::Future,
    sync::{Arc, LazyLock},
    time::Duration,
};

use axum::{
    extract::{Query, Request},
    http::StatusCode,
    middleware::Next,
    response::sse::{Event, KeepAlive, Sse},
    Json, Router,
};
use futures_util::{
    future::BoxFuture,
    stream::{self, BoxStream},
    StreamExt,
};
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::{
    auth::{Authenticated, CurrentSession},
    controllers::{NoPrecognition, Params},
    models::sessions,
    route_table,
};

/// The most subscriptions one stream may carry.
pub const MAX_SUBSCRIPTIONS: usize = 32;
/// The longest identifier (`{"channel":…,…params}` as JSON) accepted.
pub const MAX_IDENTIFIER: usize = 512;

/// What a channel sends: a message, or who is present.
#[derive(Debug, Clone)]
pub enum Body {
    Message(Arc<Value>),
    Presence(Arc<Vec<presence::Present>>),
}

/// One broadcast on the hub, to everyone subscribed to `(channel, key)`.
#[derive(Debug, Clone)]
pub struct Broadcast {
    pub channel: String,
    pub key: String,
    pub body: Body,
    /// The browser tab whose request sent it (`X-Tab-Id`), so that tab can skip its echo.
    pub tab_id: Option<String>,
}

static HUB: LazyLock<broadcast::Sender<Broadcast>> = LazyLock::new(|| broadcast::channel(1024).0);

tokio::task_local! {
    static TAB_ID: Option<String>;
}

/// `ChannelName.broadcast_to(key, payload)`: every subscriber of `channel` streaming `key`
/// receives `payload` (each through its subscription's filter, if any). Call it after the write
/// has committed. Nobody listening is fine.
pub fn broadcast_to(channel: &str, key: impl Display, payload: impl Serialize) {
    match serde_json::to_value(payload) {
        Ok(payload) => send(channel, key.to_string(), Body::Message(Arc::new(payload))),
        Err(err) => tracing::error!(%err, channel, "live: payload is not serializable"),
    }
}

fn send(channel: &str, key: String, body: Body) {
    let _ = HUB.send(Broadcast {
        channel: channel.to_owned(),
        key,
        body,
        tab_id: current_tab(),
    });
}

/// The `X-Tab-Id` of the request being handled (inside [`layer`]), if any.
#[must_use]
pub fn current_tab() -> Option<String> {
    TAB_ID.try_with(Clone::clone).ok().flatten()
}

fn valid_tab(tab: &str) -> bool {
    !tab.is_empty()
        && tab.len() <= 64
        && tab
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Keep each request's `X-Tab-Id` for [`broadcast_to`] (a task-local for the request's future).
pub fn layer(router: Router) -> Router {
    router.layer(axum::middleware::from_fn(
        |req: Request, next: Next| async move {
            let tab = req
                .headers()
                .get("x-tab-id")
                .and_then(|v| v.to_str().ok())
                .filter(|t| valid_tab(t))
                .map(str::to_owned);
            TAB_ID.scope(tab, next.run(req)).await
        },
    ))
}

/// Run `f` as if the request came from browser tab `tab` (tests, and work spawned off a
/// request that should keep its tab).
pub async fn with_tab<F: Future>(tab: Option<String>, f: F) -> F::Output {
    TAB_ID.scope(tab, f).await
}

type Filter = Arc<dyn Fn(Arc<Value>) -> BoxFuture<'static, bool> + Send + Sync>;

/// What an accepted subscription follows: one or more stream keys of its channel (Action
/// Cable's `stream_for`), and optionally a per-message filter.
#[derive(Clone)]
pub struct Subscription {
    keys: Vec<String>,
    filter: Option<Filter>,
}

impl std::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscription")
            .field("keys", &self.keys)
            .field("filter", &self.filter.is_some())
            .finish()
    }
}

impl Subscription {
    /// `stream_for key`: the messages broadcast to `key` on this channel.
    #[must_use]
    pub fn stream_for(key: impl Display) -> Self {
        Self {
            keys: vec![key.to_string()],
            filter: None,
        }
    }

    /// Another key to follow as well (a second `stream_for`), e.g. a per-user stream next to
    /// the account's.
    #[must_use]
    pub fn and(mut self, key: impl Display) -> Self {
        self.keys.push(key.to_string());
        self
    }

    /// Deliver a message only when `keep(payload)` says so, checked per message (Action
    /// Cable's `stream_for … do |message| … end`). Use it for rules that change while the
    /// stream is open, like "can this member still see this project". Presence is not filtered.
    #[must_use]
    pub fn filter<F, Fut>(mut self, keep: F) -> Self
    where
        F: Fn(Arc<Value>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        self.filter = Some(Arc::new(move |payload| Box::pin(keep(payload))));
        self
    }

    /// The stream keys, in the order given. The first is the presence room.
    #[must_use]
    pub fn keys(&self) -> &[String] {
        &self.keys
    }
}

/// A channel: who may subscribe, and what they follow. Register it in `src/channels/mod.rs`
/// (`cargo loco generate channel <name>` does both).
#[async_trait]
pub trait Channel: Send + Sync + 'static {
    /// The name clients subscribe with, e.g. `"ProjectsChannel"`.
    fn name(&self) -> &'static str;

    /// Action Cable's `subscribed`: authorize `user` for `params` (the identifier's fields,
    /// `channel` included) and say what to stream, or return an error to reject. A rejected
    /// subscription receives nothing.
    ///
    /// # Errors
    /// Any error rejects the subscription ([`reject`] is the usual one).
    async fn subscribed(
        &self,
        ctx: &AppContext,
        user: &CurrentSession,
        params: &Value,
    ) -> Result<Subscription>;

    /// Whether this channel accepts presence heartbeats (see [`presence`]).
    fn tracks_presence(&self) -> bool {
        false
    }

    /// A client's `subscription.perform(action, data)`, after [`Self::subscribed`] has accepted
    /// the same identifier for this user. Unknown actions are a 404.
    ///
    /// # Errors
    /// Whatever the action fails with.
    async fn perform(
        &self,
        _ctx: &AppContext,
        _user: &CurrentSession,
        _subscription: &Subscription,
        _action: &str,
        _data: &Value,
    ) -> Result<()> {
        Err(Error::NotFound)
    }
}

/// Reject a subscription (Action Cable's `reject`).
///
/// # Errors
/// Always.
pub fn reject<T>() -> Result<T> {
    Err(Error::NotFound)
}

static CHANNELS: LazyLock<HashMap<&'static str, Arc<dyn Channel>>> = LazyLock::new(|| {
    crate::channels::all()
        .into_iter()
        .map(|c| (c.name(), c))
        .collect()
});

/// The registered channel named `name`.
#[must_use]
pub fn channel(name: &str) -> Option<Arc<dyn Channel>> {
    CHANNELS.get(name).cloned()
}

/// The identifier a client subscribes with: `{"channel": name, …params}` as JSON with sorted
/// keys (what `frontend/lib/live.ts` sends).
#[must_use]
pub fn identifier(channel: &str, params: &Value) -> String {
    let mut fields: std::collections::BTreeMap<String, Value> = params
        .as_object()
        .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    fields.insert("channel".to_owned(), Value::String(channel.to_owned()));
    serde_json::to_string(&fields).unwrap_or_default()
}

/// The channel and subscription `identifier` gives `user`, or `None` when it is malformed,
/// names no channel, or the channel rejects it.
pub async fn authorize(
    ctx: &AppContext,
    user: &CurrentSession,
    identifier: &str,
) -> Option<(Arc<dyn Channel>, Subscription)> {
    if identifier.len() > MAX_IDENTIFIER {
        return None;
    }
    let params: Value = serde_json::from_str(identifier).ok()?;
    let channel = channel(params.get("channel")?.as_str()?)?;
    match channel.subscribed(ctx, user, &params).await {
        Ok(subscription) => Some((channel, subscription)),
        Err(Error::NotFound | Error::Unauthorized(_)) => None,
        Err(err) => {
            tracing::error!(%err, identifier, "live: subscribe failed");
            None
        }
    }
}

/// How often an open stream checks again that its session still exists and that each channel
/// still accepts its subscription. Authorization otherwise happens only when the stream opens,
/// and a stream stays open for as long as the tab does.
pub const RECHECK_EVERY: Duration = Duration::from_secs(15);

/// What a stream sends: `{type: "confirm_subscription"|"reject_subscription", identifier}`
/// first (for a presence channel, followed by who is there now), then `{identifier, message,
/// tab_id}` and `{identifier, type: "presence", present, tab_id}` frames as they are broadcast. The stream itself, for tests and other transports;
/// `GET /live` sends it as Server-Sent Events.
///
/// Every [`RECHECK_EVERY`] it authorizes again: a subscription the channel now rejects (a
/// removed member) gets `reject_subscription` and nothing after it, and the stream ends once
/// its session is gone (signed out, password changed, account deleted).
pub async fn subscribe(
    ctx: &AppContext,
    user: &CurrentSession,
    identifiers: Vec<String>,
) -> BoxStream<'static, Value> {
    subscribe_rechecking(ctx, user, identifiers, RECHECK_EVERY).await
}

/// [`subscribe`], authorizing again every `every` (tests use a short one).
pub async fn subscribe_rechecking(
    ctx: &AppContext,
    user: &CurrentSession,
    identifiers: Vec<String>,
    every: Duration,
) -> BoxStream<'static, Value> {
    // Listen before authorizing: nothing broadcast after a confirmation is missed.
    let rx = HUB.subscribe();
    let mut opening = Vec::new();
    let mut active: Vec<(String, &'static str, Subscription)> = Vec::new();
    for identifier in identifiers {
        match authorize(ctx, user, &identifier).await {
            Some((channel, subscription)) => {
                opening.push(json!({ "type": "confirm_subscription", "identifier": identifier }));
                // Who is there now: a change announced before this stream connected (or while
                // it reconnected) would otherwise be missed until the next one.
                if channel.tracks_presence() {
                    if let Some(key) = subscription.keys().first() {
                        opening.push(json!({
                            "identifier": identifier,
                            "type": "presence",
                            "present": presence::present(channel.name(), key),
                            "tab_id": null,
                        }));
                    }
                }
                active.push((identifier, channel.name(), subscription));
            }
            None => opening.push(json!({
                "type": "reject_subscription",
                "identifier": identifier,
            })),
        }
    }
    let tick = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
    let open = Open {
        rx,
        active,
        ctx: ctx.clone(),
        user: user.clone(),
        tick,
    };
    let frames = stream::unfold(Some(open), |open| async move {
        let mut open = open?;
        loop {
            tokio::select! {
                message = open.rx.recv() => match message {
                    Ok(broadcast) => {
                        let out = frames_for(&open.active, &broadcast).await;
                        if !out.is_empty() {
                            return Some((out, Some(open)));
                        }
                    }
                    // A subscriber that fell 1,024 messages behind skips them.
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return None,
                },
                _ = open.tick.tick() => match open.recheck().await {
                    None => return None,
                    Some(rejected) if !rejected.is_empty() => return Some((rejected, Some(open))),
                    Some(_) => {}
                },
            }
        }
    })
    .flat_map(stream::iter);
    stream::iter(opening).chain(frames).boxed()
}

/// An open stream's state between frames.
struct Open {
    rx: broadcast::Receiver<Broadcast>,
    active: Vec<(String, &'static str, Subscription)>,
    ctx: AppContext,
    user: CurrentSession,
    tick: tokio::time::Interval,
}

impl Open {
    /// Authorize again: `None` when the session is gone (the stream ends), else a
    /// `reject_subscription` frame for each subscription that is no longer accepted.
    async fn recheck(&mut self) -> Option<Vec<Value>> {
        match sessions::Model::find_by_token_with_user(&self.ctx.db, &self.user.session.token).await
        {
            Ok((session, user)) => self.user = CurrentSession { session, user },
            Err(ModelError::EntityNotFound) => return None,
            // Keep the stream through a passing database error; the next check decides.
            Err(err) => {
                tracing::error!(%err, "live: could not recheck the session");
                return Some(Vec::new());
            }
        }
        let mut rejected = Vec::new();
        let mut still = Vec::with_capacity(self.active.len());
        for (identifier, name, _) in std::mem::take(&mut self.active) {
            match authorize(&self.ctx, &self.user, &identifier).await {
                Some((_, now)) => still.push((identifier, name, now)),
                None => rejected.push(json!({
                    "type": "reject_subscription",
                    "identifier": identifier,
                })),
            }
        }
        self.active = still;
        Some(rejected)
    }
}

async fn frames_for(
    active: &[(String, &'static str, Subscription)],
    broadcast: &Broadcast,
) -> Vec<Value> {
    let mut out = Vec::new();
    for (identifier, channel, subscription) in active {
        if *channel != broadcast.channel || !subscription.keys.contains(&broadcast.key) {
            continue;
        }
        match &broadcast.body {
            Body::Message(payload) => {
                if let Some(keep) = &subscription.filter {
                    if !keep(payload.clone()).await {
                        continue;
                    }
                }
                out.push(json!({
                    "identifier": identifier,
                    "message": payload.as_ref(),
                    "tab_id": broadcast.tab_id,
                }));
            }
            Body::Presence(present) => out.push(json!({
                "identifier": identifier,
                "type": "presence",
                "present": present.as_ref(),
                "tab_id": broadcast.tab_id,
            })),
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct StreamQuery {
    /// The identifiers, as a JSON array of strings.
    #[serde(default)]
    s: String,
}

/// `GET /live?s=["{\"channel\":…}", …]`: the tab's subscriptions as Server-Sent Events.
async fn stream(
    Authenticated(user): Authenticated,
    State(ctx): State<AppContext>,
    Query(query): Query<StreamQuery>,
) -> Result<Response> {
    let identifiers: Vec<String> = serde_json::from_str(&query.s)
        .map_err(|_| Error::BadRequest("s: a JSON array of identifiers".into()))?;
    if identifiers.len() > MAX_SUBSCRIPTIONS {
        return Err(Error::BadRequest("too many subscriptions".into()));
    }
    let hello = stream::once(async {
        Event::default()
            .retry(Duration::from_secs(1))
            .comment("live")
    });
    let frames = subscribe(&ctx, &user, identifiers).await.map(|frame| {
        Event::default()
            .json_data(frame)
            .unwrap_or_else(|_| Event::default().comment("unserializable"))
    });
    Ok(Sse::new(hello.chain(frames).map(Ok::<_, Infallible>))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response())
}

#[derive(Debug, Default, Deserialize)]
struct PerformParams {
    #[serde(default)]
    identifier: String,
    #[serde(default)]
    action: String,
    #[serde(default)]
    data: Value,
}

/// `POST /live/perform` `{identifier, action, data}`: 204, or 404 when the subscription would
/// be rejected or the channel has no such action.
async fn perform(
    _: NoPrecognition,
    Authenticated(user): Authenticated,
    State(ctx): State<AppContext>,
    Params(params): Params<PerformParams>,
) -> Result<Response> {
    let (channel, subscription) = authorize(&ctx, &user, &params.identifier)
        .await
        .ok_or(Error::NotFound)?;
    channel
        .perform(&ctx, &user, &subscription, &params.action, &params.data)
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Debug, Default, Deserialize)]
struct PresenceParams {
    #[serde(default)]
    identifier: String,
    #[serde(default)]
    tab_id: String,
    /// What this tab is doing, shown to the others (`{"editing": 7}`); at most 1 KB.
    #[serde(default)]
    state: Value,
}

async fn presence_room(
    ctx: &AppContext,
    user: &CurrentSession,
    params: &PresenceParams,
) -> Result<(&'static str, String)> {
    if !valid_tab(&params.tab_id) {
        return Err(Error::BadRequest("tab_id".into()));
    }
    let (channel, subscription) = authorize(ctx, user, &params.identifier)
        .await
        .ok_or(Error::NotFound)?;
    if !channel.tracks_presence() {
        return Err(Error::NotFound);
    }
    let key = subscription
        .keys()
        .first()
        .cloned()
        .ok_or(Error::NotFound)?;
    Ok((channel.name(), key))
}

/// `POST /live/presence` `{identifier, tab_id, state}`: a heartbeat; answers who is present.
async fn heartbeat(
    _: NoPrecognition,
    Authenticated(user): Authenticated,
    State(ctx): State<AppContext>,
    Params(params): Params<PresenceParams>,
) -> Result<Response> {
    let (channel, key) = presence_room(&ctx, &user, &params).await?;
    let state = (!params.state.is_null()).then_some(params.state);
    if state
        .as_ref()
        .is_some_and(|s| s.to_string().len() > presence::MAX_STATE)
    {
        return Err(Error::BadRequest("state is too large".into()));
    }
    let present = presence::beat(channel, &key, &params.tab_id, &user.user, state);
    Ok(Json(json!({ "present": present })).into_response())
}

/// `DELETE /live/presence` `{identifier, tab_id}`: the tab left. 204.
async fn depart(
    _: NoPrecognition,
    Authenticated(user): Authenticated,
    State(ctx): State<AppContext>,
    Params(params): Params<PresenceParams>,
) -> Result<Response> {
    let (channel, key) = presence_room(&ctx, &user, &params).await?;
    presence::leave(channel, &key, &params.tab_id, user.user.id);
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub fn routes() -> Routes {
    Routes::new()
        .add(route_table::LIVE, get(stream))
        .add(route_table::LIVE_PERFORM, post(perform))
        .add(route_table::LIVE_PRESENCE, post(heartbeat).delete(depart))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_have_sorted_keys_like_the_client() {
        assert_eq!(
            identifier("AccountChannel", &json!({ "account": "acme", "a": 1 })),
            r#"{"a":1,"account":"acme","channel":"AccountChannel"}"#
        );
        assert_eq!(identifier("X", &Value::Null), r#"{"channel":"X"}"#);
    }

    #[test]
    fn only_short_url_safe_tab_ids_are_kept() {
        assert!(valid_tab("3f1c-9a_b"));
        for bad in ["", "a b", "<script>", &"x".repeat(65)] {
            assert!(!valid_tab(bad), "{bad}");
        }
    }

    #[tokio::test]
    async fn a_broadcast_carries_the_tab_of_the_request_that_sent_it() {
        let mut rx = HUB.subscribe();
        with_tab(Some("tab-1".into()), async {
            broadcast_to("UnitChannel", 7, json!({ "n": 1 }));
        })
        .await;
        broadcast_to("UnitChannel", 7, json!({ "n": 2 }));
        let mut seen = Vec::new();
        while seen.len() < 2 {
            let b = rx.recv().await.unwrap();
            if b.channel == "UnitChannel" {
                seen.push(b.tab_id);
            }
        }
        assert_eq!(seen, [Some("tab-1".to_owned()), None]);
    }
}
