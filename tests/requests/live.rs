//! Live updates (src/live/): subscribe-time authorization, fan-out to the right subscribers,
//! echo suppression by tab, presence, `perform`, and the SSE transport (including that the
//! production request timeout doesn't cut it).

use std::time::Duration;

use futures_util::{Stream, StreamExt};
use inertia_rust_starter_kit::{
    auth::CurrentSession,
    channels::account::AccountChannel,
    live::{self, presence},
    models::{memberships, sessions as session_model},
    route_table,
};
use serde_json::{json, Value};
use serial_test::serial;

use super::*;

const TWO: &str = "two@example.com";

/// The next frame within `wait`, if any.
async fn next(stream: &mut (impl Stream<Item = Value> + Unpin), wait: Duration) -> Option<Value> {
    tokio::time::timeout(wait, stream.next())
        .await
        .ok()
        .flatten()
}

/// The next frame that isn't presence.
async fn next_message(stream: &mut (impl Stream<Item = Value> + Unpin)) -> Option<Value> {
    loop {
        let frame = next(stream, Duration::from_millis(500)).await?;
        if frame["type"] != "presence" {
            return Some(frame);
        }
    }
}

async fn session_of(ctx: &AppContext, email: &str) -> CurrentSession {
    let user = user(ctx, email).await;
    let session = session_model::Model::create_for_user(
        &ctx.db,
        &user,
        &session_model::RequestDetails::default(),
    )
    .await
    .unwrap();
    CurrentSession { session, user }
}

fn account(slug: &str) -> String {
    live::identifier("AccountChannel", &json!({ "account": slug }))
}

#[tokio::test]
#[serial]
async fn a_rejected_subscription_receives_nothing() {
    with_app(|_server, ctx| async move {
        // one@ is in Acme only.
        let one = session_of(&ctx, ONE).await;
        let mut stream = live::subscribe(
            &ctx,
            &one,
            vec![
                account("globex"),
                account("nope"),
                r#"{"channel":"NoSuchChannel"}"#.to_owned(),
                "not json".to_owned(),
            ],
        )
        .await;
        for _ in 0..4 {
            let frame = next(&mut stream, Duration::from_secs(1)).await.unwrap();
            assert_eq!(frame["type"], "reject_subscription", "{frame}");
        }
        // Globex's broadcasts don't reach a stream that was refused Globex.
        AccountChannel::broadcast_to(2, json!({ "type": "members" }));
        assert_eq!(next(&mut stream, Duration::from_millis(300)).await, None);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_broadcast_reaches_only_that_keys_subscribers() {
    with_app(|_server, ctx| async move {
        let one = session_of(&ctx, ONE).await;
        let two = session_of(&ctx, TWO).await;
        let mut one_acme = live::subscribe(&ctx, &one, vec![account("acme")]).await;
        let mut two_both =
            live::subscribe(&ctx, &two, vec![account("acme"), account("globex")]).await;
        assert_eq!(
            next(&mut one_acme, Duration::from_secs(1)).await.unwrap()["type"],
            "confirm_subscription"
        );
        // Each confirmation is followed by who is there (AccountChannel tracks presence).
        let mut kinds = Vec::new();
        for _ in 0..4 {
            let frame = next(&mut two_both, Duration::from_secs(1)).await.unwrap();
            kinds.push(frame["type"].as_str().unwrap().to_owned());
        }
        assert_eq!(
            kinds,
            [
                "confirm_subscription",
                "presence",
                "confirm_subscription",
                "presence"
            ]
        );

        AccountChannel::broadcast_to(2, json!({ "type": "hello globex" }));
        let frame = next_message(&mut two_both).await.unwrap();
        assert_eq!(frame["identifier"], account("globex"));
        assert_eq!(frame["message"], json!({ "type": "hello globex" }));
        assert_eq!(
            next_message(&mut one_acme).await,
            None,
            "one@ is not in Globex"
        );

        AccountChannel::broadcast_to(1, json!({ "type": "hello acme" }));
        assert_eq!(
            next_message(&mut one_acme).await.unwrap()["identifier"],
            account("acme")
        );
        assert_eq!(
            next_message(&mut two_both).await.unwrap()["identifier"],
            account("acme")
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_change_names_the_tab_that_made_it_so_that_tab_can_skip_it() {
    with_app(|mut server, ctx| async move {
        let one = session_of(&ctx, ONE).await;
        let mut stream = live::subscribe(&ctx, &one, vec![account("acme")]).await;
        next(&mut stream, Duration::from_secs(1)).await.unwrap();

        // two@ is a member of Acme; one@ (an owner) makes them an admin from tab "tab-1".
        sign_in(&mut server, &ctx, ONE).await;
        let res = server
            .patch("/acme/members/2")
            .add_header("x-tab-id", "tab-1")
            .json(&json!({ "role": "admin" }))
            .await;
        assert!(res.status_code().is_redirection(), "{}", res.text());
        let frame = next_message(&mut stream).await.expect("a members change");
        assert_eq!(frame["message"], json!({ "type": "members" }));
        assert_eq!(
            frame["tab_id"], "tab-1",
            "the tab that made it, so it skips the echo"
        );

        // Without a tab id (a script, another server), nobody's echo: `null`.
        server
            .patch("/acme/members/2")
            .json(&json!({ "role": "member" }))
            .await;
        assert_eq!(
            next_message(&mut stream).await.unwrap()["tab_id"],
            Value::Null
        );
        // A malformed tab id is ignored, not echoed back.
        server
            .patch("/acme/members/2")
            .add_header("x-tab-id", "<script>")
            .json(&json!({ "role": "admin" }))
            .await;
        assert_eq!(
            next_message(&mut stream).await.unwrap()["tab_id"],
            Value::Null
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_member_joining_through_an_invitation_is_broadcast() {
    use inertia_rust_starter_kit::models::invitations;
    with_app(|mut server, ctx| async move {
        let one = session_of(&ctx, ONE).await;
        let mut stream = live::subscribe(&ctx, &one, vec![account("acme")]).await;
        next(&mut stream, Duration::from_secs(1)).await.unwrap();
        // The seeds' pending invitation to three@ (token `three-token`): three@ signs up.
        let res = server
            .post(route_table::SIGN_UP)
            .json(&json!({
                "name": "Three",
                "email": "three@example.com",
                "password": PASSWORD,
                "password_confirmation": PASSWORD,
                "invitation": invitations::SEED_TOKEN,
            }))
            .await;
        assert_redirect(&res, "/acme");
        assert_eq!(
            next_message(&mut stream).await.unwrap()["message"],
            json!({ "type": "members" })
        );
        // Removing them is too.
        sign_in(&mut server, &ctx, ONE).await;
        let three = user(&ctx, "three@example.com").await;
        let membership = memberships::Model::find_for(&ctx.db, 1, three.id)
            .await
            .unwrap();
        server
            .delete(&format!("/acme/members/{}", membership.id))
            .await;
        assert_eq!(
            next_message(&mut stream).await.unwrap()["message"],
            json!({ "type": "members" })
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn presence_is_who_has_the_channel_open_for_members_only() {
    with_app(|mut server, ctx| async move {
        presence::reset();
        let one = session_of(&ctx, ONE).await;
        let mut stream = live::subscribe(&ctx, &one, vec![account("acme")]).await;
        next(&mut stream, Duration::from_secs(1)).await.unwrap(); // confirmed
        let now = next(&mut stream, Duration::from_secs(1)).await.unwrap();
        assert_eq!((now["type"].as_str(), &now["present"]), (Some("presence"), &json!([])));

        sign_in(&mut server, &ctx, TWO).await;
        let beat = |tab: &'static str, state: Value| {
            json!({ "identifier": account("acme"), "tab_id": tab, "state": state })
        };
        let res = server
            .post(route_table::LIVE_PRESENCE)
            .json(&beat("two-tab", json!({ "page": "members" })))
            .await;
        assert_eq!(res.status_code(), 200, "{}", res.text());
        let present = &res.json::<Value>()["present"];
        assert_eq!(present[0]["name"], "Another User");
        assert_eq!(present[0]["state"], json!({ "page": "members" }));
        let frame = next(&mut stream, Duration::from_secs(1)).await.unwrap();
        assert_eq!(frame["type"], "presence");
        assert_eq!(frame["identifier"], account("acme"));
        assert_eq!(frame["present"][0]["name"], "Another User");

        // The same heartbeat again: nothing new to say.
        server.post(route_table::LIVE_PRESENCE).json(&beat("two-tab", json!({ "page": "members" }))).await;
        assert_eq!(next(&mut stream, Duration::from_millis(300)).await, None);

        // Leaving.
        let res = server
            .delete(route_table::LIVE_PRESENCE)
            .json(&json!({ "identifier": account("acme"), "tab_id": "two-tab" }))
            .await;
        assert_eq!(res.status_code(), 204);
        assert_eq!(next(&mut stream, Duration::from_secs(1)).await.unwrap()["present"], json!([]));

        // A non-member can't announce themselves in Acme, or read who is there.
        server.clear_cookies();
        sign_in(&mut server, &ctx, ONE).await;
        let res = server
            .post(route_table::LIVE_PRESENCE)
            .json(&json!({ "identifier": account("globex"), "tab_id": "one-tab" }))
            .await;
        assert_eq!(res.status_code(), 404);
        assert_eq!(presence::present("AccountChannel", "2"), vec![]);
        // A bad tab id is a 400.
        let res = server
            .post(route_table::LIVE_PRESENCE)
            .json(&json!({ "identifier": account("acme"), "tab_id": "" }))
            .await;
        assert_eq!(res.status_code(), 400);
    })
    .await;
}

/// A stream that connects after someone arrived (or reconnects after a drop) learns who is there
/// right after its confirmation, instead of waiting for the next change.
#[tokio::test]
#[serial]
async fn a_new_subscription_hears_who_is_already_there() {
    with_app(|mut server, ctx| async move {
        presence::reset();
        sign_in(&mut server, &ctx, TWO).await;
        server
            .post(route_table::LIVE_PRESENCE)
            .json(&json!({ "identifier": account("acme"), "tab_id": "two-tab" }))
            .await;
        let one = session_of(&ctx, ONE).await;
        let mut stream = live::subscribe(&ctx, &one, vec![account("acme")]).await;
        assert_eq!(
            next(&mut stream, Duration::from_secs(1)).await.unwrap()["type"],
            "confirm_subscription"
        );
        let frame = next(&mut stream, Duration::from_secs(1)).await.unwrap();
        assert_eq!(frame["type"], "presence");
        assert_eq!(frame["present"][0]["name"], "Another User");
        presence::reset();
    })
    .await;
}

/// Authorization is not only at connect time: a member removed while their stream is open
/// is rejected at the next recheck and hears nothing of the account after it.
#[tokio::test]
#[serial]
async fn a_removed_member_is_rejected_on_the_open_stream() {
    with_app(|_server, ctx| async move {
        let two = session_of(&ctx, TWO).await;
        let mut stream = live::subscribe_rechecking(
            &ctx,
            &two,
            vec![account("acme"), account("globex")],
            Duration::from_millis(100),
        )
        .await;
        assert_eq!(
            next(&mut stream, Duration::from_secs(1)).await.unwrap()["type"],
            "confirm_subscription"
        );

        memberships::Model::find_for(&ctx.db, 1, two.user.id)
            .await
            .unwrap()
            .remove(&ctx.db)
            .await
            .unwrap();
        // Frames already queued (the confirmations, presence, the removal's own "members"
        // notice) may come first; then Acme's subscription is rejected.
        let rejected = loop {
            let frame = next(&mut stream, Duration::from_secs(2))
                .await
                .expect("a reject_subscription for Acme");
            if frame["type"] == "reject_subscription" {
                break frame;
            }
        };
        assert_eq!(rejected["identifier"], account("acme"));

        AccountChannel::broadcast_to(1, json!({ "type": "acme secret" }));
        AccountChannel::broadcast_to(2, json!({ "type": "hello globex" }));
        let frame = next_message(&mut stream).await.unwrap();
        assert_eq!(
            frame["identifier"],
            account("globex"),
            "Globex still streams"
        );
        assert_eq!(next_message(&mut stream).await, None, "nothing from Acme");
    })
    .await;
}

/// A stream outlives nothing that signs its browser out: once its session is deleted
/// (logged out from the sessions page, password changed), the stream ends.
#[tokio::test]
#[serial]
async fn a_stream_ends_when_its_session_is_deleted() {
    with_app(|_server, ctx| async move {
        let one = session_of(&ctx, ONE).await;
        let mut stream = live::subscribe_rechecking(
            &ctx,
            &one,
            vec![account("acme")],
            Duration::from_millis(100),
        )
        .await;
        assert_eq!(
            next(&mut stream, Duration::from_secs(1)).await.unwrap()["type"],
            "confirm_subscription"
        );
        session_model::Model::destroy_for_user(&ctx.db, one.user.id, &one.session.token)
            .await
            .unwrap();
        let ended = tokio::time::timeout(Duration::from_secs(2), async {
            while stream.next().await.is_some() {}
        })
        .await;
        assert!(
            ended.is_ok(),
            "the stream is still open after its session was deleted"
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn perform_needs_an_accepted_subscription_and_a_known_action() {
    with_app(|mut server, ctx| async move {
        let res = server
            .post(route_table::LIVE_PERFORM)
            .json(&json!({ "identifier": account("acme"), "action": "x" }))
            .await;
        assert_redirect(&res, route_table::SIGN_IN);
        sign_in(&mut server, &ctx, ONE).await;
        // AccountChannel has no actions: an accepted subscription still gets a 404 for one.
        let res = server
            .post(route_table::LIVE_PERFORM)
            .json(&json!({ "identifier": account("acme"), "action": "x" }))
            .await;
        assert_eq!(res.status_code(), 404);
        let res = server
            .post(route_table::LIVE_PERFORM)
            .json(&json!({ "identifier": account("globex"), "action": "x" }))
            .await;
        assert_eq!(res.status_code(), 404);
        let res = server
            .post(route_table::LIVE_PERFORM)
            .add_header("precognition", "true")
            .json(&json!({ "identifier": account("acme"), "action": "x" }))
            .await;
        assert_eq!(res.status_code(), 400);
    })
    .await;
}

/// The SSE route: guests go to sign in, the stream is `text/event-stream` with `retry`, and it
/// carries confirmations and broadcasts. Wrapped in Loco's `timeout_request` layer (on in
/// production, 30 s; 300 ms here), it keeps delivering past the timeout: the layer bounds the
/// time to the response headers, not the body. A handler that is slow to answer is still cut
/// (the 408 below), so the test would notice if the layer stopped applying.
#[tokio::test]
#[serial]
async fn the_stream_is_server_sent_events_that_outlive_the_request_timeout() {
    use axum::{body::Body, http::Request};
    use inertia_rust_starter_kit::inertia::cookies;
    use loco_rs::{
        app::Hooks,
        boot::StartMode,
        controller::middleware::{timeout::TimeOut, MiddlewareLayer},
        environment::Environment,
    };
    use tower::ServiceExt;

    let config = App::load_config(&Environment::Test).await.unwrap();
    let boot = App::boot(StartMode::ServerOnly, &Environment::Test, config)
        .await
        .unwrap();
    let ctx = boot.app_context.clone();
    seed::<App>(&ctx).await.unwrap();
    let timeout: TimeOut =
        serde_json::from_value(json!({ "enable": true, "timeout": 300 })).unwrap();
    let slow = timeout
        .apply(axum::Router::new().route(
            "/slow",
            axum::routing::get(|| async {
                tokio::time::sleep(Duration::from_millis(600)).await;
                "late"
            }),
        ))
        .unwrap()
        .with_state(ctx.clone());
    let res = slow
        .oneshot(Request::get("/slow").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), 408, "the timeout layer is live");
    // The same layer around the whole app, as production applies it.
    let router = timeout
        .apply(axum::Router::new().fallback_service(boot.router.unwrap()))
        .unwrap()
        .with_state(ctx.clone());

    let query =
        serde_urlencoded::to_string([("s", serde_json::to_string(&[account("acme")]).unwrap())])
            .unwrap();
    let url = format!("{}?{query}", route_table::LIVE);
    let get = |token: Option<&str>| {
        let mut req = Request::get(&url);
        if let Some(token) = token {
            let cookie = cookies::session_token_cookie(&settings(&ctx), token);
            req = req.header("cookie", format!("{}={}", cookie.name(), cookie.value()));
        }
        router.clone().oneshot(req.body(Body::empty()).unwrap())
    };

    let res = get(None).await.unwrap();
    assert_eq!(res.headers()["location"], route_table::SIGN_IN);
    let res = get(Some(ONE_SESSION)).await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["content-type"], "text/event-stream");

    let mut body = res.into_body().into_data_stream();
    let mut read = String::new();
    while !read.contains("confirm_subscription") {
        let chunk = tokio::time::timeout(Duration::from_secs(2), body.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        read.push_str(&String::from_utf8_lossy(&chunk));
    }
    assert!(read.contains("retry: 1000"), "{read}");
    // Past the 300 ms timeout, the stream is still open and still delivering.
    tokio::time::sleep(Duration::from_millis(600)).await;
    AccountChannel::broadcast_to(1, json!({ "type": "members" }));
    let mut after = String::new();
    while !after.contains(r#""message":{"type":"members"}"#) {
        let chunk = tokio::time::timeout(Duration::from_secs(2), body.next())
            .await
            .expect("a frame after the timeout")
            .unwrap()
            .unwrap();
        after.push_str(&String::from_utf8_lossy(&chunk));
    }
    assert!(after.contains("data: {"), "{after}");
}
