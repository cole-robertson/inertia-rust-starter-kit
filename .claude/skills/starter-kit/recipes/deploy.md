# Recipe: deploy

**When:** shipping to production or a demo. Rails: Kamal 2 (the Rails kit's default).

Both options run the same image, built by the `Dockerfile` (Vite assets → cargo-chef → a slim
Debian runtime as uid 1000 under `tini`). The binary migrates on boot; SQLite and the job queue
live in `/app/storage`. Health check: `GET /up`. The Rust build needs about 4 CPUs and a few
minutes cold (`docker build --cpuset-cpus 0-3 .` on a shared machine).

## Kamal (your own server; the default)

| Step | File / command |
|---|---|
| servers, registry, domain | `config/deploy.yml` (`servers`, `registry`, `proxy.host`, `env.clear.HOST`) |
| mail | `MAILER_HOST`, `MAILER_USER` in `config/deploy.yml`; `MAILER_PASSWORD` in `.kamal/secrets` |
| secrets | `.kamal/secrets`: `SECRET_KEY_BASE` (`bin/secret`), `KAMAL_REGISTRY_PASSWORD`, `MAILER_PASSWORD` |
| first deploy | `kamal setup` |
| every deploy | `kamal deploy` (or push to `main` with `.github/workflows/deploy.yml` enabled) |
| operate | `kamal logs`, `kamal shell`, `kamal migrate`, `kamal dbstatus` (aliases in `config/deploy.yml`) |

One web container per SQLite database. A `job` role must share the host and the
`/app/storage` volume. The image starts `--all`, so `scheduler:` jobs run in the web
container (`scheduled-task.md`).

## Cloudflare Containers

`deploy/cloudflare/` is a Worker that forwards every request to one container running the image
(`cloudflare.config.ts`, `src/index.ts`); `deploy/cloudflare/deploy.sh` builds the image from
committed code, pushes it, and deploys. Settings come from the environment or the git-ignored
`deploy/cloudflare/.env.local` (copy `deploy/cloudflare/.env.example`): `CF_ACCOUNT_ID` and
`CF_DOMAIN` are required (the script stops and names a missing one; `HOST` is
`https://$CF_DOMAIN`), and `DEMO_ADMIN_EMAIL` + `DEMO_ADMIN_PASSWORD` (both or neither) seed a
known login at every boot for a public demo; leave them unset for a real app. `bin/rename`
updates the Worker/Container names. `deploy.sh --dry-run` runs only `cf build`, writing the
config to `deploy/cloudflare/.cloudflare/output/v0/` without an image, a login or a deploy.
Needs Docker, Node 22.18+, and the `cf` CLI logged in.

**The container disk is ephemeral**: the database resets whenever the container sleeps
(`sleepAfter`), restarts or redeploys. Fine for a demo (with `DEMO_ADMIN_*` set it re-seeds the
demo login at boot); for real data, move the database to durable storage first. Full setup, cold
start numbers and teardown: `docs/DEPLOY_CLOUDFLARE.md`.

## Docker Compose, systemd, Fly.io, Render

`deploy/compose/` (Compose + optional Caddy), `deploy/systemd/` (the release binary as a
service, no container), `deploy/fly/fly.toml` and `deploy/render/render.yaml`. Steps and what
each needs: `docs/DEPLOY.md`. Compose and systemd are verified end to end; Fly and Render are
config only. All of them: one instance, a persistent `/app/storage` (or the unit's
`StateDirectory`), `SECRET_KEY_BASE`, `HOST`, `MAILER_*`. `bin/rename` renames their service
names and the unit file.

## More than one hostname

The app answers on `app_url`'s host. To serve the same app on more hostnames (say a second
custom domain, each with its own sessions, since cookies are host-only), set `EXTRA_HOSTS` to
them, comma-separated (`settings.extra_hosts`, empty by default). A form post to a listed host
passes the CSRF Origin check only when its `Origin` is that same host, never another listed one
or `app_url`'s; an Inertia version reload stays on the host it came from; mail links keep
`app_url`. Point each hostname at the app (a kamal-proxy `hosts:` entry, a Cloudflare custom
domain) yourself. `tests/inertia_b.rs` covers the rule.

## Rails equivalents

`config/deploy.yml` and `.kamal/secrets` are the same files Kamal uses for Rails; `bin/rails
db:prepare` on boot → `bin/docker-entrypoint` runs `db migrate`; `RAILS_MASTER_KEY` →
`SECRET_KEY_BASE` plus individual env vars (no encrypted credentials file).

## Verify

```sh
docker build --cpuset-cpus 0-3 -t myapp .
# Production refuses a plain-http HOST; ALLOW_INSECURE_HTTP=true is for this local check only.
docker run --rm -p 8080:80 -e SECRET_KEY_BASE=$(bin/secret) \
  -e HOST=http://localhost:8080 -e ALLOW_INSECURE_HTTP=true myapp
curl -fsS localhost:8080/up
```

After a real deploy: `curl -fsS https://your.domain/up`, sign up, and check `kamal logs` for the
mail warning if SMTP isn't configured.
