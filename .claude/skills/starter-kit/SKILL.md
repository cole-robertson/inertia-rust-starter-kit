---
name: starter-kit
description: Use when extending THIS app (the Inertia Rust starter kit: Loco + Inertia.js + React + shadcn/ui) — adding a resource/CRUD, an Inertia page or props, a form with validation errors or precognition, a background job, scheduled task, mailer, file upload, cache, live updates, accounts/organizations (scoping, roles, invitations), or planning billing/admin, or deploying it. Carries the kit-specific commands, file paths and verification steps; defers generic Loco API questions to the `loco` skill.
---

# Extending the Inertia Rust starter kit

This app is a Loco app with its own Inertia adapter (`src/inertia/`) and a React frontend
(`frontend/`). Two skills cover it:

- **`loco`** (`.claude/skills/loco/`): Loco itself. Doctrine, the full `loco_rs` API index,
  Sea-ORM, generic recipes. Check `api-index.md` there before guessing any `loco_rs` name.
- **`starter-kit`** (this one): what is different in *this* kit, and the exact steps to extend
  it. Start here; follow the links into `loco` for API detail.

## What this kit adds on top of Loco

| Thing | Where | Rails equivalent |
|---|---|---|
| Inertia rendering: `Inertia` extractor, `render`, prop kinds | `src/inertia/`, `docs/INERTIA.md` | `inertia_rails` |
| Redirects with flash and validation errors | `inertia::redirect::Redirect` (`.notice()`, `.alert()`, `.errors()`) | `redirect_to …, notice:, inertia: { errors: }` |
| Session auth: `Authenticated`, `RequireGuest` extractors | `src/auth.rs` | `before_action :authenticate` |
| Accounts: `CurrentAccount` extractor (`/{account_slug}/…`, 404 for non-members), memberships with roles, invitations, the switcher | `src/auth.rs`, `src/models/{accounts,memberships,invitations}.rs` | Basecamp's `Current.account`, `scope ":account_slug"` |
| Every URL, typed on both sides | `src/route_table.rs` → `cargo loco task routes:generate` → `frontend/routes/*.ts` | `config/routes.rb` + js-routes/Typelizer |
| Rails-style params (query + JSON/form body) | `controllers::Params<T>` | `params.permit` |
| Validation errors as `{field: [messages]}` | `models::users::Errors`, `SaveError::Invalid` | `model.errors` |
| Form casting (strings → column types, Rails messages) | `src/models/cast.rs` | Active Model type casting |
| Precognition | `controllers::precognitive`, `NoPrecognition` | `inertia_rails` precognition |
| Kit scaffold | `.loco-templates/` + `cargo loco task scaffold:pages` | `rails g scaffold` |
| Kit page controller | `cargo loco generate controller` (`.loco-templates/controller/`) + `scaffold:pages controller:<name>` | `rails g controller` |
| Live channels (SSE, `broadcast_to`, `subscribed`/reject, `perform`, presence) | `src/live/`, `src/channels/`, `frontend/lib/live.ts`, `cargo loco generate channel` | Action Cable |

## Recipes

Each has: when to use it, exact commands, the files it touches in this kit, the Rails
equivalent, and how to verify. Recipes marked **sketch** describe code that does **not** exist
in this kit.

| Task | Recipe |
|---|---|
| Add a CRUD resource (the generator) | [recipes/new-resource.md](recipes/new-resource.md) |
| Add an Inertia page; shared, deferred, partial, merge props | [recipes/inertia-page.md](recipes/inertia-page.md) |
| Forms, validation errors, precognition | [recipes/forms-and-validation.md](recipes/forms-and-validation.md) |
| Background job | [recipes/background-job.md](recipes/background-job.md) |
| Scheduled (recurring) task | [recipes/scheduled-task.md](recipes/scheduled-task.md) |
| Send email | [recipes/mailer.md](recipes/mailer.md) |
| File uploads (local disk, then S3/R2) | [recipes/file-uploads.md](recipes/file-uploads.md) |
| Cache | [recipes/cache.md](recipes/cache.md) |
| Live updates: channels, broadcasts, presence (`generate channel`) | [recipes/live-updates.md](recipes/live-updates.md) |
| Accounts (organizations): scope a resource to an account, check a role, invitations | [recipes/accounts.md](recipes/accounts.md) |
| Billing with Stripe (**sketch**) | [recipes/billing.md](recipes/billing.md) |
| Admin area (**sketch**) | [recipes/admin.md](recipes/admin.md) |
| Deploy (Kamal, Cloudflare Containers) | [recipes/deploy.md](recipes/deploy.md) |
| Performance notes | [recipes/performance.md](recipes/performance.md) |

Human-facing docs: `docs/BUILDING_YOUR_APP.md` (the standard path), `docs/RAILS_TO_LOCO.md`
(command cheat sheet).

## Rules specific to this kit

1. **URLs live in `src/route_table.rs`.** Add a `pub const`, a `route(...)` entry with its `ts(...)`,
   then `cargo loco task routes:generate`. Handlers register the constant; pages import from
   `@/routes`. `tests/routes_fresh.rs` fails if you forget to regenerate.
2. **Pages are Inertia components, not JSON endpoints.** A handler takes `Inertia` and returns
   `render(inertia, "posts/index", json!({...}))`; component names are paths under
   `frontend/pages/` without `.tsx`.
3. **Mutations redirect.** POST/PATCH/DELETE answer with `Redirect::to(path)`, never with JSON,
   carrying `.notice(..)` or `.errors(..)`. The flash and errors ride a cookie to the next GET.
4. **Validation errors are `string[]` per field** and messages match Rails ("can't be blank").
5. **Signed-in pages take `Authenticated`** as their first extractor; it redirects to sign-in.
   Pages with account data live under `/{account_slug}` and take `CurrentAccount` instead; every
   query is scoped to `current.account.id`, so another account's id is a 404.
6. **Never serialize an entity to the page.** Build a props struct explicitly (`to_props()` →
   `<Singular>Props` in scaffolded models) so a new column never leaks by accident. Its
   TypeScript type is generated (`src/page_types.rs`, `cargo loco task types:generate`): pages
   import `@/types/generated/<Name>`, never redeclare it.
7. **Every new page or prop gets a budget test** (`tests/requests/budget.rs` helpers): which
   props it sends (`assert_props_exactly`), which are deferred or optional, how many SQL queries
   it runs (`assert_max_queries`), and, for a partial reload, that it sends only what was asked.
   An agent adding a prop to a page is exactly how a page quietly gets slower; the budget makes
   it a failing test instead. Example in `recipes/inertia-page.md`, "Budget tests".
8. **Write transactions use `crate::db::begin_write(db)`** (`BEGIN IMMEDIATE`, as Rails 8 does), never `db.begin()`:
   a deferred transaction that reads, then writes, fails with SQLite's 517 when another connection wrote in between.

## Before you call it done

```sh
cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test
npm run lint && npm run format && npm run check
```

If you touched `src/route_table.rs`: `cargo loco task routes:generate`. For anything a user sees,
also run the page in `bin/dev` or `npx playwright test`. `bin/ci` runs all of it.
