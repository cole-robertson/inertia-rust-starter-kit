# Building your SaaS on this kit

The standard path from a clone of this kit to your own app in production. Each step is a
command you run and a file you edit; each links the recipe in
[`.claude/skills/starter-kit/`](../.claude/skills/starter-kit/SKILL.md) that has the details, so
you (or your coding agent) can go deeper on any one of them.

The kit stays small on purpose: auth, settings, the Inertia adapter, the Rails kit's features,
and organizations (accounts, members, invitations), because nearly every SaaS needs them and
adding them later is the painful part. Billing and an admin area are not in it; step 9 says where
they go.

If you know Rails, keep [RAILS_TO_LOCO.md](RAILS_TO_LOCO.md) open alongside this.

## 0. Prerequisites

- Rust via [rustup](https://rustup.rs). `rust-toolchain.toml` pins the version, and rustup installs
  it on the first `cargo` command.
- Node, the version in `.node-version` (22). Any version manager works; with mise:
  `mise use node@22` in the project, or `mise exec node@22 -- bin/setup`.
- For the generators only: `cargo install --locked sea-orm-cli@2.0.4`. `cargo loco generate
  model|scaffold` runs it to write `src/models/_entities/`; `bin/setup` tells you if it's missing.
- For `bin/ci` only: `cargo install --locked cargo-deny` (a few minutes to compile), sea-orm-cli
  (above: `bin/ci` runs the real generators in a copy of the app), and
  `npx playwright install chromium` once, after `bin/setup` has installed the npm packages.

## 1. Clone and rename

```sh
git clone https://github.com/cole-robertson/inertia-rust-starter-kit.git acme
cd acme
bin/rename --dry-run acme_crm "Acme CRM"   # see what changes
bin/rename acme_crm "Acme CRM"
git remote set-url origin git@github.com:you/acme.git
```

`bin/rename` changes the crate and binary name (`acme_crm-cli`), the SQLite file names, the Kamal
service and image, the Cloudflare Worker and Container names (`acme-crm`), the display name in
`config/*.yaml`, the page titles and mail, and the README title. It leaves `docs/`, `bench/`,
links to the kit's repository and the README's credits alone. Then it runs `cargo fmt` and
Prettier, because a different name changes import order and line wrapping (it runs `npm ci`
first if `node_modules` is missing). It's safe to run again. `tests/rename.rs`
covers it, and a renamed copy builds and passes the full suite.

## 2. Set up and run

```sh
bin/setup      # npm ci, cargo build, migrate, seed, then bin/dev
```

Open http://localhost:5150 and sign in as `one@example.com` / `Secret1*3*5*`. You land in the
seeded account Acme at `/acme`; the sidebar's switcher also shows any other account you're in
(sign in as `two@example.com` to see Acme and Globex). On a fresh machine
the first `bin/setup` takes about 1 to 3 minutes, depending on the machine (the Rust build); after
that `bin/dev` starts in seconds.

Rename (step 1) before this first `bin/setup`. The development database is named after the app,
so after a later rename the app starts on a new, empty database, `bin/setup` leaves it unseeded
(it only seeds a database it creates), and the seeded logins don't work. If that happened, run
`bin/setup --reset`: it drops and re-seeds the new database.

## 3. Branding

| What | Where |
|---|---|
| App name | `settings.app_name` in `config/{development,test,production}.yaml` (server-rendered titles, mail) and the `"…"` fallback after `VITE_APP_NAME` in the frontend (`frontend/entrypoints/app.ts` and the logo/layout components). `bin/rename` sets both; set `VITE_APP_NAME` at build time to override the frontend one. |
| Logo | `frontend/components/app-logo-icon.tsx`: the kit's mark in the app, a gear moving right with two motion streaks. It's a one-colour inline SVG in `currentColor`, so it follows the theme's text colour in light and dark like the rest of the shadcn/ui components; callers set the size and colour (`className="text-foreground size-6"`), and the sidebar and account-switcher tile in `app-logo.tsx` and `account-switcher.tsx` is `bg-sidebar-primary text-sidebar-primary-foreground`. It's used by the sidebar, the header, the auth pages and the home page. To replace it, swap the `<svg>` body for your own (keep the component name and `{...props}`). Outside the app the kit keeps its colours (orange `#CE422B`, purple `#7C5CFC`): `public/icon.svg` and `public/icon.png` (512×512, linked from `src/inertia/document.rs` as the favicon and apple-touch icon) are the colour version, an orange gear with two purple bars on a dark tile, so it holds up at 16 px in a browser tab: replace both. `docs/logo/wordmark.svg` is the README's colour "Inertia Rust" wordmark, text outlined in Instrument Sans; `bin/rename` with a display name drops it from the top of the README along with the old title. |
| Colors | the CSS variables in `frontend/entrypoints/application.css` (`--primary`, `--sidebar-primary`, …), light and dark. They are shadcn/ui tokens; the [shadcn themes](https://ui.shadcn.com/themes) page generates them. |
| Home page | `frontend/pages/home/index.tsx` |
| Mail from | `settings.mail_from` in `config/*.yaml` (`MAIL_FROM` in production) |

## 4. Accounts: who owns the data

Every user belongs to one or more **accounts** (Basecamp's name for an organization), with a role
in each: `owner`, `admin` or `member`. Sign-up creates a personal account ("Ann's account") with
the user as owner; an invitation link instead joins the inviter's account. Signed-in pages that
hold data live under `/{account_slug}/…`, and the `CurrentAccount` extractor answers **404** to
anyone who isn't a member, so a URL never reveals that an account exists.

What you get: the account overview (`/{slug}`, the dashboard), settings, members (change roles,
remove, leave; an account always keeps an owner), invitations by email (send, revoke, accept,
sign up through the link), and the account switcher. `/dashboard` redirects to the last-used
account.

Your own resources belong to an account the same way: `account:references` on the table, routes
under `/{account_slug}`, `CurrentAccount` in the controller, and every query scoped to
`current.account.id`. Recipe: [accounts.md](../.claude/skills/starter-kit/recipes/accounts.md)
("add a resource to an account", "check a role").

**Calling them something else.** The code says `Account`/`Membership`/`Invitation` (37signals'
names). Most apps keep those and change only the words users see ("workspace", "team") in the
pages, flash messages and invitation mail; renaming the tables, routes and types is possible
too. Both are described in the recipe's "Renaming" section.

## 5. Add your first resource

```sh
cargo loco generate scaffold projects name:string! description:text due_on:date archived:bool!
cargo loco task scaffold:pages resource:projects
cargo test
```

That's Rails' `rails g scaffold`, in two commands. The first writes the Rust half from the kit's
templates in `.loco-templates/`: migration, entity, a model with params, casting and Rails-worded
validation (`src/models/projects.rs`), an Inertia controller (`src/controllers/projects.rs`),
paths and routes in `src/route_table.rs`, a model test and a request test. The second writes the
React pages (`frontend/pages/projects/`, shadcn/ui forms), adds a sidebar link, and regenerates
`frontend/routes/`. Restart `bin/dev` (new routes need a rebuild; Rust has no autoloading), then
open http://localhost:5150/acme/projects.

**The resource belongs to the account.** The kit adds `account:references` to the scaffold, so
projects live at `/{account_slug}/projects`, the controller takes `CurrentAccount`, every query
is scoped to the account, and the generated request test proves that another account's project
is a 404. For something outside accounts (a global list of countries, say), add `--global`.
Generating several resources? Run all the `generate` commands first, then the `scaffold:pages`
tasks: each `generate` rebuilds the CLI.

Column types the forms support: `string`, `text`, `int`, `big_int`, `small_int`, `float`,
`double`, `bool`, `date`, and `references`. Bare types are nullable; `!` is required. Other
columns (an `archived_at:tstz`, a `creator:references` with no `creators` table) are left out of
the form and the props with a note; set them in the model. The generated code is yours to edit
from here: add validations to `<Singular>Params::assign`, change the pages.

Controllers that aren't CRUD come from `cargo loco generate controller`: pages
(`reports summary`), write actions that redirect back (`notes create update destroy`), and
nested ones like Rails' `Todos::CompletionsController` (`todos/completions create destroy`, a
`/{account_slug}/todos/{todo_id}/completion` resource). Recipe:
[inertia-page.md](../.claude/skills/starter-kit/recipes/inertia-page.md).

A `references` column works like Rails' `belongs_to`. For example, a second resource that
belongs to a project:

```sh
cargo loco generate scaffold tasks title:string! project:references done:bool!
cargo loco task scaffold:pages resource:tasks
```

- The form picks the project from a shadcn/ui Select of the account's projects, labelled by its
  `name` column, else `title`, else `#id` (`Task::project_options` in `src/models/tasks.rs`;
  change the label there). Another account's project id is "must exist".
  `project:references` is a required select; `project:references?` adds a "None" choice.
- On save, `TaskParams` checks that the project exists. A missing or blank one is Rails'
  `project: ["must exist"]` on the form, not a 500.
- The generated request test makes its own project rows and checks the "must exist" error, so
  `cargo test` passes as generated.
- A `user:references` column is checked the same way, but gets no select (the owner comes from
  the signed-in user; see the recipe). Deleting a project deletes its tasks (the foreign key
  cascades).

`cargo test --test controller_generator -- --ignored` (in `bin/ci` and CI) runs the real
generators in a copy of the kit (an account-scoped and a `--global` scaffold, a page controller,
a write controller and a nested one) and then clippy, their request tests, `tsc` and ESLint.

Recipe: [new-resource.md](../.claude/skills/starter-kit/recipes/new-resource.md).

## 6. Background jobs, scheduled tasks, mail

```sh
cargo loco generate worker report_export     # src/workers/report_export.rs, registered, with a test
cargo loco generate task cleanup_projects    # src/tasks/cleanup_projects.rs, registered, with a test
cargo loco generate mailer project_mailer    # src/mailers/project_mailer.rs + Tera templates, sent from settings.mail_from
```

- **Jobs** run on Loco's SQLite queue (`QUEUE_URL`), no Redis. `bin/dev` (`--server-and-worker`)
  and the Docker image (`--all`) run the worker in the web process. Enqueue with
  `Worker::perform_later(&ctx, args)`. [background-job.md](../.claude/skills/starter-kit/recipes/background-job.md)
- **Scheduled tasks** are a task plus an entry under `scheduler:` in `config/<env>.yaml`, run by
  `cargo loco start --all` (or a `cargo loco scheduler` process). The Docker image starts
  `--all`; with no jobs the scheduler isn't started, and a mode that won't run configured jobs
  logs a warning at boot.
  `cargo loco generate scheduler` writes a separate `config/scheduler.yaml` that is only read with
  `--config`. [scheduled-task.md](../.claude/skills/starter-kit/recipes/scheduled-task.md)
- **Mail** follows `src/mailers/user_mailer.rs`: a worker renders and sends, so a request never
  waits on SMTP. Development sends to Mailpit on `localhost:1025`; tests assert on
  `ctx.mailer.deliveries()`. [mailer.md](../.claude/skills/starter-kit/recipes/mailer.md)

## 7. Deploy

Both paths use the same Docker image (`Dockerfile`: Vite assets, cargo-chef, a slim runtime as a
non-root user). SQLite lives in `/app/storage`; migrations run on boot.

**Kamal, on your own server** (the default, like the Rails kit):

1. `config/deploy.yml`: `servers`, `image` and `registry` (e.g. `your-user/acme_crm` on
   `ghcr.io`), `proxy.ssl` and `proxy.host`, `HOST`, `MAILER_*`, and the build cache
   `builder.cache.image` (`your-user/acme-crm-build-cache`).
2. Secrets via `.kamal/secrets`: `SECRET_KEY_BASE` (`bin/secret`), `KAMAL_REGISTRY_PASSWORD`,
   `MAILER_PASSWORD`.
3. `kamal setup` once, then `kamal deploy`. Details in the README's
   [Deploy](../README.md#deploy) section, "Kamal, on your own server".

**Cloudflare Containers** (what runs the kit's demo, https://rust.rebulk.com):
`deploy/cloudflare/` is a Worker that forwards to one container running the image. Its
settings are not in the repo:

```sh
cp deploy/cloudflare/.env.example deploy/cloudflare/.env.local   # git-ignored
```

- `CF_ACCOUNT_ID` and `CF_DOMAIN` (required): your account and the Worker's custom domain;
  `HOST` is `https://$CF_DOMAIN`. The script stops and names them if either is missing.
- `DEMO_ADMIN_EMAIL` and `DEMO_ADMIN_PASSWORD` (optional, both or neither): a login created at
  every boot, for a public demo. Leave them unset for a real app; then no demo user exists.
- The Worker and Container names come from `bin/rename`.

`deploy/cloudflare/deploy.sh --dry-run` writes the Worker/Container config without building or
deploying anything; check the domain and bindings in
`deploy/cloudflare/.cloudflare/output/v0/workers/default/worker.config.json`.

Then `deploy/cloudflare/deploy.sh`. The container's disk is **ephemeral**: the database is lost
when it sleeps or redeploys. That's fine for a demo and wrong for real users until the database
lives somewhere durable. [docs/DEPLOY_CLOUDFLARE.md](DEPLOY_CLOUDFLARE.md) has the settings,
how it fits together, and the kit demo's runbook (its resource ids and teardown); the steps apply
to yours with your names.

Recipe: [deploy.md](../.claude/skills/starter-kit/recipes/deploy.md).

## 8. Keep it green

```sh
bin/ci      # fmt, clippy, eslint, prettier, tsc, fresh routes, cargo deny, npm audit, tests, builds, Playwright
```

It needs `cargo-deny`, sea-orm-cli and Playwright's Chromium (section 0). A fresh clone with two
generated resources passes in about 5 minutes on a fast machine, most of it in
`Tests: generated code builds` (it generates code in a copy of the app and builds and tests that
copy).
`cargo deny` also fails when a crate in `Cargo.lock` is yanked upstream, even with no code
change; `cargo update -p <crate>` (it names the crate) fixes that.

GitHub Actions runs the same steps on every push.

## 9. Where billing and admin go

Billing and an admin area are not in the kit, and nothing below marked a sketch exists as code
here. The recipes say how to build them the Rails way on this codebase:

| Feature | Recipe |
|---|---|
| Accounts, memberships, invitations, scoping every query to the current account (**in the kit**) | [accounts.md](../.claude/skills/starter-kit/recipes/accounts.md) |
| Billing with Stripe Checkout and webhooks (sketch) | [billing.md](../.claude/skills/starter-kit/recipes/billing.md) |
| An admin area (sketch) | [admin.md](../.claude/skills/starter-kit/recipes/admin.md) |
| File uploads (local disk, then S3/R2) | [file-uploads.md](../.claude/skills/starter-kit/recipes/file-uploads.md) |
| Live updates: channels, presence (**in the kit**, `cargo loco generate channel`) | [live-updates.md](../.claude/skills/starter-kit/recipes/live-updates.md) |

A second, larger example app built on this kit, with organizations, generated CRUD, scheduled
jobs, uploads, caching and live updates, is **coming**; this guide will link it when it exists.
