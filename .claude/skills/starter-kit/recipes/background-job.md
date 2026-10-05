# Recipe: a background job

**When:** work a request must not wait for (export, webhook, thumbnail, slow API). Rails:
`rails g job` + `perform_later`. Generic Loco detail: `.claude/skills/loco/recipes/background-job.md`.

## Commands

```sh
cargo loco generate worker report_export
```

| Writes | |
|---|---|
| `src/workers/report_export.rs` | `Worker` + `WorkerArgs`; fill in `perform` |
| `src/workers/mod.rs` | `pub mod report_export;` |
| `src/app.rs` | `queue.register(...)` in `connect_workers` |
| `tests/workers/report_export.rs` (+ `tests/workers/mod.rs`) | runs it in `ForegroundBlocking` mode |

## This kit's queue

- **SQLite-backed** (`queue.kind: Sqlite`, `QUEUE_URL`, a separate file from the app DB). No Redis.
- **Where it runs:** `bin/dev` starts `--server-and-worker` and the Docker image `--all`, so the
  web process also works the queue. A separate worker: `cargo loco start --worker` on the same host
  and volume (SQLite), e.g. a Kamal `job` role.
- **Tests:** `config/test.yaml` sets `workers.mode: ForegroundBlocking`, so `perform_later`
  runs inline and you can assert on its effect right after.
- The kit's own job is mail delivery, `src/workers/user_mailer_delivery.rs` (enqueued by
  `UserMailer`): enqueue ids, reload in `perform`. Follow it.

## Enqueue

```rust
use crate::workers::report_export::{Worker, WorkerArgs};

Worker::perform_later(&ctx, WorkerArgs { user_id: current.user.id }).await?;
```

Args are persisted as JSON: pass ids, not models. **Never `tokio::spawn`** instead; it's not
durable or retried.

## Operate

```sh
cargo loco jobs retry        # failed -> queued
cargo loco jobs tidy         # delete completed/cancelled
cargo loco jobs dump tmp/    # inspect
```

## Verify

`cargo test --test mod workers::report_export`, then assert on the job's effect (a row written,
`deliveries(&ctx)` for mail) in a request test that triggers it.
