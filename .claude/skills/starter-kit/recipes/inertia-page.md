# Recipe: an Inertia page, and its props

**When:** a new screen that isn't CRUD (a report, a settings tab), or you need deferred,
partial, merged or shared props. Rails: a controller action with `render inertia:`.

## A new page, end to end

```sh
cargo loco generate controller reports summary   # GET actions; `index` is always there
cargo loco task scaffold:pages controller:reports
```

Pages live in the account by default (`/{account_slug}/reports`); add `--global` for a page
outside accounts (`/reports`, `Authenticated`).

| Writes | |
|---|---|
| `src/controllers/reports.rs` | `index` and `summary`: `CurrentAccount` + `render(inertia, "reports/<action>", json!({}))` |
| `src/route_table.rs` | `REPORTS = "/{account_slug}/reports"`, `REPORTS_SUMMARY = "/{account_slug}/reports/summary"`, `reports_path(slug)` and their routes |
| `src/controllers/mod.rs`, `src/app.rs` | `pub mod reports;`, `.add_route(controllers::reports::routes())` |
| `tests/requests/reports.rs` (+ `mod reports;`) | each page renders its component for one@ in Acme; signed-out visitors go to sign in; a non-member gets 404 |
| `frontend/pages/reports/{index,summary}.tsx` | placeholder pages in `AppLayout` (the task; it never overwrites) |
| `frontend/routes/` | regenerated (the task runs `routes:generate`) |

Then fill it in:

1. **Props**: query on a model, pass the result.
   ```rust
   async fn index(current: CurrentAccount, State(ctx): State<AppContext>, inertia: Inertia) -> Result<Response> {
       let total = reports::Model::total(&ctx.db, current.account.id).await?;
       render(inertia, "reports/index", json!({ "total": total })).await
   }
   ```
   A record or a list of them goes through a props struct, so its TypeScript type is generated
   (see [Typed props](#typed-props)).
2. **Page**: type the props and render them.
   ```tsx
   export default function ReportsIndex({ total }: { total: number }) {
   ```
3. **Sidebar**: add an entry in `frontend/components/app-sidebar.tsx` if it belongs in the nav
   (in the account's list: `href: reports.index(account.slug).url`).

`create`, `update` and `destroy` actions are write handlers, not pages: see
[Write actions](#write-actions-and-nested-controllers) below.

## Write actions and nested controllers

```sh
cargo loco generate controller notes create update destroy
cargo loco generate controller todos/completions create destroy   # Todos::CompletionsController
```

A write action answers `POST`/`PATCH`/`DELETE`, refuses Precognition (`NoPrecognition`), and
**redirects back** (Referer, else the index): with `errors` when `<Singular>Params::errors()`
has any, else plain. The generated `NoteParams {}` and its `errors()` are where the form's
fields and checks go; the write itself belongs on a model method you call where the generated
comment says. Rails: a controller with `redirect_back_or_to`, no views.

| | `notes create update destroy` | `todos/completions create destroy` |
|---|---|---|
| file | `src/controllers/notes.rs` | `src/controllers/todos/completions.rs` (+ `pub mod completions;` in `todos/mod.rs`, or in `todos.rs` when it is a flat module like a scaffold's) |
| paths | `NOTES = /{account_slug}/notes` (`create`; no index page), `NOTE = /{account_slug}/notes/{id}` | `TODO_COMPLETION = /{account_slug}/todos/{todo_id}/completion`, `todo_completion_path(slug, todo_id)` |
| routes | `notes.create` POST, `notes.update` PATCH, `notes.destroy` DELETE | `todos.completions.create` POST, `todos.completions.destroy` DELETE; TS module `Todos/CompletionsController` (`todosCompletions`) |
| test | each action redirects back, refuses Precognition; a non-member gets 404 | the same, plus another account's to-do through your URL is a 404 |

A nested controller is Rails' singular `resource :completion` inside `resources :todos`: write
actions only (pages go on the parent's controller). When `src/models/todos.rs` has
`find_in_account` (an account-scoped scaffold), the handlers look the to-do up there first, so
another account's id is a 404, and redirect back to the to-do's page; otherwise a comment says
where to add that lookup.
A controller of write actions only (`notes create`) gets no `index`: no page, no GET route and
no page test.

## A member page: `show:<param>`

```sh
cargo loco generate controller files show:id       # /{account_slug}/files/{id}, an i64
cargo loco generate controller files show:slug     # /{account_slug}/files/{slug}, a String
cargo loco generate controller files show:token    # any other name too: {token}, a String
cargo loco generate controller files show:*path    # /{account_slug}/files/{*path}, a glob
```

`show:<param>` writes `FILE` and `file_path(slug, param)` in `src/route_table.rs`, a `show`
handler taking `Path((_, param))` and passing it to `files/show` as a prop, and a test that
renders it. A glob (`*path`) takes the rest of the URL with its slashes, so keys like
`us/illinois/madison` work: `file_path("acme", "us/illinois/madison")` encodes each segment and
keeps the slashes, and the TS route is Rails' `/:account_slug/files/*path`, which `runtime.ts`
fills the same way. Keep a glob the last segment. `show:id` can be generated with `update` and
`destroy` (one `{id}` path); a String or glob param can't, since they take `{id}` at the same
path. Look the record up in the account in `show` (another account's is a 404).

To add a page to an existing controller, add the path and route to `src/route_table.rs` by
hand (as the generator does), the handler and `.add(..)`, then rerun
`cargo loco task scaffold:pages controller:reports` for the new page.

## Typed props

A props struct is the page's contract: Rust builds it, and its TypeScript type is generated from
it, so renaming or retyping a field fails `npm run check` instead of rendering `undefined`.

```rust
// src/models/reports.rs
#[derive(Debug, Clone, PartialEq, Serialize, ts_rs::TS)]
pub struct ReportProps {
    pub id: i64,
    pub title: String,
    pub closed_on: Option<Date>, // `string | null`
}

impl Model {
    pub fn to_props(&self) -> ReportProps { ReportProps { id: self.id, title: self.title.clone(), closed_on: self.closed_on } }
}
```

1. List it in `generate_ts` in `src/page_types.rs` (`types.add::<crate::models::reports::ReportProps>();`
   above `// scaffold:types`; scaffolds add theirs there). Types it uses (an enum like `Role`)
   are generated with it.
2. `cargo loco task types:generate` writes `frontend/types/generated/ReportProps.ts`. Commit it;
   `tests/types_fresh.rs` (in `bin/ci` and CI) fails when it is stale. Never edit those files.
3. Send it: `render(inertia, "reports/show", json!({ "report": report.to_props() }))`, or
   `Props::new().prop("report", Prop::serialize(&report.to_props())?)` next to lazy/deferred props.
   `render` also takes any `Serialize` struct whose fields are the props.
4. Import it in the page:
   ```tsx
   import type { ReportProps } from "@/types/generated/ReportProps"

   export default function ReportShow({ report }: { report: ReportProps }) {
   ```

Field types: `i64`/`f64` are `number`, `Option<T>` is `T | null`, `Date` and the ISO strings
from `models::as_json_time` are `string`, a serde `rename_all = "snake_case"` enum is a union of
strings. `#[serde(rename_all = "camelCase")]` on the struct renames the TS fields too.
A one-off scalar (`{ total: number }`, a flag) can stay `json!` with an inline TS type.

## Prop kinds

`render(inertia, name, json!(..))` sends plain props. For the others, build `Props` and call
`inertia.render` (`src/inertia/props.rs`, full table in `docs/INERTIA.md`):

```rust
use crate::inertia::{defer, lazy, merge, optional, Props};

inertia.render("reports/index", Props::new()
    .prop("total", 42)                                            // plain
    .prop("chart", defer(move || async move { slow_chart(&db).await }))  // after first paint
    .prop("filters", optional(|| async { Ok(json!([..])) }))      // only when asked for
    .prop("rows", merge(move || async move { page_of_rows(&db, page).await })), // appended on reload
).await
```

| Rails (`inertia_rails`) | This kit | Client |
|---|---|---|
| `-> { }` | `lazy(..)` | evaluated only if kept |
| `InertiaRails.defer` | `defer(..)`, `.group("g")` | `<Deferred data="chart" fallback={..}>` |
| `InertiaRails.optional` | `optional(..)` | `router.reload({ only: ["filters"] })` |
| `InertiaRails.merge` | `merge(..)`, `.prepend()`, `.match_on("id")` | `router.reload({ only: ["rows"], data: { page: 2 } })` |
| `InertiaRails.scroll` | `scroll(ScrollMetadata::new(..), ..)` | `<InfiniteScroll data="rows">` |
| `InertiaRails.once` | `once(..)` | cached client-side |
| `inertia_share` | `SharedProps` in `ctx.shared_store` (see `auth::register_shared_props`) | `usePage().props` |

Closures are `'static`: clone what they need (`let db = ctx.db.clone();`) and `move` it in.
**Partial reloads** (`only`/`except`) work for every prop without extra code; a `lazy`/`defer`
closure runs only when its prop is sent.

To add a **shared prop**, extend the closure in `auth::register_shared_props`: one
`SharedProps` is stored, so a second `insert` would replace the `auth` prop. Give it a props
struct (`Prop::serialize(&value)?`, as `auth` and `accounts` do), list it in `src/page_types.rs`,
and add the key with its generated type to `SharedProps` in `frontend/types/index.ts`.

## Verify

The generated `tests/requests/reports.rs` checks each page renders; add the props:

```rust
let page = inertia_get(&server, &ctx, route_table::REPORTS).await;
assert_eq!(page["component"], "reports/index");
assert_eq!(page["props"]["total"], 42);
```

Then `cargo test --test mod requests::reports`, `cargo test --test routes_fresh`, `npm run check`.

## Budget tests

Every page and prop gets one (rule 7 in `SKILL.md`). The helpers are in
`tests/requests/budget.rs` (`use super::budget::*;` from another request test):

| Helper | Checks |
|---|---|
| `visit(&server, &ctx, path)` / `partial(&server, &ctx, path, component, &["only"])` | an Inertia visit / partial reload, as the response |
| `assert_props_exactly(&res, &["a", "b"])` | the page's own props are exactly these (shared `auth`, `accounts`, `errors` aside) |
| `assert_deferred(&res, "stats")` | absent from the first load, listed in `deferredProps` |
| `assert_optional_absent(&res, "members")` | not sent unless a partial reload asks |
| `assert_payload_under(&res, bytes)` | the response body's size |
| `assert_max_queries(n, \|\| async { vec![visit(..).await] }).await` | none of those requests ran more than `n` SQL queries (`Server-Timing: db;desc="N queries"`, counted per request in `src/db.rs`); returns the most any ran |

```rust
#[tokio::test]
#[serial]
async fn the_reports_page_defers_its_totals_and_stays_cheap() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = visit(&server, &ctx, "/acme/reports").await;
        assert_props_exactly(&res, &["filters"]);
        assert_deferred(&res, "totals");
        assert_payload_under(&res, 4_000);
        let res = partial(&server, &ctx, "/acme/reports", "reports/index", &["totals"]).await;
        assert_props_exactly(&res, &["totals"]);
        assert_max_queries(6, || async { vec![visit(&server, &ctx, "/acme/reports").await] }).await;
    })
    .await;
}
```

Set the numbers a little above today's (`assert_max_queries` returns the measured count), so the
test fails on a new N+1, not on noise. The first visit to an account page also records it as the
user's last account (one more write), so measure a second visit.

In the browser, `expectPartialReload(page, action, { only })` from `e2e/budget.ts` runs `action`
and checks it caused a partial reload asking for `only` and receiving only those props
(`e2e/live.spec.ts` uses it for a live update).
