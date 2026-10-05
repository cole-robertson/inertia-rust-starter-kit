# Recipe: a scheduled (recurring) task

**When:** "every night at 3", "hourly". Rails: a rake task + `whenever`, or Solid Queue
recurring tasks. Generic detail: `.claude/skills/loco/recipes/task-and-schedule.md`.

Recurring work is always two pieces: a **task** that does the work, and a **scheduler entry**
that runs it by name.

## Commands

```sh
cargo loco generate task prune_sessions
```

Writes `src/tasks/prune_sessions.rs` (fill in `run`), registers it in `App::register_tasks`
(above the `// tasks-inject` comment in `src/app.rs`), and adds `tests/tasks/prune_sessions.rs`.

Run it by hand: `cargo loco task prune_sessions days:30` (args are `key:value`, read with
`vars.cli_arg("days")`). In production: `/app/<app>-cli task prune_sessions days:30`.

## Schedule it

Add to `config/<env>.yaml` (each environment that should run it):

```yaml
scheduler:
  output: stdout
  jobs:
    prune_sessions:
      run: "prune_sessions days:30"
      schedule: "0 0 3 * * *"      # sec min hour day month weekday, UTC
```

`cargo loco generate scheduler` writes a separate `config/scheduler.yaml` instead; that file is
read **only** with `cargo loco scheduler --config config/scheduler.yaml`. Plain `--list` then
reports `Scheduler(Empty)`. Prefer the env-config block above.

## Where it runs

- Development: `bin/dev --all` (server + worker + scheduler). Plain `bin/dev`
  (`--server-and-worker`) does not run it, and says so at boot: `WARN scheduler:
  prune_sessions configured under `scheduler:` but this start mode does not run the scheduler`.
- Production: the image (and `deploy/systemd`) starts `--all`, so the jobs run in the web
  process. With no jobs configured the scheduler isn't started at all. If you split roles, run
  the scheduler exactly once (e.g. the web role `start --scheduler`, a `job` role
  `start --worker`); with SQLite, keep them on the same host and volume.

## Verify

```sh
cargo loco task                       # listed
cargo loco scheduler --list           # the entry shows with its schedule
cargo test --test mod tasks::prune_sessions
```
