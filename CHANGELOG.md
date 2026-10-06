# Changelog

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
