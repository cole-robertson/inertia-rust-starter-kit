# Changelog

## Unreleased

Four ideas from inertia-omega (the Inertia team's Rust adapter) and inertia-laravel, ported
into the kit's own Inertia layer:

- **Lazy props resolve concurrently.** Sibling lazy and deferred props, and those on nested
  levels, run together instead of one after another: three 100 ms props now cost about 100 ms,
  not 300 ms. The props and every metadata list (`deferredProps`, `mergeProps`, `onceProps`,
  `rescuedProps`, ...) are still in prop order, and the error, when several fail, is the first
  one in prop order.
- **Redirects to a `#fragment` keep it.** An Inertia request answered with a redirect whose
  `Location` contains `#` gets `409` + `X-Inertia-Redirect`, and the client visits the URL
  itself (fetch drops fragments when it follows a redirect). Prefetches still get the redirect.
  As inertia-laravel does.
- **Prefetches leave the flash alone.** A request with `Purpose: prefetch` (what `<Link
  prefetch>` sends; also `Sec-Purpose` and `X-Moz`) neither shows, consumes nor writes the
  flash, so hovering a prefetching link no longer eats the "Saved" notice meant for the visit.
  `inertia::redirect::is_prefetch` reads the headers.
- **Deferred scroll props report `mergeProps: ["users"]` on the first visit,** as inertia-rails,
  inertia-laravel and inertia-omega do; the partial reload that loads them still reports
  `users.data`.
- **Error pages are readable with compression on.** A handler's 404 (an unknown invitation, a
  missing record) went out as `public/404.html` with the compressed body's `Content-Encoding`
  still set, so browsers couldn't decode it; the exceptions layer now drops it.
- **site/ pins `sharp` to ^0.35.5** too, for the same advisory (it comes in through the
  site's `wrangler`, via `miniflare`).
- **deploy/cloudflare pins `sharp` to ^0.35.5** with an npm override, for GHSA-wq5f-xc86-pv6w
  (high severity; `sharp` comes in through `wrangler`, `miniflare` and `cf`, whose own releases
  still pin 0.35.4). Remove the override once they move.

From building an app on the kit (Radar):

- **`tests/entities_fresh.rs`** checks the committed `src/models/_entities/` against what
  `db migrate` + `db entities` write in a temp copy (well under a second once built), as
  `routes_fresh`/`types_fresh` do for their files; `bin/ci` runs it before the slow steps, and
  CI's "Generated routes are fresh" job too. An edited migration (a unique index) no longer
  waits for the generator test to show its stale entities. `generate migration` prints the
  `db migrate && db entities` it needs.
- **`tokens::sign` / `tokens::verify_signed`**: signed, expiring tokens for any data and purpose
  (a one-click link for a record), on the same derived-key HMAC as the user tokens, whose format
  is unchanged.
- **Non-ASCII mail subjects** (RFC 2047 encoded words, folded) are decoded by `e2e/mail.ts`'s
  `lastMailTo` and the request tests' `decode_qp`.
- **`getByTestId` reads `data-test`** (`testIdAttribute` in `playwright.config.ts`), the one
  test-id attribute the pages use.
- `tests/rename.rs` passes in an app that deleted `site/`, and `bin/rename` says to delete
  `.github/workflows/site.yml` with it. Recipes: Loco's cron is UTC (a local-hour job),
  app-set scaffold columns, `show:<any name>` is a String param.

## 0.2.0 - 2026-10-06

- **Typed page props.** A page's props are a Rust struct deriving `Serialize` and `ts_rs::TS`,
  listed in `src/page_types.rs`. `cargo loco task types:generate` writes the matching
  TypeScript to `frontend/types/generated/`, and pages import those types instead of declaring
  their own. `tests/types_fresh.rs` (and a `bin/ci` step) fails when the generated types are
  stale, so renaming a field in Rust breaks the TypeScript build. `controllers::render` takes
  any `Serialize` value, and `Prop::serialize` keeps deferred, lazy and once props typed.
- **Scaffolds generate typed props:** `cargo loco generate scaffold` writes a
  `<Singular>Props` struct from the entity's field types (`Option<T>` becomes `T | null`), the
  controller returns it, and `scaffold:pages` regenerates the types the pages import. The
  kit's own shared props, account, member, invitation and session props are typed too.
- **Docs site at [inertia-rust.dev](https://inertia-rust.dev),** built from the repo's own
  markdown with VitePress (`site/`): a landing page, the guides and recipes, the reference,
  search, and `llms.txt` / `llms-full.txt`. Apps made from the template can delete `site/`.
  The live demo moved to [demo.inertia-rust.dev](https://demo.inertia-rust.dev).
- **New mark:** two violet chevrons in front of an orange gear, in colour in the app, the
  favicon and the README wordmark. The sidebar and account-switcher tile is a bordered
  `bg-background` square so the colours read in light and dark.
- The sidebar and header **Documentation** links open the kit's guide.

## 0.1.1 - 2026-10-06

Fixes from walking through the kit as a first-time user, start to finish.

- **`bin/rename`:** `cargo test` passes after a rename (it missed a test that reads the systemd
  unit), `--dry-run` reports exactly what the real run changes, the `SECURITY.md` advisory link
  points at your app, and it tells you to run `bin/setup --reset` if you renamed after setup.
- **Fresh machines:** `cargo test` no longer needs every platform's crates downloaded first.
- **`cargo loco db seed --reset`** no longer fails intermittently with `no such table` (a reset
  now runs on one database connection).
- **Accounts:** a page left open in an account you were removed from goes to your home with
  "That account isn't available" instead of a 404 overlay. The README says up front that
  accounts are built in, and the accounts recipe explains how to flatten them for a
  single-user app.
- **Docs:** rename before the first `bin/setup`; restart `bin/dev` after a scaffold; the Node
  version from `.node-version` with mise; what `bin/ci` needs installed; invited sign-ups join
  the inviter's account. The README shows a controller and the React page it renders.
- **License:** `LICENSE` is plain MIT (GitHub detects it); the Rails kit's license is in
  `NOTICE`.
- **Dependencies:** Inertia 3.8, Vite 8.3.2, Babel 8, lucide-react 1.50, typescript-eslint 8.71
  and patch updates. Dependabot skips updates that can't land yet (crates Loco pins, TypeScript
  7, ESLint 10, Node type majors).
- `bin/e2e-server` honours `CARGO_TARGET_DIR`.

## 0.1.0 - 2026-10-05

First public release: an Inertia.js v3 + React 19 + Loco (Rust) starter kit, ported from the
[Inertia Rails React Starter Kit](https://github.com/inertia-rails/react-starter-kit).

- **Auth:** sign up, sign in, sign out, a sessions list with remote sign-out, email verification,
  password reset, profile / email / password settings and account deletion.
- **Organizations:** a personal account per user, members with roles (`owner`, `admin`, `member`),
  email invitations and an account switcher. Pages live under `/{account_slug}/…`.
- **Live updates:** channels, `broadcast_to`, presence and `perform` over Server-Sent Events.
- **Generators**, account-scoped by default: `generate scaffold`, `generate controller` and
  `generate channel`, plus `bin/rename`.
- **Inertia v3 server adapter:** partial reloads, deferred / merge / once / scroll props, history
  encryption, Precognition, and optional SSR.
- **Deploy targets:** Kamal, Cloudflare Containers, Docker Compose, a single binary under systemd,
  and Fly.io and Render configs.
- **Agent skills:** `AGENTS.md`, the `loco` and `starter-kit` skills in `.claude/skills/`, and
  budget test helpers for props, payload size and query count.
- **Security hardening:** generated channels stream by account and id, open live streams
  re-authorize every 15 s, tokens in invitation and session paths are redacted from logs,
  password checks and verification mail are rate-limited per IP, the deploy workflow only runs
  for a push to this repository's `main`, and the Cloudflare deploy tooling is audited in CI.
