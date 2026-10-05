# Rails → Loco: the commands in this kit

If you know Rails, you already know this app's shape. This page maps each Rails command to what
you run here. Every command below was run against this kit (Loco 1.2); where a Loco generator
needs something the Rails one doesn't, it says so.

`cargo loco` is an alias for `cargo run --` (`.cargo/config.toml`), so every `cargo loco …`
compiles the app first and then runs its CLI. In production the same CLI is the binary itself:
`/app/inertia_rust_starter_kit-cli db migrate`.

## Project and server

| Rails | This kit | Notes |
|---|---|---|
| `rails new myapp` | `git clone` this repo, then `bin/rename myapp` | See [BUILDING_YOUR_APP.md](BUILDING_YOUR_APP.md). `loco new` makes a bare Loco app without the Inertia adapter, auth or frontend. |
| `bin/setup` | `bin/setup` | Installs npm deps, builds, migrates, seeds a fresh DB, starts `bin/dev`. `--skip-server`, `--reset`. |
| `bin/dev` | `bin/dev` | `cargo loco start --server-and-worker` + Vite. `bin/dev --all` adds the scheduler. |
| `rails server` | `cargo loco start` | Server only. `--worker` for only the queue worker, `--server-and-worker`, `--all`. |
| `bin/ci` | `bin/ci` | Same steps as the Rails kit's `config/ci.rb`. |
| `rails about` / `rails doctor`-ish | `cargo loco doctor` | Checks config, DB, queue, and dev tools such as `sea-orm-cli`. |
| `rails --version` | `cargo loco version` | |

## Generators

| Rails | This kit | Notes |
|---|---|---|
| `rails g scaffold posts title:string body:text` | `cargo loco generate scaffold posts title:string! body:text` then `cargo loco task scaffold:pages resource:posts` | Two steps: Loco writes the Rust half from `.loco-templates/` (migration, entity, model with params/validation, Inertia controller, route-table entries, request test); the task writes the React pages and the sidebar link. **In the account** by default (`account:references` added, `/{account_slug}/posts`, `CurrentAccount`, scoped finders, a cross-account 404 test); `--global` for a resource outside accounts. Columns the forms can't edit (`tstz`, …) are left out with a note. Needs `sea-orm-cli`. Generating several? All `generate` commands first, then the tasks: each `generate` rebuilds the CLI. |
| `rails g model post title:string` | `cargo loco generate model posts title:string!` | Plural name. Runs `db migrate` + `db entities` for you. Needs `sea-orm-cli`. |
| `rails g migration AddSlugToPosts slug:string` | `cargo loco generate migration AddSlugToPosts slug:string` | Then `cargo loco db migrate && cargo loco db entities`. Unlike Rails, the table must be one word: `AddSlugToCrmAccounts` writes an empty `todo!()` migration, which you fill in with `add_column(m, "crm_accounts", "slug", ColType::StringNull)` (`.claude/skills/loco/recipes/model-and-migration.md`). |
| `rails g controller reports summary` | `cargo loco generate controller reports summary` then `cargo loco task scaffold:pages controller:reports` | The kit's template (`.loco-templates/controller/`): an Inertia page per action (plus `index`) at `/{account_slug}/reports` and `/{account_slug}/reports/summary` (`--global`: `/reports`), its paths and routes in `src/route_table.rs` (`REPORTS`, `REPORTS_SUMMARY`), and a request test; the task writes `frontend/pages/reports/{index,summary}.tsx` and regenerates `frontend/routes/`. Props and the sidebar link are yours; see `.claude/skills/starter-kit/recipes/inertia-page.md`. |
| `rails g controller notes create update destroy` | `cargo loco generate controller notes create update destroy` | Write handlers: `POST /{account_slug}/notes`, `PATCH`/`DELETE /{account_slug}/notes/{id}`, each redirecting back (with `errors` when `NoteParams::errors()` has any) and refusing Precognition; a request test for each. No pages, and no `index` (a controller of write actions only gets no GET route). |
| `get "files/:id", to: "files#show"`, `get "files/*path"` | `cargo loco generate controller files show:id` (or `show:slug`, or `show:*path`) | `show:<param>` is the member page at `/{account_slug}/files/{param}`: `show:id` an `i64` (and the same `{id}` path as `update`/`destroy`, which it can be generated with), any other name a `String`, and `show:*path` a glob, the rest of the path with its slashes (axum's `{*path}`, Rails' `*path`; keep it the last segment). It adds `FILE`, `file_path(slug, path)` (a glob's segments encoded, its slashes kept), the handler (the param goes to the page as a prop), and a test. Only `show` takes a param; for other member routes add them to `src/route_table.rs` by hand. |
| `rails g controller Todos::Completions create destroy` | `cargo loco generate controller todos/completions create destroy` | Rails' namespaced controller as a nested singular resource: `src/controllers/todos/completions.rs` (`pub mod completions;` in `todos/mod.rs`, or in `todos.rs` when that is a flat module), `TODO_COMPLETION = /{account_slug}/todos/{todo_id}/completion`, `todo_completion_path(slug, todo_id)`, TS module `Todos/CompletionsController`. With an account-scoped `todos` model, the handlers look the to-do up in the account (another account's id is a 404). Write actions only. |
| `rails g channel projects` | `cargo loco generate channel projects` | `src/channels/projects.rs` (`ProjectsChannel`: `subscribed` accepts members of the account in `params.account`, `broadcast_to`), registered in `src/channels/mod.rs`, and a test (a member receives, a non-member is rejected). The kit's own generator: Loco has none. Recipe: `.claude/skills/starter-kit/recipes/live-updates.md`. |
| `rails g job ReportExport` | `cargo loco generate worker report_export` | Registered in `App::connect_workers`, with a test in `tests/workers/`. |
| `rails g mailer Digest` | `cargo loco generate mailer digest_mailer` | Tera templates in `src/mailers/digest_mailer/welcome/`; see the mailer recipe for the kit's pattern (a worker that renders and sends). |
| `rails g task` / a rake task | `cargo loco generate task cleanup_sessions` | Registered in `App::register_tasks`, with a test in `tests/tasks/`. |
| (whenever / Solid Queue recurring) | `cargo loco generate scheduler` | Writes `config/scheduler.yaml`, which is read **only** with `--config config/scheduler.yaml`. Put jobs under `scheduler:` in `config/<env>.yaml` to have `cargo loco start --all` pick them up. |
| `rails g … --help` | `cargo loco generate --help` | `cargo loco generate override --info` lists every template you can take over. |

Column types: bare is **nullable** and `!` is `NOT NULL` (Rails' `null: false`), `^` is unique
and not null. `references` is the exception: bare is `NOT NULL`, `references?` is nullable.

## Database

| Rails | This kit | Notes |
|---|---|---|
| `rails db:migrate` | `cargo loco db migrate` | |
| `rails db:rollback` | `cargo loco db down` | `down 2` for two steps. |
| `rails db:migrate:status` | `cargo loco db status` | |
| `rails db:reset` | `cargo loco db reset` then `cargo loco db seed` | |
| `rails db:seed` | `cargo loco db seed` | Loads `src/fixtures/*.yaml` (Loco's seeds are YAML fixtures). `--reset` truncates first. |
| `rails db:schema:dump` | `cargo loco db schema` | |
| (Active Record reads the schema) | `cargo loco db entities` | Regenerates `src/models/_entities/` from the live DB. Rust has no runtime reflection, so this step is how the model learns its columns; run it after every migration. |
| `rails dbconsole` | `sqlite3 inertia_rust_starter_kit_development.sqlite` | |

## Console, runner, routes

| Rails | This kit | Notes |
|---|---|---|
| `rails console` | no REPL. Write a task, or a test | Rust has no REPL on a compiled app. For one-off data work: `cargo loco generate task fix_something`, then `cargo loco task fix_something key:value`. For exploring an API, a `#[tokio::test]` with `boot_test::<App>()` is the fastest loop. |
| `rails runner 'Foo.bar'` | `cargo loco task <name> key:value` | Task arguments are `key:value` pairs, read with `vars.cli_arg("key")`. |
| `rails routes` | `cargo loco routes` | What axum registered. The source of truth is `src/route_table.rs`. |
| (js-routes / Typelizer) | `cargo loco task routes:generate` | Writes `frontend/routes/*.ts` from `src/route_table.rs`; `tests/routes_fresh.rs` fails if they drift. |
| `rails middleware` | `cargo loco middleware` | |

## Jobs, mail, credentials

| Rails | This kit | Notes |
|---|---|---|
| `bin/jobs` (Solid Queue) | `cargo loco start --worker` | `bin/dev` runs `--server-and-worker`, the Docker image `--all` (plus the scheduler). The queue is SQLite (`QUEUE_URL`), so no Redis. |
| `MissionControl::Jobs` | `cargo loco jobs retry\|cancel\|tidy\|purge\|dump` | CLI only; no UI. The queue table exists once a worker has started (`bin/dev` does it); before that these report `no such table: sqlt_loco_queue`. |
| `deliver_later` | `UserMailer::email_verification(&ctx, &user)` | Enqueues a worker that renders and sends. See `src/mailers/user_mailer.rs`. |
| letter_opener | Mailpit on `localhost:1025`, or `mailer.stub: true` | Tests assert on `ctx.mailer.deliveries()`. |
| `rails credentials:edit` | environment variables | `config/production.yaml` reads every secret with `get_env(name="…")` and no default; Kamal sets them from `.kamal/secrets`. |
| `rails secret` | `bin/secret` | Prints a `SECRET_KEY_BASE`. |
| `Rails.application.config.x` | `settings:` in `config/<env>.yaml` | Read as `crate::controllers::settings(&ctx)`. |

## Tests

| Rails | This kit | Notes |
|---|---|---|
| `bin/rails test` / `rspec` | `cargo test` | Request tests in `tests/requests/`, model tests in `tests/models/`. `sign_in`, `inertia_get` and `assert_redirect` are in `tests/requests/mod.rs`. |
| system tests (Capybara) | `npx playwright test` | Runs against the release binary, client- and server-rendered. Signed-in specs use `e2e/fixtures.ts` (one@ and two@ signed in once per server, from `storageState`). |
| `assert_queries(n) { … }` / `inertia_rails` matchers (`expect_inertia.to include_props`) | `assert_max_queries(n, …)`, `assert_props_exactly`, `assert_deferred`, `assert_optional_absent`, `assert_payload_under` | `tests/requests/budget.rs`; `expectPartialReload` in `e2e/budget.ts`. Every new page or prop gets one. |
| `rails test test/models/post_test.rb` | `cargo test --test mod models::posts` | All Rust tests live in one binary (`tests/mod.rs`) except a few protocol tests. |

## Accounts (organizations)

The Rails kit has no organizations; this kit has Basecamp-style accounts built in
(`.claude/skills/starter-kit/recipes/accounts.md`).

| Rails (Basecamp style) | This kit | Notes |
|---|---|---|
| `scope ":account_slug" do … end` | paths starting `/{account_slug}` in `src/route_table.rs` | `auth::slug_constraint` plays `constraints: { account_slug: /[a-z0-9-]{3,40}/ }`. |
| `Current.account`, `Current.membership` (set in a concern) | `current: CurrentAccount` as the handler's first extractor | `current.account`, `current.membership`, `current.session.user`. A non-member gets 404. |
| `Current.account.projects.find(params[:id])` | `projects::Model::find_in_account(&db, current.account.id, id)` | Write the scoped finder on the model; never call an unscoped `find_by_id` from an account page. |
| `before_action :require_admin` | `if !current.is_manager() { return Ok(current.forbidden(&headers)); }` | Redirects back with "You don't have permission to do that". |
| `Membership.roles` enum | `memberships::Role` (`Owner`, `Admin`, `Member`) | Stored as a string column. |
| `AccountMailer.invite(invitation).deliver_later` | `InvitationMailer::invite(&ctx, &invitation)` | The worker mints the token at send time. |
| Devise `:registerable` off / invitation-only sign-up | `SIGN_UP=invitation_only` (`settings.sign_up`) | Sign-up only through a pending account invitation, with its address; the sign-in page hides "Sign up" unless an invitation is carried. Default `open`. `.claude/skills/starter-kit/recipes/accounts.md`, "Invitation-only sign-up". |

## Live updates (Action Cable)

| Rails | This kit | Notes |
|---|---|---|
| `ProjectsChannel.broadcast_to(project, data)` | `ProjectsChannel::broadcast_to(project.account_id, project.id, json!({…}))` | After the write commits; from a model too (the hub is a `static`). |
| `def subscribed; reject unless …; stream_for project` | `async fn subscribed(..) -> Result<Subscription>`: `Ok(Subscription::stream_for(id))` or `live::reject()` | Runs again before every `perform` and presence heartbeat, and every 15 s on an open stream. |
| `stream_for project do \|msg\| … end` | `Subscription::stream_for(id).filter(\|payload\| async move { … })` | Checked per message. |
| `consumer.subscriptions.create(…, { received })` | `subscribe(channel, params, { received })`, `useChannel`, `useLiveReload(channel, params, { only })` | `frontend/lib/live.ts`. |
| `subscription.perform("x", data)` | `perform("x", data)` → `Channel::perform` | A POST (`/live/perform`), not a WebSocket frame. |
| `/cable` (WebSocket) | `GET /live` (Server-Sent Events, one per tab) | Survives `timeout_request`; `retry: 1000`. |
| async / Solid Cable adapter | in-process hub | Several servers: see the recipe's sketch (SQLite polling or Postgres `LISTEN/NOTIFY`). |
| (Turbo / custom) presence | `fn tracks_presence() -> bool { true }` + `usePresence(channel, params, state)` | 15 s heartbeat, 30 s expiry. |

## Where things go

| Rails | This kit |
|---|---|
| `app/models/post.rb` | `src/models/posts.rs` (yours) + `src/models/_entities/posts.rs` (generated, never edit) |
| `app/controllers/posts_controller.rb` | `src/controllers/posts.rs` |
| `config/routes.rb` | `src/route_table.rs` (paths) + each controller's `routes()` |
| `app/frontend/pages/posts/index.tsx` | `frontend/pages/posts/index.tsx` |
| `app/jobs/` | `src/workers/` |
| `app/mailers/` + views | `src/mailers/<name>.rs` + `src/mailers/<name>/<message>/{subject,html,text}.t` |
| `lib/tasks/*.rake` | `src/tasks/` |
| `db/migrate/` | `migration/src/` |
| `db/seeds.rb` | `src/fixtures/*.yaml` + `App::seed` in `src/app.rs` |
| `config/environments/*.rb` | `config/{development,test,production}.yaml` |
| `config/initializers/` | `App::after_context` / `App::initializers` in `src/app.rs` |
