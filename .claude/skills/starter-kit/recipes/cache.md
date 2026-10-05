# Recipe: cache

**When:** an expensive value read far more often than it changes (dashboard counts, a remote
API response). Rails: `Rails.cache.fetch`. Generic detail and the Redis option:
`.claude/skills/loco/recipes/cache.md`.

**Not configured in the kit.** No `config/*.yaml` has a `cache:` block, so `ctx.cache` is Loco's
**Null** cache: every write is dropped, every read misses, and nothing errors. Add the block
first, to every environment:

```yaml
# config/development.yaml, config/test.yaml, config/production.yaml
cache:
  kind: InMem
  max_capacity: 33554432   # bytes
```

`cache_inmem` is already a feature in `Cargo.toml`. InMem is per process, which matches this
kit's one-container SQLite deploy; with several app hosts, use Redis (`cache_redis`).

## Use

```rust
use std::time::Duration;

let stats: Stats = ctx
    .cache
    .get_or_insert_with_expiry("dashboard:stats", Duration::from_secs(60), async {
        Stats::compute(&ctx.db).await
    })
    .await?;
```

- Invalidate on write: `ctx.cache.remove("dashboard:stats").await?` in the model method that
  changes the data.
- Per-user keys: `format!("user:{}:stats", user.id)`.
- A cached value that feeds a page prop pairs well with `defer(..)` (see `inertia-page.md`), so
  a cold cache doesn't slow the first paint.

## What's already cached without `ctx.cache`

The Vite manifest and `Settings` are loaded once at boot into `shared_store`; the profiling
pass (docs/PROFILING.md) moved cookie key derivation to boot as well. Don't cache those again.

## Verify

With the `cache:` block in `config/test.yaml`, assert the second call doesn't run the closure
(count calls with an `AtomicUsize`), and that `remove` makes the next call recompute.
