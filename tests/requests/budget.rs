//! Inertia budgets: test helpers that pin down what a page sends and costs, so a change that
//! quietly makes it heavier fails a test (`.claude/skills/starter-kit/recipes/inertia-page.md`,
//! "Budget tests").
//!
//! - [`partial`]: an Inertia partial reload asking for `only` props.
//! - [`assert_props_exactly`]: the page's props are exactly these (plus the shared ones).
//! - [`assert_deferred`]: a deferred prop is absent from the first load and listed in
//!   `deferredProps`.
//! - [`assert_optional_absent`]: an optional prop is absent unless asked for.
//! - [`assert_payload_under`]: the response body is under `bytes`.
//! - [`assert_max_queries`]: a request runs at most `n` SQL queries (the `Server-Timing: db`
//!   count, `src/db.rs`).

use axum_test::{TestResponse, TestServer};
use serde_json::Value;
use serial_test::serial;

use super::*;

/// The props every signed-in page carries (`auth::register_shared_props`), which
/// [`assert_props_exactly`] ignores.
pub const SHARED: &[&str] = &["auth", "accounts", "errors"];

/// An Inertia partial reload of `component` at `path` asking for `only` (Inertia's
/// `router.reload({ only })`).
pub async fn partial(
    server: &TestServer,
    ctx: &AppContext,
    path: &str,
    component: &str,
    only: &[&str],
) -> TestResponse {
    let res = server
        .get(path)
        .add_header("x-inertia", "true")
        .add_header("x-inertia-version", asset_version(ctx))
        .add_header("x-requested-with", "XMLHttpRequest")
        .add_header("x-inertia-partial-component", component.to_owned())
        .add_header("x-inertia-partial-data", only.join(","))
        .await;
    assert_eq!(res.status_code(), 200, "GET {path}: {}", res.text());
    res
}

/// An Inertia visit (not partial) to `path`, as the response.
pub async fn visit(server: &TestServer, ctx: &AppContext, path: &str) -> TestResponse {
    let res = server
        .get(path)
        .add_header("x-inertia", "true")
        .add_header("x-inertia-version", asset_version(ctx))
        .add_header("x-requested-with", "XMLHttpRequest")
        .await;
    assert_eq!(res.status_code(), 200, "GET {path}: {}", res.text());
    res
}

fn page(res: &TestResponse) -> Value {
    res.json::<Value>()
}

fn prop_keys(page: &Value) -> Vec<String> {
    let mut keys: Vec<String> = page["props"]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    keys
}

/// The page's own props are exactly `expected` (any order): the shared props ([`SHARED`]) aside,
/// nothing more is sent, and nothing is missing. On a partial reload, `expected` is the `only`
/// list the client asked for.
#[track_caller]
pub fn assert_props_exactly(res: &TestResponse, expected: &[&str]) {
    let page = page(res);
    let mut sent: Vec<String> = prop_keys(&page)
        .into_iter()
        .filter(|k| !SHARED.contains(&k.as_str()))
        .collect();
    sent.sort();
    let mut expected: Vec<String> = expected.iter().map(|s| (*s).to_owned()).collect();
    expected.sort();
    assert_eq!(
        sent, expected,
        "the props {} sent (shared props {SHARED:?} aside)",
        page["component"]
    );
}

/// `prop` is deferred: absent from this (first) load, and listed in `deferredProps` for the
/// client to fetch after the page renders.
#[track_caller]
pub fn assert_deferred(res: &TestResponse, prop: &str) {
    let page = page(res);
    assert!(
        page["props"].get(prop).is_none(),
        "{prop} is in the first load of {}: it should be deferred",
        page["component"]
    );
    let listed = page["deferredProps"].as_object().is_some_and(|groups| {
        groups
            .values()
            .any(|g| g.as_array().is_some_and(|g| g.iter().any(|p| p == prop)))
    });
    assert!(
        listed,
        "{prop} is not in deferredProps: {}",
        page["deferredProps"]
    );
}

/// `prop` is optional: not sent unless a partial reload asks for it.
#[track_caller]
pub fn assert_optional_absent(res: &TestResponse, prop: &str) {
    let page = page(res);
    assert!(
        page["props"].get(prop).is_none(),
        "{prop} was sent to {} without being asked for",
        page["component"]
    );
    let deferred = page["deferredProps"].to_string();
    assert!(
        !deferred.contains(&format!("\"{prop}\"")),
        "{prop} is deferred, not optional: {deferred}"
    );
}

/// The response body is at most `bytes` long.
#[track_caller]
pub fn assert_payload_under(res: &TestResponse, bytes: usize) {
    let size = res.as_bytes().len();
    assert!(size <= bytes, "{size} bytes, over the budget of {bytes}");
}

/// `N` from the response's `Server-Timing: db;desc="N queries"` (src/inertia/timing.rs), the SQL
/// queries the request ran. Only GET/HEAD responses carry it.
#[track_caller]
pub fn queries(res: &TestResponse) -> u32 {
    let header = res
        .maybe_header("server-timing")
        .expect("a Server-Timing header (GET and HEAD only)");
    header
        .to_str()
        .unwrap()
        .split(", ")
        .find_map(|entry| entry.strip_prefix("db;desc=\""))
        .and_then(|rest| rest.split_once(' '))
        .and_then(|(n, _)| n.parse().ok())
        .expect("a db entry in Server-Timing")
}

/// Run `requests` (which returns the responses it got) and check that none of them ran more
/// than `n` SQL queries. Returns the most any ran, so a test can tighten the budget.
///
/// ```ignore
/// assert_max_queries(6, || async { vec![visit(&server, &ctx, "/acme/members").await] }).await;
/// ```
pub async fn assert_max_queries<F, Fut>(n: u32, requests: F) -> u32
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Vec<TestResponse>>,
{
    let responses = requests().await;
    assert!(
        !responses.is_empty(),
        "assert_max_queries: no responses to check"
    );
    let mut most = 0;
    for res in &responses {
        let count = queries(res);
        most = most.max(count);
        assert!(
            count <= n,
            "{count} SQL queries, over the budget of {n} ({})",
            res.json::<Value>()["url"]
        );
    }
    most
}

// The helpers on the kit's own pages: each page's props, its deferred and optional ones, and a
// query and payload budget. A page that starts sending more, or querying more, fails here.

#[tokio::test]
#[serial]
async fn the_account_overview_defers_its_member_count_and_stays_small() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        visit(&server, &ctx, "/acme").await; // remembers Acme as the last account (a write)
        let res = visit(&server, &ctx, "/acme").await;
        assert_props_exactly(&res, &["account", "membership"]);
        assert_deferred(&res, "members_count");
        assert_payload_under(&res, 2_000);
        // The deferred reload sends that prop alone.
        let res = partial(&server, &ctx, "/acme", "accounts/show", &["members_count"]).await;
        assert_props_exactly(&res, &["members_count"]);
        assert_eq!(res.json::<Value>()["props"]["members_count"], 2);
        assert_max_queries(5, || async {
            vec![
                visit(&server, &ctx, "/acme").await,
                partial(&server, &ctx, "/acme", "accounts/show", &["members_count"]).await,
            ]
        })
        .await;
    })
    .await;
}

#[tokio::test]
#[serial]
async fn the_members_page_reloads_only_its_lists() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = visit(&server, &ctx, "/acme/members").await;
        assert_props_exactly(&res, &["members", "invitations", "can_manage"]);
        assert_payload_under(&res, 3_000);
        // What `useLiveReload` asks for when a member joins.
        let res = partial(
            &server,
            &ctx,
            "/acme/members",
            "members/index",
            &["members", "invitations"],
        )
        .await;
        assert_props_exactly(&res, &["members", "invitations"]);
        let most = assert_max_queries(6, || async {
            vec![visit(&server, &ctx, "/acme/members").await]
        })
        .await;
        assert!(most >= 3, "session, membership, members: {most}");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn settings_and_sessions_pages_send_only_their_props() {
    use inertia_rust_starter_kit::route_table;
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = visit(&server, &ctx, route_table::SETTINGS_PROFILE).await;
        assert_props_exactly(&res, &[]);
        let res = visit(&server, &ctx, route_table::SETTINGS_SESSIONS).await;
        assert_props_exactly(&res, &["sessions"]);
        let res = visit(&server, &ctx, "/acme/settings").await;
        assert_props_exactly(&res, &["account"]);
        assert_max_queries(4, || async {
            vec![
                visit(&server, &ctx, route_table::SETTINGS_PROFILE).await,
                visit(&server, &ctx, route_table::SETTINGS_SESSIONS).await,
                visit(&server, &ctx, "/acme/settings").await,
            ]
        })
        .await;
    })
    .await;
}

/// The helpers are tests too: each fails on a page that breaks its budget.
#[tokio::test]
#[serial]
async fn the_helpers_fail_when_the_budget_is_broken() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = visit(&server, &ctx, "/acme").await;
        let size = res.as_bytes().len();
        let fails = |check: &dyn Fn(&TestResponse)| {
            let res = res.clone();
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(&res))).is_err()
        };
        assert!(
            fails(&|r| assert_props_exactly(r, &["account"])),
            "a missing prop"
        );
        assert!(fails(&|r| assert_props_exactly(
            r,
            &["account", "membership", "x"]
        )));
        assert!(
            fails(&|r| assert_deferred(r, "account")),
            "sent, so not deferred"
        );
        assert!(fails(&|r| assert_deferred(r, "nope")), "not listed");
        assert!(fails(&|r| assert_optional_absent(r, "membership")), "sent");
        assert!(
            fails(&|r| assert_optional_absent(r, "members_count")),
            "deferred"
        );
        assert!(fails(&|r| assert_payload_under(r, size - 1)));
        assert_payload_under(&res, size);
        // The same request again (the first visit also remembered Acme as the last account).
        let n = queries(&visit(&server, &ctx, "/acme").await);
        assert!(n > 0);
        let over = std::panic::AssertUnwindSafe(assert_max_queries(n - 1, || async {
            vec![visit(&server, &ctx, "/acme").await]
        }));
        assert!(futures_util::FutureExt::catch_unwind(over).await.is_err());
    })
    .await;
}
