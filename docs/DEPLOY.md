# Deploying

Every target runs the same app: one Rust binary that serves the web app and processes the job
queue, with SQLite for both. It migrates on boot and answers `GET /up` for health checks. Kamal
and Cloudflare Containers are covered in the README; this page adds the others.

| Target | Files | Status |
|---|---|---|
| [Docker Compose](#docker-compose) | `deploy/compose/` | verified end to end (2026-10-04) |
| [Single binary + systemd](#single-binary--systemd) | `deploy/systemd/` | verified end to end (2026-10-04) |
| [Fly.io](#flyio) | `deploy/fly/fly.toml` | config provided, not deployed by us |
| [Render](#render) | `deploy/render/render.yaml` | config provided, not deployed by us |

"Verified end to end" means it was run on a Linux server: boot, `/up`, sign up, sign in, then a
restart and sign in again with the same account (the data survived). For Fly and Render, the
files pass those platforms' published config schemas and the image answers on the ports they
use, but nobody has deployed them; treat them as a starting point.

## What every target needs

- **Exactly one instance.** SQLite is a file: two instances means two databases. Don't scale
  out, and turn off any "high availability" or standby machine.
- **A persistent disk** for the database and the job queue: `/app/storage` in the image, the
  `StateDirectory` with systemd. Without one, every restart or redeploy starts empty.
- **`SECRET_KEY_BASE`**: at least 64 characters; `bin/secret` prints one. Keep it stable:
  changing it signs everyone out.
- **`HOST`**: the public `https://` URL. Mail links and the CSRF origin check use it, and
  production refuses to boot on an `http://` one.
- **Mail**: `MAILER_HOST`, `MAILER_USER`, `MAILER_PASSWORD` (and `MAILER_PORT`, default 587;
  `MAIL_FROM`). Without `MAILER_HOST` the app runs but sends no mail, so sign-up can't verify
  addresses, password reset doesn't work, and invitations aren't delivered.
- **One proxy hop in front.** The app takes the client IP (for rate limits and the session list)
  from the rightmost `X-Forwarded-For` entry, so the app port should be reachable only through
  that proxy. All four setups below do this. See `remote_ip` in `config/production.yaml`.

All settings: the comments at the top of `config/production.yaml`.

## Docker Compose

Any server with Docker. The image builds on the server from the repository; SQLite lives on a
named volume; [Caddy](https://caddyserver.com) is optional for HTTPS.

```sh
cp deploy/compose/.env.example deploy/compose/.env    # SECRET_KEY_BASE (bin/secret), HOST, MAILER_*, DOMAIN
docker compose -f deploy/compose/compose.yaml --profile tls up -d --build
curl -fsS https://app.example.com/up
```

- `--profile tls` adds Caddy on ports 80 and 443, with a Let's Encrypt certificate for `DOMAIN`.
  Point the domain's DNS at the server first.
- Without `--profile tls`, the app is published only on `127.0.0.1:8080` (`APP_BIND`,
  `APP_PORT`), for a proxy you already run.
- Upgrade: `git pull`, then the same `up -d --build`. The volume `storage` keeps the data;
  `docker compose down -v` deletes it.
- Console: `docker compose -f deploy/compose/compose.yaml exec app sqlite3 /app/storage/production.sqlite`.

## Single binary + systemd

No container: build once, copy one binary and two folders, and let systemd supervise it. The
binary is ~50 MB (x86_64 release build).

```sh
# On a build machine (same CPU architecture and glibc as the server, or build on the server):
npm ci && npx vite build && cargo build --release --locked

# On the server:
sudo install -d /opt/inertia-rust-starter-kit /etc/inertia-rust-starter-kit
sudo install -m 755 target/release/inertia_rust_starter_kit-cli /opt/inertia-rust-starter-kit/
sudo cp -r config public /opt/inertia-rust-starter-kit/
sudo install -m 600 deploy/systemd/env.example /etc/inertia-rust-starter-kit/env   # then edit it
sudo cp deploy/systemd/inertia-rust-starter-kit.service /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable --now inertia-rust-starter-kit
curl -fsS 127.0.0.1:3000/up
```

What the unit does:

- **Storage**: `StateDirectory=` gives the service `/var/lib/inertia-rust-starter-kit`, owned by
  a system user that systemd allocates (`DynamicUser=yes`). The database and queue live there
  and survive restarts, upgrades and reboots. Everything else is read-only to the app.
- **Env file**: `/etc/inertia-rust-starter-kit/env` holds `SECRET_KEY_BASE`, `HOST` and
  `MAILER_*`; the unit sets the rest (`LOCO_ENV`, port, database paths).
- **Restarts**: `Restart=always`, so a crash or `kill -9` brings it back in 2 seconds.
- **Logs**: JSON lines in the journal, `journalctl -u inertia-rust-starter-kit -f`.

It listens on `127.0.0.1:3000` only. Put a reverse proxy in front for HTTPS:
`deploy/systemd/Caddyfile` is the whole Caddy config (install Caddy from your distribution, copy
it to `/etc/caddy/Caddyfile` with your domain, reload). With nginx, `proxy_pass
http://127.0.0.1:3000` and set `X-Forwarded-For` to `$remote_addr` (the client, as the one
trusted hop). The unit was verified on loopback without a proxy; Caddy in front was verified
with the Compose setup, whose Caddyfile is the same apart from the upstream address.

Upgrade: copy the new binary, `config/` and `public/` over the old ones, then
`systemctl restart inertia-rust-starter-kit`. Migrations run on boot.

SSR is off here (it needs Node on the server); see the README's Deploy section to turn it on.

## Fly.io

**Config provided, not deployed by us.** `deploy/fly/fly.toml` passes Fly's published config
schema; `flyctl config validate` needs a Fly login, so it wasn't run.

```sh
cp deploy/fly/fly.toml fly.toml            # Fly reads it from the repository root; set `app`
fly apps create <app>
fly volumes create storage --size 1 --region iad
fly secrets set SECRET_KEY_BASE=$(bin/secret) HOST=https://<app>.fly.dev
fly deploy --ha=false
```

- **One Machine.** `--ha=false` stops the first deploy from creating a standby; never
  `fly scale count 2`. Each Machine gets its own volume, so a second Machine is a second,
  separate database.
- The volume `storage` is mounted at `/app/storage`. The image runs as uid 1000; Fly says it
  gives the mount to the image's `USER`. If the app logs a permission error on the database,
  `fly ssh console -C "ls -ld /app/storage"` shows the owner.
- `auto_stop_machines` stops the Machine when idle and starts it on the next request; the data
  stays on the volume. Set `min_machines_running = 1` to skip the cold start.
- Mail: `fly secrets set MAILER_HOST=… MAILER_USER=… MAILER_PASSWORD=…`.

## Render

**Config provided, not deployed by us.** `deploy/render/render.yaml` passes Render's published
Blueprint schema.

```sh
cp deploy/render/render.yaml render.yaml   # Render reads it from the repository root
git add render.yaml && git commit -m "Render Blueprint" && git push
# Render dashboard: New > Blueprint, pick the repository, fill in SECRET_KEY_BASE, HOST, MAILER_*
```

- A persistent disk needs a paid instance type (`plan: starter` here), and a service with a disk
  always runs exactly one instance: no scaling and no zero-downtime deploys, which is what
  SQLite needs anyway.
- The disk is mounted at `/app/storage`.

Railway isn't covered: it would be the same image with a volume on `/app/storage`.
