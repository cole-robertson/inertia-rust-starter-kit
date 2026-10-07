# Inertia adapter: cookies, flash, redirects, CSRF, headers, precognition

Owner: agent B (`src/inertia/{cookies,flash,redirect,csrf,headers,precognition}.rs`).
Tests: `tests/inertia_b.rs`.

## Layer order

`src/app.rs` applies these outermost first: **headers → csrf → flash → redirect → version (A)**.
Each module exposes `layer(router, Arc<Settings>) -> Router`.

- `headers` sits outermost, so every response gets the security headers, including CSRF rejections.
- `csrf` rejects before any handler or flash work happens.
- `flash` runs outside `redirect`. That way it sees the final status after the 303 and 409 rewrites, and it still sees the `OutgoingFlash` and `FlashConsumed` extensions, which `redirect` keeps.

## Keys (`cookies.rs`)

Each cookie purpose gets its own 64-byte `cookie::Key`:

```
key(label) = HMAC-SHA256(secret_key_base, "inertia/<label>/0") || HMAC-SHA256(secret_key_base, "inertia/<label>/1")
```

The labels are `flash`, `csrf` and `session`. A key used for one purpose can't be used to forge another purpose's cookie. Changing `secret_key_base` invalidates all of them at once: sessions, flash and CSRF.

| Cookie | Protection | Flags |
|---|---|---|
| `_flash` | private jar (AES-256-GCM) | HttpOnly, SameSite=Lax, Path=/, Secure in production or on https, session lifetime |
| `_csrf` | signed jar (HMAC) | HttpOnly, SameSite=Lax, Path=/, Secure in production or on https, 20 years |
| `XSRF-TOKEN` | none (it is the public token) | **not** HttpOnly, SameSite=Lax, Path=/, Secure in production or on https |
| `session_token` | signed jar (HMAC) | HttpOnly, SameSite=Lax, Path=/, Secure in production or on https, 20 years (Rails `permanent`) |

`Secure` is set in production (the resolved `AppContext.environment`, carried as `Settings::production`), and in any environment when `settings.app_url` starts with `https://`. Production also refuses to boot with a non-https `app_url` unless `allow_insecure_http: true`.

The controllers agent uses these session helpers: `set_session_token`, `read_session_token`, `clear_session_token`, `session_token_cookie`, and `session_cookie_key`.

## Flash (`flash.rs`)

`FlashState { notice, alert, errors, error_bag, clear_history, preserve_fragment }` is stored as JSON in `_flash`.

- **Request:** the middleware decrypts the cookie into the `IncomingFlash(Arc<FlashState>)` extension. The state is empty when the cookie is absent. When the cookie can't be decrypted, the state is empty, a warning is logged and the cookie is deleted.
- **Response with `OutgoingFlash`:** the middleware writes the cookie, replacing whatever was there.
- **Response with `FlashConsumed`** (inserted by the render path): the middleware deletes the cookie, **unless** the status is 301/302/303/307/308 or 409. This matches inertia-rails `keep_inertia_session_options?`: the flash survives redirects and version-mismatch reloads until a render actually shows it.
- **Any other response** (JSON endpoints, assets, …) leaves the cookie alone.

## Redirects (`redirect.rs`)

- `Redirect::to(url)` returns a 302. The builder methods `.notice/.alert/.errors(impl Serialize)/.error_bag/.clear_history/.preserve_fragment` attach an `OutgoingFlash`.
- `Redirect::back(&headers, fallback)` uses the Referer only when its host:port matches the request `Host`. It keeps only the path and query, and falls back when that path is a network-path reference (`//evil.example`, `/\evil.example`, which browsers resolve to another host), so it can't be used as an open redirect.
- `location(&headers, url)` returns 409 + `X-Inertia-Location` for Inertia requests and a 302 otherwise. This is inertia-rails `inertia_location`.
- The middleware applies only to requests with `X-Inertia: true`. It does two things, both ported from `InertiaRails::Middleware`:
  - **External redirects:** the `Location` of a 301/302/303 is first resolved against `app_url`, the way a browser resolves it, so scheme-relative `//evil.example` counts as absolute while `/path` and `path` stay internal. If the result differs from both `app_url`'s origin and the request `Host` (scheme, host or port), the response becomes a 409 with `X-Inertia-Location`. Other headers are kept, Set-Cookie in particular (it matters in the middle of an OAuth flow). The body, Content-Type and Content-Length are dropped.
  - **Method rewrite:** a 301/302 answering a PUT, PATCH or DELETE becomes a 303, so the browser follows it with a GET.
  - **Fragment redirects:** a 201/301/302/303/307/308 whose `Location` contains `#` becomes a 409 with `X-Inertia-Redirect: <location>`, unless the request is a prefetch. `fetch` drops the fragment when it follows a redirect, so the client visits the URL itself (`router.visit`, as a GET) and keeps the `#section`. Other headers (Set-Cookie) and the response extensions are kept, so a flash still rides along; the body is dropped. This is inertia-laravel's `Middleware#handle` → `onRedirectWithFragment` (also in inertia-omega's `protocol::after`); inertia-rails doesn't do it. The external-redirect rule runs first, so an external URL with a fragment is still an `X-Inertia-Location` visit.

## CSRF (`csrf.rs`)

The scheme is double-submit with a server-bound secret, so an attacker can't fixate a token:

- Each browser gets a random 32-byte secret in the signed, HttpOnly `_csrf` cookie.
- Tokens are `base64url(nonce16 || HMAC-SHA256(secret, nonce16))`. Each token is freshly masked, verified in constant time (`Mac::verify_slice`), and bound to that browser's secret. A cookie injected from a sibling subdomain doesn't help an attacker, because they can't compute the MAC.
- On GET/HEAD, a fresh `XSRF-TOKEN` is set when the browser's token is missing or doesn't verify. The Inertia client (axios/fetch) sends it back as `X-XSRF-TOKEN`. `X-CSRF-TOKEN` is also accepted.
- Non-safe methods (everything except GET/HEAD/OPTIONS/TRACE) are rejected when any of these hold:
  - `Sec-Fetch-Site: cross-site`
  - an `Origin` header that doesn't equal `app_url`'s origin
  - a token that is missing or invalid, or no `_csrf` secret at all
- A rejection is a 422 with `Can't verify CSRF token authenticity.`: plain text normally, `{"message": …}` for Inertia requests. The reason is logged but never sent to the client.
- **Rotation:** on sign-in or sign-out, the handler adds the `RotateCsrf` response extension. The middleware then issues a new secret and a new `XSRF-TOKEN`, and tokens issued before the rotation stop working.
- If `settings.forgery_protection: false`, the layer is a no-op (no checks, no cookies).

## Security headers (`headers.rs`)

- Each request gets a random 128-bit nonce in the `CspNonce(String)` extension. The document builder (A) puts it on the inline theme and page scripts.
- The CSP is: `default-src 'self'; script-src 'self' 'nonce-…'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'`.
  - `style-src` needs `'unsafe-inline'` because Radix/shadcn set inline styles.
- With `vite.dev_server: true`, the Vite origin is added to script, style, img, font and connect, and its `ws://` (or `wss://`) origin is added to connect for HMR.
- Every response also gets:
  - `X-Content-Type-Options: nosniff`
  - `Referrer-Policy: strict-origin-when-cross-origin`
  - `X-Frame-Options: DENY`
  - `Permissions-Policy: camera=(), microphone=(), geolocation=()`
  - `Cross-Origin-Opener-Policy: same-origin`
- `Strict-Transport-Security: max-age=63072000; includeSubDomains` is sent only in production. That is decided by the resolved `AppContext.environment` (`Settings::production`, set in `Settings::from_ctx`), so `cargo loco start --environment production` gets HSTS without `LOCO_ENV` being set.
- If a handler already set one of these headers, its value wins.

## Precognition (`precognition.rs`)

- `is_precognition(&headers)` checks for `Precognition: true`. When it's set, the handler validates and returns without writing anything.
- `respond(&errors)` returns 204 + `Precognition-Success: true` when there are no errors. Otherwise it returns 422 `{"errors": {...}}`. Both responses carry `Precognition: true` and `Vary: Precognition`.
- `respond_for(&headers, &errors)` does the same after filtering the errors to the fields listed in `Precognition-Validate-Only`, which is how the form validates only the fields the user has touched.
- `filter_errors` and `validate_only` are also exported.

## Production boot checks and client IPs

- `secret_key_base`: at least 64 characters, and not one of the committed development/test examples (anything containing `development-secret` or `test-secret` is refused too).
- `app_url` must be `https://` in production. The opt-out `allow_insecure_http: true` (env `ALLOW_INSECURE_HTTP`) exists only for trying a production build locally, and is off by default.
- `config/production.yaml` enables Loco's `remote_ip` with `source: RightmostXForwardedFor`. The client IP (rate limiting, session records) is the rightmost `X-Forwarded-For` entry, the one kamal-proxy appends. That assumes exactly one trusted hop, with the app port reachable only through the proxy. Entries further left are client-controlled and ignored. Development and test leave `remote_ip` off and use the socket address.
- Request logs redact token-bearing query parameters (see `docs/INERTIA.md`, "Request logging"). SSR error bodies are never logged.
