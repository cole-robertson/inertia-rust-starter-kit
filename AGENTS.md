# Agent guide for this app

This is a [Loco](https://loco.rs) app (**Rails for Rust**) serving a React 19 frontend through
Inertia.js v3: a starter kit with auth, organizations, live updates and generators. It started from
the Evil Martians Inertia Rails React starter kit, and its behaviour, routes and flash texts still
match that kit. When you're unsure how something should work, the answer is almost always "the way
Rails (and the Rails kit) does it." Where Loco diverges, it's because Rust forced it.

## Read this first

A complete Loco skill ships with this app at **`.claude/skills/loco/`**, matched
to the exact `loco-rs` version in `Cargo.toml`:

| File | What it gives you |
|---|---|
| `.claude/skills/loco/SKILL.md` | start here: the router, `AppContext`, project layout, CLI |
| `.claude/skills/loco/doctrine.md` | what good Loco code looks like; read it before writing any |
| `.claude/skills/loco/api-index.md` | every public `loco_rs` symbol, generated from rustdoc. **Check here before guessing an API name** |
| `.claude/skills/loco/recipes/` | how to add a model, endpoint, worker, task, mailer, middleware, auth, tests |

A second skill, **`.claude/skills/starter-kit/`**, covers what this kit adds on top of Loco and
how to extend it: the resource generator, Inertia pages and props, forms and precognition, jobs,
scheduled tasks, mail, uploads, cache, live updates, deploy, accounts (organizations: scoping,
roles, invitations), and design sketches for billing and an admin area. **Start there when extending the app**; it links into the `loco`
skill for API detail.

If your tool supports Agent Skills, it will load `SKILL.md` automatically. If
not, read it directly. It's a normal markdown file.

For humans: `docs/BUILDING_YOUR_APP.md` (the standard path: rename, brand, generate, deploy)
and `docs/RAILS_TO_LOCO.md` (every Rails command and its equivalent here).

## Layout

| Path | What lives there |
|---|---|
| `src/app.rs` | `Hooks`: routes, initializers, workers, tasks, seeds |
| `src/route_table.rs` | **every URL in the app**, the single source for Rust routes and `frontend/routes/*.ts` |
| `src/page_types.rs` | the props structs whose TypeScript types are generated into `frontend/types/generated/` |
| `src/inertia/` | our Inertia v3 server adapter (page object, partial reloads, prop kinds, SSR, CSRF, flash/errors cookie, CSP) |
| `src/controllers/` | handlers; parse, call a model method, render an Inertia page or redirect |
| `src/models/` | SeaORM models with the domain logic (`users.rs`, `sessions.rs`, `tokens.rs`, `accounts.rs`, `memberships.rs`, `invitations.rs`); `_entities/` is generated |
| `src/live/`, `src/channels/` | live updates (the kit's Action Cable): the SSE hub, presence, and the app's channels |
| `src/mailers/`, `src/workers/` | mail (password reset, email verification) delivered on the SQLite queue |
| `src/tasks/` | `cargo loco task …`, including `routes:generate`, `types:generate` and `scaffold:pages` |
| `.loco-templates/` | the kit's generator templates: `cargo loco generate scaffold` writes Inertia code |
| `src/fixtures/` | seed data (`cargo loco db seed`); users have password `Secret1*3*5*` |
| `migration/` | SeaORM migrations |
| `config/{development,test,production}.yaml` | Loco config; app settings are under `settings:` |
| `frontend/` | React app: `pages/`, `components/` (shadcn/ui in `components/ui`), `layouts/`, `routes/` (**generated**), `types/` (`types/generated/` is **generated**) |
| `tests/` | Rust request/model/protocol tests; `tests/routes_fresh.rs`, `tests/types_fresh.rs` and `tests/entities_fresh.rs` guard the generated routes, prop types and SeaORM entities |
| `e2e/` | Playwright system test, run against the release binary via `bin/e2e-server`; mail goes to `e2e/mail-sink.ts` (read it with `e2e/mail.ts`) |
| `bin/` | `setup`, `dev`, `ci`, `e2e-server`, `secret` (prints a `SECRET_KEY_BASE`), `rename` |
| `Dockerfile`, `config/deploy.yml`, `.kamal/` | production image and Kamal 2 deploy |

## The rules that prevent most mistakes

1. **Generate, then edit.** `cargo loco generate <thing>` writes the file *and*
   the wiring. Rust has no autoloading; hand-wiring is how "the handler exists
   but 404s" happens. For CRUD: `cargo loco generate scaffold <plural> <field:type>...`
   then `cargo loco task scaffold:pages resource:<plural>` (needs `sea-orm-cli`).
2. **Use the batteries.** This app already has an ORM, queue, scheduler, mailer,
   task runner, storage, cache, and test harness. Adding a crate for something
   Loco already does is the most common mistake.
3. **Fat model, slim controller.** Domain logic on the model; handlers parse,
   call a model method, and render.
4. **Routes come from `src/route_table.rs`.** To add or change a URL, edit it there, then run
   `cargo loco task routes:generate` and commit `frontend/routes/`. Never hand-edit
   `frontend/routes/*.ts` (except `runtime.ts`).
5. **Page props are Rust structs; their TS types are generated.** A record on a page is a
   `#[derive(Serialize, ts_rs::TS)]` struct listed in `src/page_types.rs`; run
   `cargo loco task types:generate`, commit `frontend/types/generated/`, and import the type in
   the page (`import type { ProjectProps } from "@/types/generated/ProjectProps"`). Never
   redeclare a props type by hand in TypeScript, and never hand-edit the generated files.
6. **Match the Rails kit.** Flash texts, error messages and redirects must match
   `inertia-rails/react-starter-kit`. Frontend errors are `string[]` per field. The one
   deliberate addition is **accounts** (organizations): data lives under `/{account_slug}/…`,
   handlers take `CurrentAccount`, and every query is scoped to the account (another account's
   id is a 404). See `.claude/skills/starter-kit/recipes/accounts.md`.
7. **Write transactions use `crate::db::begin_write(db)`** (`BEGIN IMMEDIATE`, as Rails 8 does), never `db.begin()`.

## Running things

```sh
bin/setup                   # install, build, migrate, seed (--skip-server, --reset)
bin/dev                     # cargo loco start --server-and-worker + vite (PORT=5150, VITE_PORT=5173)
cargo test                  # Rust tests
npx playwright test         # system test, csr + ssr projects; bin/e2e-server builds what it needs
bin/ci                      # everything CI runs, fail-fast
```

## Before you call it done

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
npm run lint && npm run format && npm run check
```

If you touched routes: `cargo loco task routes:generate` and `cargo test --test routes_fresh`.
If you touched a props struct: `cargo loco task types:generate` and `cargo test --test types_fresh`.
If you touched a migration: `cargo loco db migrate && cargo loco db entities` (`db reset` first if it
already ran) and `cargo test --test entities_fresh`.
Or just run `bin/ci`.

**Every new page or prop gets a budget test**: its exact props, deferred/optional ones, a
query budget and a payload budget, with the helpers in `tests/requests/budget.rs`:

```rust
let res = visit(&server, &ctx, "/acme/reports").await;
assert_props_exactly(&res, &["filters"]);
assert_deferred(&res, "totals");
assert_max_queries(6, || async { vec![visit(&server, &ctx, "/acme/reports").await] }).await;
```

More in `.claude/skills/starter-kit/recipes/inertia-page.md`, "Budget tests". Playwright specs
that need a signed-in user import `test` from `e2e/fixtures.ts` (one@ as `page`, two@ as `two`,
signed in once per server) instead of signing in themselves. Mark elements for specs with
`data-test="…"` (not `data-testid`): `playwright.config.ts` sets `testIdAttribute: "data-test"`,
so `page.getByTestId("…")` finds them.

## More

- Docs: <https://loco.rs/docs/>
- Framework agent guide: <https://loco.rs/AGENTS.md>
- Inertia protocol: <https://inertiajs.com/the-protocol>
