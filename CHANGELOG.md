# Changelog

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
