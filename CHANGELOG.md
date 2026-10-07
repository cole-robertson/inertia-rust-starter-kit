# Changelog

## Unreleased

- **The Inertia protocol runs on [inertia-omega](https://github.com/inertiajs/inertia-omega),**
  the Inertia team's Rust adapter (a port of inertia-laravel), instead of the kit's own
  resolver: props and partial reloads, the page object, JSON or the HTML document, asset
  versioning and the redirect rules. Until omega is on crates.io it is a git dependency on a
  fork carrying fixes that are open as pull requests. `src/inertia/` keeps what is the kit's own
  and wires it into omega: Rails-style flash (`Redirect::to(..).notice(..)`) in the encrypted
  `_flash` cookie, CSRF, the CSP nonce, meta tags, precognition, the external-redirect 409,
  the asset-version reload on `app_url`, the HTML document and the SSR client. Controllers
  build props with `.with(..)` instead of `.prop(..)`; `render(..).await` is unchanged.
  What it brings:
  - **Lazy props resolve concurrently.** Sibling lazy and deferred props run together instead
    of one after another; the props and every metadata list are still in prop order.
  - **Redirects to a `#fragment` keep it.** An Inertia request answered with a redirect whose
    `Location` contains `#` gets `409` + `X-Inertia-Redirect`, and the client visits the URL
    itself (fetch drops fragments when it follows a redirect). Prefetches still get the
    redirect. As inertia-laravel does.
  - **Deferred scroll props report `mergeProps: ["users"]` on the first visit,** as
    inertia-rails, inertia-laravel and inertia-omega do; the partial reload that loads them
    reports `users.data`.
  - On the wire, as inertia-laravel: `clearHistory`/`encryptHistory` are sent only when true,
    once props carry `expiresAt: null`, every response has `Vary: X-Inertia`, and the
    asset-version 409 also sends `X-Inertia-Version`.
  - **Deploying:** the `_flash` cookie now holds omega's session keys, so a flash set by the
    previous build (one in flight mid-redirect at deploy time) is dropped on the next request.
    Sessions, CSRF tokens and the database carry over (no migration); rolling back is the same.
  - **Upgrading an app made from the kit:** take `src/inertia/` from this release whole (keep
    your own `config.rs` settings), `inertia::render::layer` replaces the `version` and
    `redirect` layers in `app.rs`, and add the `omega` dependency and `deny.toml` entry. Then
    rename: `Props::prop(k, v)` → `.with(k, v)`; `Prop::serialize(&x)?` → pass `x` (any
    `Serialize`); `.once_key(k)` → `.once_as(k)`; `.expires_in(d)` / `.expires_at(ms)` →
    `.until(d)`; `Props::from_json(v)` → `v.into_props()?` (`IntoProps`); `Prop::array` /
    `lazy_prop` → a `Vec` or a closure returning the value. **Infinite scroll changed shape:**
    `scroll(ScrollMetadata, closure)` is now `scroll(Paginator)` / `scroll_with(..)`, and the
    items always arrive wrapped, `{data: [...]}` with `mergeProps: ["x.data"]` (`.wrapper(..)`
    renames the key). A page that read the prop as a bare array must read `x.data`.
- **Prefetches leave the flash alone.** A request with `Purpose: prefetch` (what `<Link
  prefetch>` sends; also `Sec-Purpose` and `X-Moz`) neither shows, consumes nor writes the
  flash, so hovering a prefetching link no longer eats the "Saved" notice meant for the visit.
  `inertia::redirect::is_prefetch` reads the headers. The kit's own addition; omega,
  inertia-laravel and inertia-rails don't have it.
- **Error pages are readable with compression on.** A handler's 404 (an unknown invitation, a
  missing record) went out as `public/404.html` with the compressed body's `Content-Encoding`
  still set, so browsers couldn't decode it; the exceptions layer now drops it.
- **site/ pins `sharp` to ^0.35.5** too, for the same advisory (it comes in through the
  site's `wrangler`, via `miniflare`).
- **deploy/cloudflare pins `sharp` to ^0.35.5** with an npm override, for GHSA-wq5f-xc86-pv6w
  (high severity; `sharp` comes in through `wrangler`, `miniflare` and `cf`, whose own releases
  still pin 0.35.4). Remove the override once they move.

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
