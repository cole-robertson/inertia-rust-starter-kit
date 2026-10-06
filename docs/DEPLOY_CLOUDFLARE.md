# Deploying on Cloudflare Containers

`deploy/cloudflare/` runs the app on Cloudflare Containers: a Worker on your domain forwards
every request to one container running the kit's Docker image. The kit's own public demo,
**https://demo.inertia-rust.dev**, runs this way. Its demo login isn't published; sign up there instead
(the database resets when the container sleeps). Its settings live in a git-ignored
`deploy/cloudflare/.env.local`, not in the repo.

## Settings

`deploy/cloudflare/deploy.sh` reads them from the environment or from
`deploy/cloudflare/.env.local` (git-ignored). Start from the example:

```bash
cp deploy/cloudflare/.env.example deploy/cloudflare/.env.local
```

| Variable | |
|---|---|
| `CF_ACCOUNT_ID` | **required**: the account that owns the zone |
| `CF_DOMAIN` | **required**: the Worker's custom domain, e.g. `app.example.com`; the app's `HOST` is `https://$CF_DOMAIN` |
| `DEMO_ADMIN_EMAIL`, `DEMO_ADMIN_PASSWORD` | optional, both or neither: a login created (or its password reset) at every boot, for a public demo. Unset, no demo user exists. |

A variable set in the environment wins over the file. Without `CF_ACCOUNT_ID` or `CF_DOMAIN` the
script stops before doing anything and says which is missing.

This is a demo setup, not a production one:

- **The disk is ephemeral.** The SQLite database lives on the container's own disk, which is gone
  whenever the container sleeps (after 6 idle hours), restarts or is redeployed. Users who
  sign up, their sessions and their settings all disappear then. A demo admin (if configured) is
  created again at every boot, so that login always works.
- **One instance.** Every request goes to one container (`getByName("app")`, `max_instances: 1`),
  because two instances would mean two separate databases.
- **Cold starts.** The first request after a sleep starts the container. See
  [Cold start](#cold-start) for the measured time.
- **No mail is sent.** There is no SMTP server, so the app runs on Loco's stub mailer: verification
  and password-reset emails are logged (recipient and purpose, never the link) and dropped. A new
  user can sign in but stays unverified. Cloudflare Email Service could send them, but that isn't
  set up.

The rest of this page is the runbook: logs, redeploys and teardown. Names below are the kit's
defaults (`bin/rename` changes them).

## How it fits together

```
browser ── $CF_DOMAIN, e.g. demo.inertia-rust.dev (Worker custom domain)
             └─ Worker `inertia-rust` (deploy/cloudflare/src/index.ts)
                  └─ Durable Object `App`, instance "app"
                       └─ Container: the kit's Docker image (CSR build), port 8080
                            /app/storage/production.sqlite, queue.sqlite (ephemeral)
```

- `deploy/cloudflare/` is a separate npm package (the Worker and `@cloudflare/containers`). It
  doesn't touch the kit's frontend dependencies.
- The Worker sets `X-Forwarded-For` to `CF-Connecting-IP`, because the app reads the client IP
  from the rightmost `X-Forwarded-For` entry (rate limiting, session records).
- The container listens on **8080**, not the image's default 80. Cloudflare's runtime doesn't let
  the image's non-root user bind a port below 1024. (Docker allows that by default, which is why
  the image works locally on 80.) The first deploy on 80 failed with
  `Error: IO(Os { code: 13, kind: PermissionDenied })` in the container log and
  `Failed to start container: The container just exited` in the Worker.
- The container gets its environment from the `App` class's `envVars`:
  - `HOST=https://$CF_DOMAIN` is a Worker text binding (`cloudflare.config.ts`), and so is
    `DEMO_ADMIN_EMAIL` when a demo login is set.
  - `SECRET_KEY_BASE` is a Worker secret, and so is `DEMO_ADMIN_PASSWORD` when a demo login is set.
  - `PORT=8080`, and `DATABASE_URL` / `QUEUE_URL` use the image defaults under `/app/storage`.
- With a demo login, the image's entrypoint (`bin/docker-entrypoint`) sees the `DEMO_ADMIN_*`
  variables at boot, migrates, and runs `task seed:demo` (see the README, "A demo login").
  Without one, neither variable reaches the container and no user is created.
- `MAILER_HOST` is unset, so the app boots without SMTP and logs a warning.

## Logs

The container's stdout (the app's JSON logs) and the Worker's logs go to Workers Observability
(dashboard → Workers → `inertia-rust` → Observability), because `observability` is enabled in
`cloudflare.config.ts`. Instance state: `cf containers applications instances list
--application-id <application-id>` (the id is in `cf containers applications list`).

## Deploy and redeploy

You need Docker, Node 22.18 or newer with npm, the settings above, and the `cf` CLI logged in to
the account that owns your domain's zone (`cf auth whoami`; if the token has expired, run
`cf auth login --no-browser`).

```bash
deploy/cloudflare/deploy.sh --dry-run   # write the Worker/Container config only; see below
deploy/cloudflare/deploy.sh
```

`--dry-run` checks the settings and runs `cf build`, which writes the config `cf deploy` would
upload to `deploy/cloudflare/.cloudflare/output/v0/` (`workers/default/worker.config.json`:
domain, `HOST`, bindings; `containers/*/container.config.json`: the image reference). It builds
no image, needs no login, and deploys nothing.

The script:

1. Builds the image from `git archive HEAD` (committed code only) under
   `~/.cache/inertia-rust-deploy`, with `docker build --cpuset-cpus 0-3`, and pushes it with
   `cf containers push` as `inertia-rust:<short sha>`.
2. Keeps `SECRET_KEY_BASE` in `deploy/cloudflare/.secrets.env` (git-ignored, mode 600). It is
   generated with `bin/secret` on the first run and reused after that, so a redeploy doesn't
   invalidate cookies. (The database resets on a redeploy anyway.)
3. Runs `cf deploy --secrets-file … --containers-rollout immediate` in `deploy/cloudflare/`, with
   `IMAGE_REF` set to the pushed image. `cloudflare.config.ts` (the `cf` CLI's config) reads it.
   The secrets are uploaded with the Worker version from a temporary mode-600 file that is removed
   afterwards, and they never touch git.

To redeploy, commit your changes and run the script again.

### Changing a secret

To change the demo password, edit `DEMO_ADMIN_PASSWORD` in `deploy/cloudflare/.env.local` (or set
it in the environment) and redeploy. For a new `SECRET_KEY_BASE`, delete
`deploy/cloudflare/.secrets.env` first. It signs everyone out. Removing the demo login from
`.env.local` removes its bindings on the next deploy; the user is gone once the container
restarts on a fresh disk.

## Tear down

These are all the Cloudflare resources a deploy creates:

| Resource | Name |
|---|---|
| Worker (script) | `inertia-rust` |
| Durable Object namespace (class `App`, SQLite) | created with the Worker |
| Containers application | `inertia-rust`, `basic`, max 1 instance |
| Images in the Cloudflare registry | `registry.cloudflare.com/<account-id>/inertia-rust:<tag>`, one tag per deploy |
| Worker custom domain | `$CF_DOMAIN` → `inertia-rust` |
| DNS record (created by the custom domain) | `AAAA $CF_DOMAIN 100::`, proxied |
| Worker secrets | `SECRET_KEY_BASE`, and `DEMO_ADMIN_PASSWORD` with a demo login |

Nothing else on the account or the zone is touched.

To remove them:

```bash
cf workers delete inertia-rust                   # Worker, Durable Object namespace, custom domain and its DNS record
cf containers applications list                  # confirm the application is gone; if not:
cf containers applications delete <application-id>
cf containers images list                        # then, for each inertia-rust tag:
cf containers images delete inertia-rust:<tag>
rm deploy/cloudflare/.secrets.env
```

Then check that `cf dns records list -z <zone> --name $CF_DOMAIN` returns `[]`.

## Cold start

Measured from a client in California, 2026-09-29, `basic` instance. `sleepAfter` was set to 2 minutes
temporarily so the container slept between probes. Each probe runs `GET /sign_in`, then the demo
admin's sign-in `POST`, then warm requests.

| | Time |
|---|---:|
| First request after the container slept (`GET /sign_in`) | **0.97 s** (0.966, 0.968, 0.973) |
| The sign-in `POST` right after it (argon2id verify + session insert) | 0.16–0.17 s |
| Warm `GET /up` / signed-in `GET /dashboard` | 0.11–0.12 s |
| First request after a **deploy** (new image version rolling out) | 2.3–4.5 s, and requests can take 2–15 s for about a minute while the rollout replaces the instance |

Where the ~0.97 s goes: the warm round trip is ~0.11 s, most of it network and the Worker → Durable
Object → container hop. The app accounts for about 0.2 s: migrate, `seed:demo` (one argon2 hash)
and boot, all in the same second in the container log (`migrate:` → `Starting background job
processing` in 0.15–0.2 s), and the same image serves `/up` 199 ms after `docker run` locally. That
leaves **~0.65 s for Cloudflare to start the container**, which the app can't reduce. The sign-in
POST costs ~60 ms more than a warm GET: that is argon2 on 1/4 vCPU, so `basic` is fine (`lite`,
1/16 vCPU, would be about 4× slower).

What was slow before: the first sign-in that was reported slow landed right after the last deploy, and the Worker log
shows requests of 8–15 s at that time. That's the rollout replacing the instance, not the
app. A plain wake from sleep is under a second.

Changes made for this:

- `sleepAfter` went from 30 minutes to **6 hours** (`deploy/cloudflare/src/index.ts`). Awake, a
  `basic` instance bills 1 GiB of memory and 4 GB of disk continuously (about $0.01/hour, roughly
  $0.06 for a full 6-hour idle tail at list price, within the plan's included 25 GiB-hours a month
  for occasional use). CPU is billed only when used, and nothing is billed while asleep. A longer
  sleep also means the demo database survives longer.
- Migrations and `seed:demo` stay at boot. Together they take ~0.2 s, about a fifth of a cold
  start, and the demo login has to exist before the first request can sign in.
- **Keep-warm (not enabled):** a Cron Trigger on the Worker that fetches `/up` every few minutes
  would keep the container awake for good, at roughly $7/month of memory and disk beyond the
  included amount. For a demo, the 6-hour sleep is the cheaper compromise.
