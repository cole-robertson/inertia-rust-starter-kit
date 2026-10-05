# src/inertia — module contract (fixed by the lead; agents implement against it)

Settings are already in `config/*.yaml` under `settings:` (commit "Config: drop JWT…").

```
src/inertia/
  mod.rs          (A) pub use of everything below; `pub fn layers(router, &AppContext)`? — no: see app.rs wiring
  config.rs       (A) Settings { secret_key_base, app_url, app_name, mail_from, encrypt_history,
                      forgery_protection, vite: ViteSettings, ssr: SsrSettings }
                      Settings::from_ctx(&AppContext) -> Result<Arc<Settings>>; validates secret len >= 64 in production
                      (refuse boot). Stored in ctx.shared_store in Hooks::after_context.
  props.rs        (A) Prop kinds + Props builder
  resolver.rs     (A) partial reload / metadata resolution (port of props_resolver.rb)
  page.rs         (A) Page struct (serde, camelCase keys per protocol)
  vite.rs         (A) dev/prod tags + asset version
  document.rs     (A) HTML root document builder (takes nonce: &str, ssr: Option<SsrOutput>)
  ssr.rs          (A) SSR HTTP client (+ SsrOutput {head: Vec<String>, body: String}) and supervisor Initializer
  render.rs       (A) `Inertia` extractor + `Inertia::render(component, Props) -> Response`
  cookies.rs      (B) key derivation + the flash cookie ("_flash", private/encrypted)
  flash.rs        (B) FlashState { notice, alert, errors: Option<Value>, error_bag, clear_history,
                      preserve_fragment } load/consume + write
  redirect.rs     (B) `Redirect` builder + redirect/external/303 middleware + `location(url)`
  csrf.rs         (B) CSRF middleware + cookies
  headers.rs      (B) CSP nonce + security headers middleware; `CspNonce(String)` request extension
  precognition.rs (B) helpers
```

## Shared interfaces between A and B (both must match exactly)

```rust
// headers.rs (B) inserts into request extensions; render.rs (A) reads it (fallback: no nonce attr).
#[derive(Clone)] pub struct CspNonce(pub String);

// flash.rs (B). Middleware (B) loads the cookie into a request extension:
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct FlashState {
    pub notice: Option<String>,
    pub alert: Option<String>,
    pub errors: Option<serde_json::Value>,   // {"field": ["msg", ...]}
    pub error_bag: Option<String>,
    pub clear_history: bool,
    pub preserve_fragment: bool,
}
#[derive(Clone)] pub struct IncomingFlash(pub std::sync::Arc<FlashState>);   // request extension
// Middleware (B) removes the cookie on any non-redirect response that was rendered
// (render.rs marks consumption by inserting response extension `FlashConsumed`),
// and KEEPS it on redirects and 409 version-mismatch responses.
#[derive(Clone, Copy)] pub struct FlashConsumed;
// Handlers set outgoing flash by returning a response carrying extension
// `OutgoingFlash(FlashState)`; middleware (B) writes the cookie.
#[derive(Clone)] pub struct OutgoingFlash(pub FlashState);
```

`Inertia::render` (A) builds page.flash from `IncomingFlash` (notice/alert only if present),
the errors prop from `IncomingFlash.errors` (default `{}`, nested under `X-Inertia-Error-Bag` or
the stored error_bag), clearHistory/preserveFragment from it, and inserts `FlashConsumed`.

`Redirect` (B):
```rust
Redirect::to(url).notice("..").alert("..").errors(Errors-or-Value).clear_history().preserve_fragment()
   -> impl IntoResponse (302 + Location + OutgoingFlash)
Redirect::back(&HeaderMap, fallback)   // Referer if same-origin, else fallback
pub fn location(headers: &HeaderMap, url: &str) -> Response  // 409 X-Inertia-Location if inertia else 302
```
The redirect middleware (B) rewrites 301/302→303 for inertia PUT/PATCH/DELETE and external
Location on inertia requests → 409 + X-Inertia-Location. Also the version-mismatch 409 lives in
(A)'s render path OR a middleware owned by A (`version.rs`, A) — A owns it.

Shared props: (A) provides
```rust
pub type SharedPropsFn = Arc<dyn Fn(&axum::http::request::Parts, &AppContext) -> BoxFuture<'static, Result<Props>> + Send + Sync>;
// registered in ctx.shared_store as `SharedProps(SharedPropsFn)`; render.rs calls it.
```
The controllers agent registers the `auth` prop (user/session come from a request extension
`CurrentSession` that the controllers agent's auth middleware inserts — not A/B's concern).

Wiring in src/app.rs (lead does/validates it): `after_routes` applies, outermost first:
headers (B) → csrf (B) → flash (B) → redirect (B) → version (A). Each module exposes
`pub fn layer(router: axum::Router, settings: Arc<Settings>) -> axum::Router`.
