# Recipe: live updates (channels, the kit's Action Cable)

**When:** a page should change without a reload when something happens elsewhere: another
person edits a record, a job finishes, someone joins the account. Rails: Action Cable channels
(`rails g channel`), `broadcast_to`, `subscribed`/`reject`, `perform`.

## The pieces

| Action Cable | This kit |
|---|---|
| `app/channels/projects_channel.rb` | `src/channels/projects.rs`: a type implementing `live::Channel`, listed in `src/channels/mod.rs` |
| `def subscribed; reject unless …; stream_for project; end` | `async fn subscribed(&self, ctx, user, params) -> Result<Subscription>`: `Ok(Subscription::stream_for(project.id))`, or `live::reject()` |
| `stream_for … do \|message\| … end` (filter) | `Subscription::stream_for(key).filter(\|payload\| async move { … })`, checked per message |
| `ProjectsChannel.broadcast_to(project, data)` | `ProjectsChannel::broadcast_to(project.account_id, project.id, json!({…}))` (generated channels key by account and id; or `live::broadcast_to("ProjectsChannel", key, data)`) |
| `def perform_action(data)`, client `subscription.perform("action", data)` | `async fn perform(&self, ctx, user, subscription, action, data)`; client `subscription.perform(action, data)` → `POST /live/perform` |
| `consumer.subscriptions.create({channel, …}, {received})` | `subscribe("ProjectsChannel", { project: id }, { received })` in `frontend/lib/live.ts` |
| the `/cable` connection, cookie-authenticated | one SSE stream per browser tab, `GET /live?s=[identifiers]`, signed-in users |
| async adapter (one process) | an in-process `tokio::sync::broadcast` hub (`src/live/mod.rs`) |
| Solid Cable / Redis adapter (several processes) | not built; see [Several app servers](#several-app-servers) |

`src/channels/account.rs` (`AccountChannel`) is the kit's own: members of an account subscribe
with `{account: slug}`, `Membership` broadcasts `{"type":"members"}` when someone joins, leaves
or changes role, and `frontend/pages/members/index.tsx` reloads its lists and shows who else
has the page open.

## Generate a channel

```sh
cargo loco generate channel projects
```

| Writes | |
|---|---|
| `src/channels/projects.rs` | `ProjectsChannel` with `NAME`, `broadcast_to(account_id, id, payload)`, and a `subscribed` that accepts members of the account in `params.account` and streams `params.id` *within that account* (the key is `"{account_id}:{id}"`, so the same id in another account is never received; still look the record up in the account to reject ids that aren't there) |
| `src/channels/mod.rs` | `pub mod projects;` and `Arc::new(projects::ProjectsChannel)` in `all()` |
| `tests/requests/projects_channel.rs` | a member's subscription is confirmed and receives a broadcast; a non-member's is rejected and receives nothing; another account's broadcast for the same id doesn't arrive |

It prints the client snippet. Rails: `rails g channel projects`.

## Notify, then reload (the Inertia way)

Send a tiny "something changed" message and let the page re-fetch its own props with a partial
reload. The server keeps rendering; no second data format, no client-side store.

**Server**, after the write commits (in the model method, which has no `AppContext`: the hub is
a `static`):

```rust
txn.commit().await?;
ProjectsChannel::broadcast_to(project.account_id, project.id, json!({ "type": "changed" }));
```

**Client:**

```tsx
import { useLiveReload } from "@/lib/live"

useLiveReload("ProjectsChannel", { account: slug, id: project.id }, { only: ["todolists"] })
```

`useLiveReload` takes `when: (message) => boolean` to skip messages, and `useChannel(channel,
params, (message) => …)` is the general hook (it returns `perform`). `subscribe(...)` is the
non-React API.

## Echo suppression

The tab that made a change already shows it. Every Inertia request sends `X-Tab-Id` (set by
`frontend/lib/live.ts`); `live::layer` keeps it for the request, `broadcast_to` stamps it on the
message, and that tab's client skips it. `subscribe(…, { echo: true })` keeps them. Work outside
a request (a job) broadcasts with no tab: everyone reloads. `live::with_tab(tab, fut)` carries a
tab into spawned work.

## Presence

```rust
fn tracks_presence(&self) -> bool { true }
```

```tsx
const here = usePresence("ProjectsChannel", { account: slug, id }, { editing: todoId })
// [{ user_id, name, state }], one per person, sorted by name
```

The hook sends a heartbeat every 15 s (`POST /live/presence`, at once when `state` changes) and a
goodbye when the page closes; a tab not heard from for 30 s expires. Every subscriber of the
same stream key gets `{type: "presence", present}` when who is there (or their state) changes;
plain heartbeats send nothing. The room is the subscription's first `stream_for` key, so only
people the channel accepts can see or join it. In memory, like the hub.

## Authorization

`subscribed` runs when the stream opens, again before every `perform` and presence
heartbeat, and again every 15 s on an open stream (`live::RECHECK_EVERY`). A rejected
subscription gets `reject_subscription` and nothing else; a stream whose session was deleted
(signed out, password changed) ends. So a removed member stops receiving within 15 s. For a
rule that must hold message by message, add `.filter(…)`, which runs per message. Return `live::reject()` for "no": the response never says
whether the record exists.

## Client to server: `perform`

```rust
async fn perform(&self, ctx: &AppContext, user: &CurrentSession, sub: &Subscription,
                 action: &str, data: &Value) -> Result<()> {
    match action {
        "typing" => { Self::broadcast_to(&sub.keys()[0], json!({ "typing": user.user.name })); Ok(()) }
        _ => Err(Error::NotFound),
    }
}
```

```tsx
const perform = useChannel("ProjectsChannel", params, onMessage)
void perform("typing", {})
```

It is a `POST`, not a WebSocket message: it takes the same session cookie, CSRF check and
proxies as every other write, and needs no second protocol. SSE carries server-to-client;
everything a page changes is already an Inertia request. A WebSocket transport would only pay off
for high-rate client messages (cursor positions, games); it would plug in behind the same
`subscribe`/`perform` API.

## Kit-specific details

- **Request timeout.** Production's `timeout_request` (30 s) bounds the time to the response
  headers, not the body, so it never cuts a stream (`tests/requests/live.rs` checks it). The
  stream still ends now and then (deploys, proxies); `retry: 1000` reconnects within a second.
- **Proxies** must not buffer `text/event-stream` (kamal-proxy and Cloudflare don't); a comment
  every 15 s keeps idle ones from closing it.
- **One stream per tab** carries every subscription; adding or removing one reopens it with the
  new set. Browsers allow 6 HTTP/1.1 connections per host, which is why it's one, not one per
  channel; behind HTTP/2 (any TLS proxy) it doesn't matter.
- **Missed messages.** A message sent while a tab is reconnecting is missed, as with Action
  Cable. Notify-then-reload makes that harmless: the next message reloads the current state.
  A slow subscriber that falls 1,024 messages behind skips them.

## Several app servers

The hub and presence live in one process, which matches the one-container SQLite deploy. With
several app servers, `broadcast_to` must reach the others:

- **SQLite, Solid Cable style:** an `live_messages (id, channel, key, payload, tab_id,
  created_at)` table; `broadcast_to` inserts a row, each process polls `WHERE id > last_seen`
  every ~100 ms and feeds its local hub, and a scheduled task trims old rows. Works on one host
  with several processes sharing the file.
- **Postgres:** `NOTIFY live, '<json>'` in `broadcast_to`, one `LISTEN live` connection per
  process feeding the local hub (payloads up to 8 KB: send ids, not records).

Either way the change is inside `live::send` and a background task; channels, the client and
the tests stay as they are. Presence needs the same treatment (a table with `seen_at`). Neither
is built here.

## Verify

```sh
cargo test --test mod requests::live            # authorization, fan-out, echo, presence, perform, SSE
npx playwright test e2e/live.spec.ts            # two browsers on the members page
```
