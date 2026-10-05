# Recipe: file uploads (local disk, then S3/R2)

**When:** avatars, attachments, imports. Rails: Active Storage.

**Not wired in the kit yet.** Loco ships the storage abstraction (`ctx.storage`), but this app's
`AppContext` still has Loco's default **null** driver, which accepts writes and stores nothing.
Step 1 is required before anything else works.

## 1. Configure a store

`src/app.rs`, at the end of `after_context` (it already returns the context from
`configure_sqlite_pool`; wrap that):

```rust
use loco_rs::storage::{self, Storage};

async fn after_context(ctx: AppContext) -> Result<AppContext> {
    crate::inertia::install(&ctx)?;
    crate::db::configure_sqlite_queue(&ctx).await?;
    let ctx = crate::db::configure_sqlite_pool(ctx).await?;
    let store = storage::drivers::local::new_with_prefix("storage/uploads")
        .map_err(Box::from)?;
    Ok(ctx.into_builder().storage(Storage::single(store).into()).build())
}
```

`storage/` is git-ignored and is the Docker volume (`/app/storage`), so uploads survive
redeploys under Kamal. In tests use `storage::drivers::mem::new()`.

S3 / Cloudflare R2: enable the feature (`loco-rs = { …, features = [..., "storage_aws_s3"] }`)
and use `storage::drivers::aws::with_credentials_and_endpoint(bucket, "auto", endpoint,
credentials)` with the R2 endpoint `https://<account>.r2.cloudflarestorage.com`. Read the keys
from `settings:` in `config/production.yaml` (`get_env(name="R2_ACCESS_KEY_ID")`), never
`std::env::var`. Check the exact signatures in `.claude/skills/loco/api-index.md` (`storage::drivers::aws`).

## 2. Accept the upload

`axum::extract::Multipart` is in `loco_rs::prelude`. The frontend's `<Form>` sends
`multipart/form-data` automatically when a field is a file.

```rust
async fn update(Authenticated(current): Authenticated, State(ctx): State<AppContext>,
                mut form: Multipart) -> Result<Response> {
    while let Some(field) = form.next_field().await.map_err(|e| Error::BadRequest(e.to_string()))? {
        if field.name() == Some("avatar") {
            let bytes = field.bytes().await.map_err(|e| Error::BadRequest(e.to_string()))?;
            // validate size and type here; add to Errors and Redirect back on failure
            let key = format!("avatars/{}", uuid::Uuid::new_v4());
            ctx.storage.upload(std::path::Path::new(&key), &bytes).await?;
            // store `key` on the user (a migration adding `avatar_key:string`)
        }
    }
    Ok(Redirect::to(route_table::SETTINGS_PROFILE).notice("Avatar updated").into_response())
}
```

Notes for this kit:

- `Params<T>` doesn't parse multipart; use `Multipart` for upload handlers.
- The request body limit is `server.middlewares.limit_payload.body_limit` (5 MB in test; check
  the other env files) and applies to uploads.
- Never use the client's file name as the storage key.
- CSRF still applies: Inertia's `<Form>` sends the `X-XSRF-TOKEN` header with multipart too.

## 3. Serve it

Private files: a handler that checks ownership, then `ctx.storage.download::<Vec<u8>>(path)` and
returns the bytes with a `Content-Type`. Public files on R2: serve from the bucket's public URL
and store only the key.

## Rails equivalents

`has_one_attached :avatar` → a `*_key` column + `ctx.storage`; `config/storage.yml` → the store
built in `after_context`; direct uploads → presigned URLs from your S3 client (not in Loco).

## Verify

A request test with `server.post(path).multipart(axum_test::multipart::MultipartForm::new()
.add_part("avatar", Part::bytes(b"...".to_vec()).file_name("a.png")))`, with the test context's
store set to `mem`, then `ctx.storage.download::<Vec<u8>>(key)` returns the bytes.
