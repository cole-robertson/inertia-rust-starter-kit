# Recipe: performance notes

**When:** before adding a cache or a crate "for speed", or when a page is slow. Measure first:
`docs/PROFILING.md` has the method (`bench/profile.sh`: app CPU per request, alternating A/B
runs) and `docs/BENCHMARK.md` the numbers against the Rails kit.

## What the kit already does (don't redo it)

| Setting | Where | Why |
|---|---|---|
| mimalloc as the global allocator | `src/bin/main.rs` | +50–66% req/s over glibc malloc (`docs/BENCHMARK.md`) |
| SQLite WAL, `synchronous=NORMAL`, `busy_timeout=5s`, `mmap_size`, `cache_size` on **every** pooled connection | `src/db.rs` | Loco runs its PRAGMAs on one connection only |
| `.first(db)` instead of `.one(db)` for single-row lookups | `crate::db::First` | sea-orm's `.one()` binds `LIMIT ?`, which makes SQLite re-prepare the statement on every call; `.first()` halved signed-in CPU per request (PROFILING finding 2) |
| Session + user in one `JOIN` | `src/models/sessions.rs` | one query per signed-in request |
| Cookie keys derived once | `Settings::cookie_keys` | PROFILING finding 3 |
| Vite manifest and `Settings` parsed once at boot | `shared_store` | no per-request file or YAML reads |
| tokio workers = available CPUs (honours `taskset` / container limits) | `#[tokio::main]` | right as is |

## Rules of thumb for new code

- **Single-row lookups:** `use crate::db::First;` and `.first(db)` (scaffolded models do).
- **One query per page where you can:** `find_also_related` / `find_with_related` instead of a
  loop of finds (the N+1). `.all()` then `iter()` is fine for small tables.
- **Don't clone `AppContext` into long-lived state.** Its `Config` carries the whole `settings:`
  JSON; cloning it cost ~0.9 µs and 42 allocations (PROFILING finding 1). Take what you need
  (`ctx.db.clone()`, `Arc<Settings>`). Loco itself still clones it per handler call (finding 4,
  an upstream issue).
- **Slow prop? `defer(..)` it** (`inertia-page.md`) so the first paint doesn't wait.
- **SQLite writes serialize.** More pool connections don't speed up writes; keep transactions
  short and move bulk work to a job.
- **Log level** in production is `info`; the request span is built even at `error` (finding 7),
  so a quieter level buys little.

## Verify a change

Same method as PROFILING: release build, `LOG_LEVEL=error`, pinned CPUs for the app and the load
generator, 3+ alternating runs, report median and range of req/s *and* CPU per request.
