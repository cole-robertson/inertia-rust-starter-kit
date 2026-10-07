# Inertia server adapter (`src/inertia/`): rendering, props, SSR, Vite

The Inertia.js v3 protocol itself (props and partial reloads, the page object, rendering JSON or
the HTML document, asset versioning, the redirect rules) is [inertia-omega](https://github.com/inertiajs/inertia-omega),
the Inertia team's Rust adapter, a port of inertia-laravel, imported as `omega`. Until it is on
crates.io, `Cargo.toml` pins a fork's `kit` branch: upstream main plus a few fixes, each open as a
pull request (listed in `Cargo.toml`). `src/inertia/` wires it into Loco and adds what the kit keeps
from inertia-rails: Rails-style flash in the encrypted flash cookie (prefetches leave it alone),
server-managed head tags, the kit's HTML document and SSR client, CSRF, the CSP nonce,
Precognition, the external-redirect rule and the asset-version reload on `app_url`.
Flash, redirects, CSRF and security headers are covered in [INERTIA_SECURITY.md](INERTIA_SECURITY.md).

## Rendering

```rust
use crate::inertia::{defer, lazy, Inertia, Props};

async fn show(inertia: Inertia) -> loco_rs::Result<Response> {
    inertia.render("Dashboard/Index", Props::new()
        .with("stats", lazy(|| async { Ok(json!({"n": 1})) }))
        .with("activity", defer(|| async { Ok(load_activity().await?) })))
    .await
}
```

`Inertia` is an extractor. It needs `AppContext` as router state, `Arc<Settings>` in
`shared_store`, and the Inertia layer (`render::layer`, installed in `App::after_routes`).

`render` runs the shared props (below) and has omega resolve the page right away, in the handler:

- If the request has `X-Inertia: true`, the response is the page as JSON (`application/json; charset=utf-8`) and carries `X-Inertia: true`.
- Any other request gets the HTML document from `document.rs` (omega's root view).
- Both responses carry `Vary: X-Inertia`.
- A lazy prop's error is `render`'s error, as if the handler had returned it: a `NotFound` is a 404.

omega would rather resolve a returned render later, in its layer (Laravel's `Responsable`). The
kit doesn't: Loco's compression and ETag middlewares sit between that layer and the handler, and
would see an empty body.

The page object contains:

- `component`, `props`, `url` (the original path and query) and `version`.
- `sharedProps`: the top-level shared keys, `errors` first (see below).
- `flash`: `{notice, alert}`, only when at least one is set.
- `encryptHistory` (`settings.encrypt_history`), `clearHistory` and `preserveFragment`, each only when true.
- The metadata keys `deferredProps`, `scrollProps`, `mergeProps`, `prependProps`, `deepMergeProps`, `matchPropsOn`, `onceProps` and `rescuedProps`. Each is omitted when empty.

### The `errors` prop

`errors` is always present and always included, even in a partial reload that doesn't ask for it.

- Its default value is `{}`.
- The value comes from the errors a `Redirect::errors` carried to this request: every message per field (`{"name": ["can't be blank"]}`, omega's `with_all_errors`), the shape the frontend reads.
- When the request has `X-Inertia-Error-Bag`, or the redirect set an `error_bag`, the errors are nested under that bag name.
- It is a shared prop, so `sharedProps` always starts with `"errors"`.

## Props

Props are omega's `Props` and `Prop` (`src/inertia/props.rs` re-exports them). Build them with
`Props::new().with(key, value)`; a value is anything `Serialize` (a `json!`, a typed props
struct) or a `Prop`. The lazy constructors take closures returning Loco's `Result`, as handlers do.

| Builder | Ruby | First visit | Partial reload |
|---|---|---|---|
| any `Serialize` / `Prop::value(v)` | plain | sent | sent if selected |
| `lazy(|| async { Ok(..) })` | `-> { }` | evaluated + sent | evaluated only if selected |
| `always(v)` / `.always()` | `InertiaRails.always` | sent | always sent |
| `optional(..)` / `.optional()` | `InertiaRails.optional` | not sent | sent if selected |
| `defer(..)` / `.deferred()` / `.group("g")` | `InertiaRails.defer(group:)` | listed in `deferredProps` | sent if selected |
| `merge(v)` / `.merge()` / `.prepend()` / `.append_at(p)` / `.prepend_at(p)` / `.match_on(f)` | `InertiaRails.merge` | `mergeProps` / `prependProps` / `matchPropsOn` | same (unless in `X-Inertia-Reset`) |
| `deep_merge(v)` / `.deep_merge()` | `InertiaRails.deep_merge` | `deepMergeProps` | same |
| `once(..)` / `.once()` / `.once_as(k)` / `.until(duration)` / `.fresh()` | `InertiaRails.once` | `onceProps`; skipped if in `X-Inertia-Except-Once-Props` | sent anyway when explicitly requested |
| `scroll(paginator)` / `scroll_with(|| async { paginator })` / `.wrapper("data")` | `InertiaRails.scroll` | `scrollProps` + merge at the wrapper | `X-Inertia-Infinite-Scroll-Merge-Intent: prepend` prepends; `X-Inertia-Reset` sets `reset: true` |
| `.rescue()` | `rescue: true` | a failure is logged and listed in `rescuedProps` instead of failing the render | same |

A lazy closure is `FnOnce() -> impl Future<Output = Result<impl Serialize>>`. It runs at most once,
and only when the prop is kept. **Sibling closures run concurrently** (omega's resolver), so
independent queries overlap; the page and its metadata lists are still in prop order. `scroll`
takes omega's `Paginator` (or any `ProvidesScrollMetadata + Serialize`). A scroll prop's merge
intent is applied when it is resolved, as in inertia-laravel: a **deferred** scroll prop with
`.wrapper("data")` reports `mergeProps: ["users"]` on the first visit, where it is only listed,
and `["users.data"]` (or `prependProps`) on the partial reload that loads it.

### Keys and partial reloads

- Keys can use dot notation (`"auth.user"`), and a `Props` value can be nested inside another `Props`, with behaviour of its own.
- `X-Inertia-Partial-Data` and `X-Inertia-Partial-Except` select dot paths in both directions: a key selects its ancestors and all of its descendants, and filtering reaches inside plain JSON objects too.
- These headers only apply when `X-Inertia-Partial-Component` matches the component being rendered.

## Shared props

Register a `SharedProps(SharedPropsFn)` in `ctx.shared_store`:

```rust
let f: SharedPropsFn = Arc::new(|parts, _ctx| Box::pin(async move { Ok(Props::new().with("auth", ..)) }));
ctx.shared_store.insert(SharedProps(f));
```

- It runs when a page renders (not on redirects or JSON endpoints), with the request parts and the `AppContext`, and hands each prop to omega's `share`.
- Page props override shared props key by key (a shallow merge).
- The top-level keys of the shared props, after `errors`, are reported in `sharedProps`.

This kit registers `auth` and `accounts` in `src/auth.rs`.

## Head tags (`meta.rs`, inertia-rails `server_head`)

A port of `InertiaRails::MetaTag` and `MetaTagBuilder`, plus the renderer and helper parts:

```rust
use crate::inertia::{InertiaMeta, MetaTag};

inertia
    .meta(
        InertiaMeta::new()
            .title("Dashboard")
            .tag(MetaTag::new().attr("name", "description").attr("content", "…"))
            .tag(MetaTag::new().tag_name("script").tag_type("application/ld+json")
                 .inner_content(json!({"@context": "https://schema.org"}))),
    )
    .render("dashboard/index", props)
    .await
```

- **Head keys** are generated as in Ruby: `title`, `meta-charset`, `meta-name-<parameterized>`, `meta-property-…`, `meta-http_equiv-…`, or `<tag>-<8 hex of sha256("k=v&…")>`. `.head_key(k)` overrides, and `.allow_duplicates()` adds the digest suffix. A tag with an existing head key replaces the earlier one. `remove(key)`, `remove_if(f)` and `clear()` mirror the builder.
- **Scripts**: a `script` tag is always `type="text/plain"` unless it is `application/ld+json` (compared case-insensitively), so nothing executes. ld+json content is JSON-escaped for a `<script>` body. Other content and all attributes are HTML-escaped.
- **Structural keys** are not attributes. `.attr("type", …)` is the same as `.tag_type(…)`, and `tag_name`, `head_key`, `allow_duplicates` and `inner_content` go to their builders too, in any letter case, snake_case or camelCase. An attribute named like the head-key marker (`inertia` / `data-inertia`) is dropped. So `type` is normalized in one place and appears exactly once in the HTML and the JSON.
- **Settings** (`settings:`):
  - `server_head: false` (the default): the tags go into the props as objects (`tagName`, `headKey`, camelCased attributes) under `_inertia_meta`, and are marked with the `inertia` attribute.
  - `server_head: true`: the tags go in as HTML strings under `head`, the prop Inertia v3's `serverHead` client option reads, and are marked with `data-inertia`. `server_head: seo` uses the prop name `seo` instead.
  - `use_data_inertia_head_attribute: true` switches the attribute to `data-inertia` without `server_head`.
- **Reserved prop**: with `server_head` on, a page or shared prop named like the meta prop (`head` by default) makes the render fail, as in `validate_meta_prop!`.
- **Title template**: `ctx.shared_store.insert(MetaTitleTemplate(Arc::new(|title| title.map(|t| format!("{t} | Kit")))))` is `meta_title_template`. It receives the current title and is applied before serializing. A blank result leaves the title alone.
- **HTML**: on client-rendered HTML responses the tags are also written into `<head>` (`inertia_meta_tags`), and a server-managed `<title>` replaces the document's default one. On SSR responses the server writes only the tags the SSR head lacks, matched by their `data-inertia` head key, plus the title when SSR rendered none. Nothing repeats, and nothing is lost if the client and server settings disagree.
- **Client** (`frontend/entrypoints/app.ts`, shared by `inertia.tsx` and the SSR entry `ssr.tsx`): `serverHead` comes from `VITE_INERTIA_SERVER_HEAD` at build time. The YAML `server_head` reads the same variable at boot, so set it for both. With it on, `@inertiajs/react` 3.7 renders the `head` prop's HTML into the SSR head, applies it on hydration, and replaces it on every navigation. Its head manager keys tags by `data-inertia`, so a page's `<Head>` tag with the same key, or any `<Head title>`, wins over the server's. `e2e/head.spec.ts` checks the initial HTML and navigation in both the CSR and SSR projects.
- **Object mode** (`server_head: false`, the kit default): the React adapter has no consumer for `_inertia_meta`. The tags are rendered by the server only: into the document head on CSR, and as missing tags on SSR. They are not updated on client navigation. Use `server_head: true` for tags that follow navigation, or render `_inertia_meta` with `<Head>` yourself (inertia-rails' cookbook `MetaTags` component).
- The meta prop is added after partial-reload filtering, so it is present on every response that has tags.

## HTML document

`document.rs` is omega's root view, and mirrors the Rails kit's `layouts/application.html.erb`. It contains:

- `<title data-inertia>`
- the viewport and app meta tags
- the favicons
- the inline dark-mode script, which carries the CSP nonce
- the Vite tags
- the SSR head

Without SSR, the body is `<script data-page="app" type="application/json" nonce=…>` followed by `<div id="app">`.

The page JSON is made safe for a script context: `<`, `>`, `&`, U+2028 and U+2029 are escaped to `\uXXXX`.

The nonce comes from the headers layer's `CspNonce` request extension, which `Inertia::render` passes to the document as view data. If that extension is missing, no nonce attribute is written.

## Vite

`settings.vite.dev_server: true`:

- The tags point at the dev server: the React Refresh preamble (inline, with the nonce), `@vite/client`, the stylesheet and `frontend/entrypoints/inertia.tsx`.
- The version is `"dev"`.

`settings.vite.dev_server: false`:

- `public/vite/.vite/manifest.json` is read once, at boot in `inertia::install`, and cached.
- The tags are built from the manifest: stylesheets (the `application.css` entry plus the css of the entry chunk and its imports), the entry as a `type="module"` script, and a `modulepreload` for each static import. Everything is served under `/vite/`.
- The version is the SHA-256 of the manifest bytes.

A missing manifest:

- In production it is a boot error.
- In other environments the app logs a warning and renders pages without asset tags.

## Asset versioning

omega answers an Inertia `GET` whose `X-Inertia-Version` differs from the current version (the Vite
version above) with `409` and `X-Inertia-Version`. The Inertia layer sets its
`X-Inertia-Location` to `{app_url}{original path+query}` (or the extra host the request came in
on), never to the request's own `Host`. The client then does a full page load.

The flash waits for the next render: a 409 renders nothing, so it leaves the flash cookie alone.

## Prefetch requests

`redirect::is_prefetch(&headers)` is true when `Purpose`, `Sec-Purpose` or `X-Moz` is `prefetch` (any case), as Laravel's `Request::prefetch()` and inertia-omega's `Request::is_prefetch` read it. Inertia's `<Link prefetch>` sends `Purpose: prefetch`. Two layers treat a prefetch differently (see [INERTIA_SECURITY.md](INERTIA_SECURITY.md)):

- the flash layer leaves the `_flash` cookie alone, so a prefetched page shows no flash and the visit that follows still gets it;
- a redirect to a URL with a `#fragment` stays a redirect instead of becoming a 409.

## SSR (`ssr.rs`)

`settings.ssr.enabled` switches SSR on for HTML responses. Inertia JSON visits never call the SSR server.
The kit's `SsrClient` is omega's SSR gateway (`omega::ssr::Gateway`); it is kept instead of omega's
`HttpGateway` for the dev-server switch below and for its logging, which never includes the
response body.

- **Dev** (`vite.dev_server`): the page is POSTed to `{vite.dev_server_url}/__inertia_ssr`, which the `@inertiajs/vite` plugin serves.
- **Prod**: the page is POSTed to `settings.ssr.url`, which defaults to `http://127.0.0.1:13714/render`.
- The response is `{head: [..], body}`:
  - `head` goes into `<head>`, and replaces our `<title>` when it contains one.
  - `body` is inserted as-is, because it already contains the page script and the root div.
- Any failure falls back to client rendering and logs a warning with the component name. Failures include a connection error, a timeout (`ssr.timeout_ms`), a non-2xx status and a bad body.
- For a non-2xx status only the status is logged, never the response body. Inertia's SSR error body echoes the page, including its URL, and a password-reset URL carries its `sid`.
- **Node output is redacted too.** `@inertiajs/core`'s SSR server prints render errors with `URL: <page.url>`. The spawned child's stdout and stderr are therefore piped, not inherited: each line goes through `request_log::redact_text` into `tracing` (the `ssr_output` field, at info for stdout and warn for stderr). In dev, Vite's `/__inertia_ssr` endpoint logs the same error through Vite's logger, which `vite.config.ts` replaces with a redacting one (`redactSensitiveQueryValues`, the same parameter rules).

**Process.** With `ssr.enabled && ssr.spawn`, the `SsrSupervisor` initializer runs `{ssr.node} {ssr.bundle}` as a child process.

- The child uses `kill_on_drop`.
- It is restarted with exponential backoff, from 1 s up to 30 s. The backoff resets after 30 s of uptime.
- It is killed in `Hooks::on_shutdown`.
- If the bundle file is missing, the supervisor logs one line and does nothing.
- The child listens on the port of `ssr.url`, passed as `INERTIA_SSR_PORT` and read by `frontend/entrypoints/ssr.tsx` when node starts (default 13714, bound to 127.0.0.1). The port is not baked into the bundle, so two apps built from one checkout can use different ports (`SSR_URL`).

Production turns `spawn` on by default (`SSR_SPAWN`). Set `SSR_SPAWN=false` to run `node ssr/ssr.js` yourself.

## Boot

`App::after_context` calls `inertia::install`, which does three things:

1. Parses and validates `settings:`, and records whether the resolved `AppContext.environment` is production (`Settings::production`, never read from YAML). In production it refuses to boot when:
   - `secret_key_base` is shorter than 64 characters, is one of the `config/development.yaml`/`config/test.yaml` examples, or contains `development-secret` or `test-secret`;
   - `app_url` is empty, not an absolute URL, or not `https://`. `allow_insecure_http: true` (`ALLOW_INSECURE_HTTP=true`, off by default) permits `http://` for trying a production build locally.
2. Loads the Vite assets.
3. Stores `Arc<Settings>` in `shared_store`.

## Public files (`public.rs`)

`App::before_routes` starts the router from `inertia::base_router()`, whose fallback serves `public/`. Loco's `static` middleware is disabled in every environment.

- It only runs when no route matched, so a file can never shadow a route. A path that has a route for other methods gets that route's 405.
- It serves only GET and HEAD. Any other unmatched request is a plain 404.
- `/vite/…` gets `Cache-Control: public, max-age=31536000, immutable`. Other files (`/icon.png`, `/icon.svg`, `/robots.txt`, the error pages) get `public, max-age=3600`.
- A missing file, a directory, or any path with a dot-segment (`/vite/.vite/manifest.json`) is a real 404 with `public/404.html` as the body and `Cache-Control: no-cache`. Segments are checked after percent-decoding, as `ServeDir` decodes them, so `/%2egit/config`, `/vite/%2evite/manifest.json`, encoded separators (`%2f`, `%5c`), a backslash, a NUL and a malformed escape are 404s too.
- It runs inside the whole middleware stack, so these responses get the security headers too.

## Request logging (`request_log.rs`)

`App::middlewares` is Loco's default stack with its `logger` replaced by `inertia::request_log::Middleware`. It has the same name, the same `server.middlewares.logger.enable` switch, the same position and the same span fields, except that `http.uri` is redacted:

- The values of `sid`, `token`, `password*` and `*_token` query parameters (case-insensitive, including the innermost key of `user[password]`-style names) become `[FILTERED]`.
- Other parameters are kept byte for byte.
- `redact_text` applies the same rules to `?name=value` / `&name=value` pairs anywhere in a free-form line. The SSR child's output goes through it.

`tests/inertia_a.rs` runs the real stack against a failing SSR server that echoes the page, captures the formatted log output, and asserts that a known `sid` never appears. It also spawns node through `ssr::spawn_redacted`, once with a script that prints sid-bearing URLs and once with the real SSR bundle forced into a render error, and asserts that the captured `tracing` output is redacted.
