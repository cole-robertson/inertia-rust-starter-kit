# Parity with the Rails kit

Reference: [inertia-rails/react-starter-kit](https://github.com/inertia-rails/react-starter-kit) at
`f808193` (2026-09-25; still upstream HEAD on 2026-09-29). This kit is a port of it and aims at
**full parity**: the same routes, pages, texts and behaviour. On top of that it keeps its own
Rust-specific extras: generators, `bin/rename`, the agent skills, benchmarks and the Cloudflare
deploy. One addition changes behaviour on purpose: **organizations** (accounts, memberships,
invitations; see [below](#organizations-a-deliberate-addition)).

Two parts:

1. **The oracle.** `bench/parity/compare.sh` boots both kits side by side and compares what they
   do. Rerun it any time.
2. **The audit table** below. It covers the whole Rails kit, file by file and behaviour by
   behaviour, including what the oracle can't see (DX, CI, Docker, tests).

## The oracle

```sh
# the Rails kit, bundled, with `bin/rails assets:precompile` done (RAILS_ENV=production)
RAILS_KIT=~/.cache/parity/rails bench/parity/compare.sh
```

The script starts both apps in production mode. Each gets a fresh, empty SQLite database and
its own SMTP sink (`smtp_sink.py`, standard library only). The only change to the Rails kit is
one initializer that the script writes: it points Action Mailer at the sink when
`PARITY_SMTP_PORT` is set.

`probe.py` then drives both apps through 124 steps:

- **Pages:** every page as HTML and as an Inertia visit, signed out and signed in.
- **Sign-up:** invalid input, blank and over-long passwords, email normalization, a duplicate
  email.
- **Sign-in:** wrong and right credentials, and while already signed in.
- **Settings:** profile, email and password, each with a wrong, missing or right challenge.
- **Sessions:** list, revoke another session, a foreign session and the current one; the effect
  of a password change on other sessions.
- **Email verification:** resend with and without a Referer, bad, expired and valid links.
- **Password reset:** verified, unverified and unknown email; mismatch, short password, bad
  token, a used link.
- **Account deletion:** wrong, missing and right challenge.
- **Errors:** 404, 406 for old browsers (11 user agents), a CSRF failure as HTML and as
  Inertia, trailing slashes, unknown methods, `HEAD`.
- **Health:** `/up` as HTML and as JSON.
- **Response headers** of each kind of response.
- **Explicit JSON `null` params.**

For every step it records the status, the redirect `Location`, the Inertia page object
(component, URL, props, flash, history flags), validation errors, each delivered mail
(subject, from, to, MIME parts, bodies), the `<head>` of HTML pages and the response headers.

`diff.py` compares the two transcripts field by field. It masks only values that can never
match: ids, tokens, timestamp digits (the format is still compared), asset fingerprints, the
Inertia version and host:port. A difference not listed in `bench/parity/allowed.json` fails the
run. So does an allowlist entry that matched nothing. Every allowed difference is a row below
with status **intentional** and its reason.

## Status legend

| status | meaning |
|---|---|
| same | identical (verified by the oracle, a byte diff, or reading both sides) |
| differs → fixed | was different; fixed in this pass |
| missing → fixed | the Rails kit has it and this kit didn't; added in this pass |
| intentional | different on purpose; the reason is given here and in `bench/parity/allowed.json` |
| extra | this kit only; kept |

## Audit table

`R:` is a Rails kit path (`~/.cache/em-kit-latest`) and `K:` is this kit.

### Routes (R:config/routes.rb)

| area | Rails kit | this kit | status | fix? |
|---|---|---|---|---|
| `GET/POST /sign_in` (`sign_in`) | R:routes.rb:4-5 | K:src/route_table.rs:15, controllers/sessions.rs:90 | same | |
| `GET/POST /sign_up` (`sign_up`) | R:routes.rb:6-7 | K:route_table.rs:16, controllers/users.rs:102 | same | |
| `DELETE /sessions/:id` | R:routes.rb:9 | K:route_table.rs:17, sessions.rs:91 | same (the id is the session's random token; intentional, see Models) | |
| `DELETE /users` | R:routes.rb:10 | K:route_table.rs:18, users.rs:103 | same | |
| `GET/POST /identity/email_verification` | R:routes.rb:13 | K:email_verifications.rs:70 | same | |
| `/identity/password_reset` new/edit/create/update | R:routes.rb:14 | K:password_resets.rs:149 | same | |
| `GET /dashboard` | R:routes.rb:17 | K:dashboard.rs | **intentional**: a redirect to the last-used account's overview (`/{account_slug}`), which is the dashboard here; see Organizations | no (allowed) |
| settings profile/password/email show+update, sessions index | R:routes.rb:20-23 | K:settings/*.rs | same (PUT also routed, like Rails `resource`) | |
| `inertia :appearance` | R:routes.rb:24 | K:settings/appearance.rs | same (authenticated, component `settings/appearance`, TS in `RoutesController`) | |
| `root "home#index"` | R:routes.rb:27 | K:home.rs:22 | same | |
| `GET /up` | R:routes.rb:30 (`rails/health#show`) | K:controllers/health.rs | differs → fixed: the same green HTML page (`text/html`), `{"status":"up","timestamp":…}` for JSON, `Vary: Accept`; was `OK` as `text/plain` | yes |
| PWA routes (commented out) | R:routes.rb (none; the layout comment only) | — | same: neither kit routes `/manifest.json` or `/service-worker.js`; R:app/views/pwa/* are unused templates | see PWA row |
| trailing slash (`/sign_in/`) | Rails routes ignore a trailing slash | K:src/inertia/mod.rs `service`, app.rs `serve` | differs → fixed: trimmed before routing (was 404) | yes |
| a known path with the wrong method (`GET /sessions/1`, `PATCH /nope`) | 404 + public/404.html | K:src/inertia/exceptions.rs | differs → fixed: 404 + public/404.html (was 405, empty body). axum still adds an `Allow` header, which Rails doesn't send; harmless | yes |
| `HEAD` for unknown paths | 404, empty | 404, empty | same | |

### Controllers: flash texts, redirects, error shapes

Every flash string below was compared byte for byte by the oracle (`page.flash` after following
the redirect) and by reading both sources.

| area | Rails kit | this kit | status | fix? |
|---|---|---|---|---|
| `authenticate`: redirect to `/sign_in`, no flash | R:application_controller.rb:12 | K:src/auth.rs:143 | same | |
| `require_no_authentication`: "You are already signed in" → `/` | R:application_controller.rb:16 | K:auth.rs:166 | same | |
| sign-in success "Signed in successfully" → `/dashboard` | R:sessions_controller.rb:16 | K:sessions.rs | **intentional**: same flash, but → `/{account_slug}` of the last-used account; see Organizations | no (allowed) |
| sign-in failure alert "That email or password is incorrect" → `/sign_in` | R:sessions_controller.rb:18 | K:sessions.rs:60 | same | |
| sign-in with `null` email/password (hand-written JSON client) | alert, 302 | 400 JSON | differs → fixed: `null` is treated like a missing param | yes |
| session destroy "That session has been logged out" + `clear_history` → `/settings/sessions` | R:sessions_controller.rb:25 | K:sessions.rs:76 | same | |
| destroying another user's session | 404 (`RecordNotFound`) + public/404.html | K:sessions.rs:73 + exceptions.rs | differs → fixed: public/404.html (was Loco's JSON) | yes |
| sign-up "Welcome! You have signed up successfully" → `/dashboard` + verification mail | R:users_controller.rb:18-19 | K:users.rs | **intentional**: same flash and mail, but → `/{slug}` of the new personal account; see Organizations | no (allowed) |
| sign-up invalid → `/sign_up` with `errors` | R:users_controller.rb:21 | K:users.rs:66 | same (all four fields, the same messages in the same order) | |
| sign-up with all-`null` params | 302 with errors | 400 JSON | differs → fixed | yes |
| account delete "Your account has been deleted" + `clear_history` → `/` | R:users_controller.rb:30 | K:users.rs:85 | same | |
| account delete wrong/missing challenge → `/settings/profile`, `errors: { password_challenge: "Password challenge is invalid" }` | R:users_controller.rb:32 | K:users.rs:93 | **intentional**: Rails sends a *string*, we send `["Password challenge is invalid"]`. The kit's own `delete-user.tsx` calls `errors.password_challenge?.map(...)`, and the string makes **the Rails kit crash**: verified in Chromium, `TypeError: i.password_challenge?.map is not a function`, blank page. The array is what the frontend expects; this is a bug in the Rails kit. | no (allowed) |
| verification link valid "Thank you for verifying your email address" → `/` | R:email_verifications_controller.rb:10 | K:email_verifications.rs:53 | same | |
| verification link used a second time | Rails: verifies again (the token is bound to the email only), until it expires | K: "That email verification link is invalid" → `/settings/email`; links are single-use (`users.rs` `verify_email` only updates an unverified user) | **intentional** (security, Cole 2026-09-29) | no (allowed) |
| verification resend "We sent a verification email to your email address", `redirect_back_or_to root_path` | R:email_verifications_controller.rb:15 | K:email_verifications.rs:65 (`Redirect::back`, same-origin Referer only) | same | |
| invalid verification link alert "That email verification link is invalid" → `/settings/email` | R:email_verifications_controller.rb:23 | K:email_verifications.rs:40 | same | |
| reset request, any email | Rails: verified → notice "Check your email for reset instructions" → `/sign_in`; unverified or unknown → alert "You can't reset your password until you verify your email" → the reset form. The two replies reveal whether a verified account exists (user enumeration). | K:password_resets.rs `create`: every request → `/sign_in` with the one notice "If that email belongs to a verified account, we've sent reset instructions to it"; only verified accounts get mail | **intentional** (security, Cole 2026-09-29) | no (allowed) |
| reset success "Your password was reset successfully. Please sign in" → `/sign_in` | R:password_resets_controller.rb:26 | K:password_resets.rs:138 | same | |
| reset invalid → edit with `sid` and errors | R:password_resets_controller.rb:28 | K:password_resets.rs:140 | same | |
| invalid reset link alert "That password reset link is invalid" → `/identity/password_reset/new` | R:password_resets_controller.rb:37 | K:password_resets.rs:64 | same | |
| `Identity::PasswordResetsController` skips `authenticate`, so `auth` is null even when signed in | R:password_resets_controller.rb:4 | K:password_resets.rs `routes` (`auth::without_session`) | differs → fixed: `auth: {user: null, session: null}` on those pages, like Rails | yes |
| email change "Your email has been changed" (only when it changed) | R:emails_controller.rb:28-32 | K:emails.rs:53-59 | same | |
| email change with a `null` challenge | Rails: `password_challenge: nil` **skips** the challenge check and the email changes | K: `password_challenge: ["is invalid"]`, nothing changes | **intentional**, a security bug in the Rails kit: `with_defaults` fills only a *missing* key, and `has_secure_password` validates the challenge only when it's non-nil, so a hand-written client can change the email without the password. (It was a 400 here before; now the normal error redirect.) | no (allowed) |
| password change "Your password has been changed" | R:passwords_controller.rb:11 | K:passwords.rs:53 | same | |
| password change with `password: null` | Rails: "can't be blank" (`password=` nil clears the digest) | K: accepted as "no change" | differs → fixed | yes |
| profile update "Your profile has been updated" | R:profiles_controller.rb:11 | K:profiles.rs:41 | same | |
| `inertia: { errors: }` shape: `{field: [messages]}` | inertia_rails `errors.to_hash` | K:models/users.rs `Errors` | same | |
| `set_current_request_details`: user agent and IP on the new session | R:application_controller.rb:27 | K:auth.rs `Details` | same (oracle: the `session record` step) | |
| `allow_browser versions: :modern`: 406 + public/406-unsupported-browser.html | R:application_controller.rb:5, actionpack allow_browser.rb | K:src/controllers/browser.rs | missing → fixed: a port of actionpack's `BrowserBlocker` and `useragent` 0.16.11's detection (safari 17.2, chrome 120, firefox 121, opera 106, never IE; bots and version-less UAs pass), checked against 64 verdicts the Rails code produced (`tests/fixtures/allow_browser.tsv`). Routed requests only, not `/up` or public files, like Rails | yes |
| CSRF failure | 422 + public/422.html (for Inertia requests too) | K:exceptions.rs | differs → fixed (was plain text, or JSON for Inertia) | yes |
| unhandled error | 500 + public/500.html | K:exceptions.rs | differs → fixed (was Loco's JSON) | yes |
| malformed request body | 400 + public/400.html | K:exceptions.rs | differs → fixed (was Loco's JSON) | yes |
| error with `Accept: application/json` | `{"status":404,"error":"Not Found"}` (PublicExceptions) | K:exceptions.rs | differs → fixed: the same JSON for 400/404/422/500 (was public/404.html) | yes |
| rate limiting of sign-in / sign-up / reset (10 per 3 min per IP, "Try again later.") | none | K:controllers/rate_limit.rs | extra (from the build brief; `rate_limit` is a Rails 8 idiom) | |
| rate limiting of password change, email change, account deletion and verification-email resend (Rails 8's `rate_limit` defaults: per client IP, one budget per endpoint, 10 per 3 min; precognitive password-change and email-change checks spend a token too) | none | K:controllers/rate_limit.rs | extra (security: an unlimited current-password oracle and unlimited verification mail) | |
| Precognition on sign-up and the settings forms | none (the pages don't use it) | K:controllers/mod.rs `precognitive` | extra | |

### Inertia page objects and shared props

| area | Rails kit | this kit | status | fix? |
|---|---|---|---|---|
| components and URLs of every page | inertia_rails `default_render` | K:controllers | same (oracle) | |
| `auth` shared prop `{user: {id,name,email,verified,created_at,updated_at}, session: {id}}` | R:inertia_controller.rb:5 | K:auth.rs:114 | same keys | |
| `auth.session.id` / `sessions[].id` type | integer | string (the session's random token) | **intentional** (brief): the id is never guessable and a cookie never carries the integer id | no (allowed) |
| timestamp format (`created_at`, `updated_at`) | `2026-09-29T16:15:24.391Z` (UTC, milliseconds, `Z`) | `2026-09-29T16:15:24Z` or `…24.123456789Z`, depending on the value | differs → fixed: serialized like Rails' `as_json` (UTC, 3 fraction digits, `Z`) | yes |
| `errors` always present (`always_include_errors_hash`) | R:config/initializers/inertia_rails.rb | K:inertia/render.rs:150 | same | |
| `sharedProps` key list | `["errors","auth"]` | same | same | |
| `encryptHistory` in production | true | true | same | |
| `clearHistory` on sign-out and delete | true | true | same | |
| `flash` top-level page key (`notice`/`alert`) | inertia_rails flash_keys | K:inertia/flash.rs | same | |
| `_inertia_meta` prop on home and dashboard (title + description) | none: the pages set their `<title>` with `<Head>` | K:home.rs, dashboard.rs | differs → fixed: removed. It was added to exercise the meta-tag API, but the pages already set the same titles with `<Head>`, and the extra `<title inertia>` and `<meta name="description">` changed the HTML head. The meta API stays (`src/inertia/meta.rs`, `tests/inertia_a.rs`); e2e/head.spec.ts now checks the Rails head and titles | yes |
| lazy props resolve concurrently (siblings and nested levels), page and metadata still in prop order | `props_resolver.rb` evaluates them one by one | K:inertia/resolver.rs `resolve` | extra (from inertia-omega): the same page object, sooner (the members page's `members` and `invitations` now load together). Only timing changes, so no oracle step does | |
| an Inertia redirect (201/301/302/303/307/308) to a URL with `#fragment` → 409 + `X-Inertia-Redirect` (not for prefetches) | none: fetch follows the redirect and drops the fragment | K:inertia/redirect.rs | extra (inertia-laravel `Middleware#handle`, inertia-omega `protocol::after`). No kit route redirects to a fragment, so no oracle step changes | |
| a prefetch (`Purpose`/`Sec-Purpose`/`X-Moz: prefetch`) leaves the `_flash` cookie alone: it shows no flash, doesn't consume it and doesn't write one | a prefetch reads (and so consumes) the flash like any visit | K:inertia/flash.rs | extra: the kit's own `<Link prefetch>` links would otherwise eat a flash before the visit shows it. The oracle sends no prefetch requests | |
| a deferred scroll prop with a wrapper: `mergeProps` on the first visit | `["users"]` (`collect_metadata` runs before `ScrollProp#call` sets the wrapper path) | `["users"]` (was `["users.data"]`) | differs → fixed. On the partial reload that loads it the kit reports `users.data`, as inertia-laravel and inertia-omega do; inertia-rails reports `users` on every visit (a non-deferred one too), where inertia-laravel, inertia-omega and the kit report `users.data` once the prop is resolved. No page of either kit uses scroll props, so the oracle can't see it | yes |

### Models

| area | Rails kit | this kit | status | fix? |
|---|---|---|---|---|
| `has_secure_password` (bcrypt) | R:user.rb:4 | K:models/users.rs (argon2id via `loco_rs::hash`) | **intentional**: the hash algorithm; bcrypt digests don't carry over (noted in fixtures) | no |
| password `length: { minimum: 12 }` "is too short (minimum is 12 characters)" | R:user.rb:18 | K:users.rs:180 | same | |
| password over 72 **bytes**: "is too long" (`has_secure_password`'s bcrypt limit) | activemodel secure_password.rb:160 | K: none; any length accepted | missing → fixed: the same rule and message (bytes, not chars). Argon2 doesn't need it, but it is observable validation behaviour. | yes |
| password blank "can't be blank" on create | secure_password.rb | K:users.rs:176 | same | |
| confirmation "doesn't match Password" | activemodel `confirmation` | K:users.rs:188 | same | |
| `password_challenge` "is invalid" | secure_password.rb:154 | K:users.rs `check_challenge` | same | |
| `validates :name, presence` "can't be blank" | R:user.rb:16 | K:users.rs:143 | same | |
| email presence + format (`URI::MailTo::EMAIL_REGEXP`) + uniqueness | R:user.rb:17 | K:users.rs:20,149,207 | same messages and order (`can't be blank`, `is invalid`, `has already been taken`) | |
| `normalizes :email, with: strip.downcase` | R:user.rb:20 | K:users.rs:139 `trim().to_lowercase()` | differs → fixed: Ruby's `strip` removes ASCII whitespace and NUL only (not U+00A0 etc.) and `downcase` is full Unicode; Rust's `trim` removes all Unicode whitespace. Now strips `\0\t\n\v\f\r ` only. | yes |
| `before_validation` on email change: `verified = false` | R:user.rb:22 | K:users.rs `change_email` | same | |
| `after_update` on password change: delete other sessions | R:user.rb:26 | K:users.rs `set_password` | same (oracle: other browser signed out after a change; every session after a reset) | |
| `generates_token_for :email_verification, 2.days { email }` | R:user.rb:6 | K:models/tokens.rs | same semantics (purpose, expiry, fingerprint); the token format differs | intentional (opaque) |
| `generates_token_for :password_reset, 20.minutes { password_salt.last(10) }` | R:user.rb:10 | K:users.rs `token_fingerprint` | same semantics: the link dies after the password changes (oracle: "GET reset link after use") | |
| `has_many :sessions, dependent: :destroy` | R:user.rb:14 | K:users.rs `destroy_with_challenge` | same | |
| `Session` `before_create` user_agent / ip_address from `Current` | R:session.rb:6 | K:models/sessions.rs `create_for_user` | same | |
| session cookie: signed integer id, permanent (20 y), httponly | R:sessions_controller.rb:14 | K: signed random token, 20 y, HttpOnly, SameSite=Lax, Secure on https | intentional (brief: unguessable token) | |
| schema: `users`, `sessions` columns | R:db/migrate | K:migration/src | same, plus `sessions.token` (unique) | |

### Mailers

| area | Rails kit | this kit | status | fix? |
|---|---|---|---|---|
| subjects "Verify your email", "Reset your password" | R:user_mailer.rb:8,15 | K:mailers/user_mailer/*/subject.t | same | |
| from `from@example.com` | R:application_mailer.rb:2 | K:config `mail_from` default | same | |
| body copy | R:app/views/user_mailer/*.html.erb | K:mailers/user_mailer/*/html.t | same text (oracle) | |
| HTML layout (`<!DOCTYPE html>…<meta http-equiv…><style>/* Email styles need to be inline */</style>…<body>`) | R:layouts/mailer.html.erb | K:mailers/user_mailer/*/html.t | missing → fixed: the same layout; the HTML part is byte-identical to Rails' (oracle) | yes |
| MIME: a single `text/html` part (no text template; `mailer.text.erb` is only a layout) | R:user_mailer views | K: `multipart/alternative` with a text part | **intentional**: Loco's mailer (`EmailSender::mail`) always sends `multipart/alternative` text+html and has no html-only path. The text part carries the same copy and link, which helps plain-text clients. | no (allowed) |
| links `identity_email_verification_url(sid:)`, `edit_identity_password_reset_url(sid:)` | R:views | K:user_mailer.rs:82 from `settings.app_url` | same path and query | |
| delivery on a queue (`deliver_later`), token minted at send time | Solid Queue | K:workers/user_mailer_delivery.rs on Loco's SQLite queue | same | |

### Frontend (`app/javascript` vs `frontend/`)

Diffed file by file (`cmp`). **92 of 97 files are byte-identical**, including every page and
layout, every hook, lib and component except the ones listed here, all 24 `components/ui`
primitives, `application.css` and `types/globals.d.ts`.

| area | Rails kit | this kit | status | fix? |
|---|---|---|---|---|
| `components/app-logo-icon.tsx` | Rails mark | two violet chevrons in an orange gear (colour gradients, `useId()` ids; 2026-10-06) | intentional (the kit's own mark) | |
| `pages/home/index.tsx` | Rails text, links | description, stack badges, server-timing, Loco/Inertia links | intentional (brief) | |
| `components/app-header.tsx`, `app-sidebar.tsx` footer links | repo + inertia-rails.dev | this repo + loco.rs docs; `// scaffold:nav` marker | intentional (links point at this project; marker used by the generator) | |
| `components/user-menu-content.tsx`, `types/index.ts` | `id: number` | `id: string` | intentional (session id as string) | |
| `routes/*.ts` header | "generated by Typelizer" | "generated by `cargo loco task routes:generate`" | intentional (brief); bodies identical | |
| `entrypoints/inertia.tsx` | options inline | options in `entrypoints/app.ts`, shared with `ssr.tsx`; same `title`, `strictMode`, `layout`, `defaults.form` (`forceIndicesArrayFormatInFormData: false`, `withAllErrors: true`), `visitOptions` (`brackets`), progress `#4B5563` | same behaviour; `serverHead` added (off by default) | |
| missing-root hint text | `Consider moving <%= vite_tags "inertia.tsx" %> …` | K:frontend/entrypoints/inertia.tsx | differs → fixed: named a file that doesn't exist (`src/inertia/template.rs`); now `src/inertia/document.rs` | yes |
| `entrypoints/ssr.tsx`, `entrypoints/app.ts`, `hooks/use-server-timing.ts` | — | present | extra | |
| `components.json` (new-york, aliases) | R:components.json | K:components.json | same except the css path | |
| `tsconfig*.json`, `eslint.config.js`, `.prettierrc`, `.prettierignore` | | | same except `app/javascript` → `frontend` and the e2e files | |
| `vite.config.ts` | `rails-vite-plugin` | manifest config, `/vite/` base, SSR entry, log redaction | intentional (brief: no Rails plugin) | |

### Dependencies (package.json / package-lock.json)

| area | Rails kit | this kit | status |
|---|---|---|---|
| every shared package in the lock file | | | same versions (all 18 named in the brief checked: react 19.3.0, @inertiajs/* 3.7.1, radix-ui 1.6.7, lucide-react 1.48.0, sonner 2.0.8, tailwindcss 4.3.3, vite 8.3.1, typescript 6.0.3, eslint 9.39.5, prettier 3.9.9, prettier-plugin-tailwindcss 0.8.1, …); the full top-level lists differ only by the rows below |
| `rails-vite-plugin` | 0.2.5 | — | intentional |
| `@playwright/test`, `@types/node` | — | 1.63.0, 22.20.4 | extra (system tests, config types) |
| npm scripts | check, format, lint | + build, dev, test:e2e; paths `frontend`, `e2e` | extra |

### Layout HTML (R:app/views/layouts/application.html.erb)

| area | Rails kit | this kit | status | fix? |
|---|---|---|---|---|
| `<title data-inertia>React Starter Kit</title>` on every page | line 4 | K:inertia/document.rs | differs → fixed: the home and dashboard had `<title inertia>Welcome</title>` from the meta prop instead. **intentional** since 2026-10-04: the default name is "Inertia Rust Starter Kit" (`settings.app_name`; `bin/rename` sets it) | no (allowed) |
| viewport, apple-mobile-web-app-capable, application-name, mobile-web-app-capable | lines 5-8 | K:document.rs:82-85 | same | |
| `csrf_meta_tags` (`csrf-param` / `csrf-token`) | line 10 | K: none | **intentional**: Inertia sends the `XSRF-TOKEN` cookie as `X-XSRF-TOKEN` in both kits; nothing reads the meta tags. Our CSRF secret is per browser and never rendered into HTML. | no (allowed) |
| `csp_meta_tag` | line 11 (empty: no CSP configured) | CSP as a response header with a nonce | extra | |
| PWA manifest comment | lines 15-16, an ERB comment: never reaches the browser | K:document.rs (a Rust comment) | same: nothing in the HTML either way; the Rust comment says how to add a manifest | |
| icons (png, svg, apple-touch) | lines 18-20 | K:document.rs:87-89 | same | |
| dark-mode inline script | lines 22-29 | K:document.rs:52 | same (with the CSP nonce) | |
| asset tags order and attributes | per entry: modulepreloads (depth first), then the entry tag, then its CSS; `<script src type="module">`; no `crossorigin` | K:inertia/vite.rs `tags` | differs → fixed: the same order and attributes (was stylesheet, script, preloads with `crossorigin="anonymous"`). Also emits imported chunks' CSS, which Vite's guide requires and rails_vite skips; the kit's chunks have none | yes |
| `inertia_ssr_head` | line 32 | K:document.rs (SSR head) | same | |

### Public files and error pages

| area | Rails kit | this kit | status | fix? |
|---|---|---|---|---|
| public/400, 404, 406-unsupported-browser, 422, 500 .html, robots.txt | R:public/* | K:public/* | same bytes (`cmp`) | |
| public/icon.png, icon.svg | R:public/* | K:public/* | **intentional** (2026-10-04): the kit's own mark (2026-10-06: two violet chevrons in an orange gear, in colour on a transparent square; `icon.png` on white for the apple-touch icon) instead of the Rails kit's red circle; same paths and types | no (allowed) |
| served on 404 | yes | yes | same | |
| served on 400 / 406 / 422 / 500 | yes | yes | missing → fixed (see Controllers) | yes |
| 404 `Cache-Control` / charset | none / `charset=UTF-8` | `no-cache` / `charset=utf-8` | intentional: a CDN must never cache a missing asset as if it existed; `utf-8` and `UTF-8` are the same charset | no (allowed) |
| public file `Cache-Control` | `public, max-age=31556952` (1 year) | `public, max-age=3600` | **intentional**: robots.txt, icons and the error pages aren't fingerprinted, so a year-long cache means a changed icon never reaches returning visitors. Fingerprinted `/vite/*` gets `immutable` for a year in both kits. | no (allowed) |

### Response headers

| area | Rails kit | this kit | status | fix? |
|---|---|---|---|---|
| security headers | Rails defaults: `x-frame-options: SAMEORIGIN`, `x-xss-protection: 0`, `x-content-type-options: nosniff`, `x-permitted-cross-domain-policies: none`, `referrer-policy: strict-origin-when-cross-origin`; none on public files and error pages | K:inertia/headers.rs: `DENY`, CSP with nonces, HSTS, permissions-policy, COOP, on every response | missing → fixed: `x-xss-protection: 0` and `x-permitted-cross-domain-policies: none` added. `DENY` instead of `SAMEORIGIN` and headers on public files/404s: **intentional** (stricter) | partly (allowed) |
| `x-powered-by: loco.rs` | none | K:config/*.yaml `server.ident: ""` | differs → fixed: removed | yes |
| `cache-control` on pages / redirects | `max-age=0, private, must-revalidate` / `no-cache` | K:inertia/headers.rs | differs → fixed: the same values (were absent) | yes |
| `etag` on pages | weak ETag (Rack::ETag) | none | intentional: pages carry per-request CSRF cookies and are `private, must-revalidate`; Rack::ETag renders the whole page before it can answer 304 | no (allowed) |
| `content-type` of Inertia JSON and redirects | `application/json; charset=utf-8`; redirects `text/html; charset=utf-8` | K:render.rs, redirect.rs | differs → fixed | yes |
| `vary: accept-encoding` | none (Thruster compresses in front of Rails) | Loco's compression layer | intentional: compression happens in the app here | no (allowed) |
| `server-timing` | dev only | every GET | extra (home page badge) | |

### Config and DX

| area | Rails kit | this kit | status | fix? |
|---|---|---|---|---|
| `bin/setup` (deps, db:prepare, `--reset`, clear logs/tmp, `--skip-server`, exec bin/dev) | R:bin/setup | K:bin/setup | same steps, plus a toolchain check | |
| `bin/dev` (overmind/hivemind/foreman on Procfile.dev: web + vite) | R:bin/dev, Procfile.dev | K:bin/dev (starts both itself, no process manager) | intentional: no Ruby gem to install; same two processes | |
| `bin/ci` / config/ci.rb steps | setup, rubocop, eslint, prettier, tsc, typelizer freshness, bundler-audit, npm audit, brakeman, rspec, seeds replant | setup, rustfmt, clippy, eslint, prettier, tsc, routes and prop-types freshness, cargo-deny, npm audit, cargo test, seeds, builds, Playwright | same coverage with Rust tools (rubocop→fmt+clippy, bundler-audit+brakeman→cargo-deny+clippy, rspec→cargo test + Playwright) | |
| gh-signoff comment | config/ci.rb | K:bin/ci | same | |
| `.github/workflows/ci.yml` | scan_ruby, lint_js, lint, test | rust, routes, js, security, e2e | same coverage | |
| `.github/workflows/deploy.yml` | Kamal, `if: false` | Kamal, `if: false` | same (off by default; the commented-out condition also requires a push to this repository's `main`) | |
| dependabot | bundler, github-actions, npm weekly, limit 10 | cargo, github-actions, npm | same | |
| Dockerfile | multi-stage, jemalloc, Thruster, `SSR_ENABLED` arg (default **true**), non-root 1000 | multi-stage (cargo-chef), mimalloc, no Thruster, `SSR_ENABLED` (default **false**), non-root 1000, tini, HEALTHCHECK | intentional: Thruster (compression, X-Sendfile, asset caching) is covered by the app's own layers; SSR is opt-in (commit 50d1b33: 124 MB image, no Node at runtime) | |
| Kamal `deploy.yml` | service, servers, registry, env, aliases console/shell/logs/dbc, volume, asset_path, builder cache | same keys; aliases shell/logs/dbc + migrate/dbstatus/routes | `dbc` missing → fixed: opens `sqlite3` on the database (the image now ships `sqlite3`, like the Rails image). `console`: **intentional**, Rust has no REPL; `shell` and `dbc` cover it | `dbc` yes |
| `.kamal/hooks/*.sample` | 9 samples | same 9 | same bytes | |
| `.kamal/secrets` | RAILS_MASTER_KEY | SECRET_KEY_BASE, MAILER_PASSWORD | intentional (no credentials file) | |
| README "Enabling SSR" | R:README.md:43 | K:README.md "Server-side rendering" | same content, adapted | |
| LICENSE (MIT) | R:LICENSE | K:LICENSE, K:NOTICE | same MIT text; LICENSE carries the port's copyright only (so GitHub detects MIT), NOTICE reproduces the Rails kit's license | |
| `.node-version`, `.prettierrc` | | | same bytes | |

### Tests (R:spec → this kit)

All **37 RSpec examples** have an equivalent that asserts the same status, redirect, flash,
props, errors, mail and side effects: `tests/requests/*.rs` for the request and mailer specs,
`e2e/sessions.spec.ts` (run in both CSR and SSR) for the system spec. None are missing or
weaker. On top of that, this kit has model, Inertia protocol, security, generator and
production-config tests.

### Seeds

| area | Rails kit | this kit | status |
|---|---|---|---|
| `db/seeds.rb` | empty (a comment only) | `src/fixtures/*.yaml` (two verified users, one session each) + `task seed:demo` | extra: `bin/setup` gives a login out of the box; the Rails kit's users exist only in spec/fixtures |

### PWA

| area | Rails kit | this kit | status |
|---|---|---|---|
| `app/views/pwa/manifest.json.erb`, `service-worker.js` | templates, no route | — | intentional: they are disabled in the Rails kit (commented route and link). The layout comment now says where to add them here. |

## Organizations: a deliberate addition

The Rails kit has users only. This kit ships Basecamp-style organizations in the base kit
(Cole, 2026-10-02: "every app I've ever had had them, and regretted not having them"):
`Account`, `Membership` (`owner|admin|member`), `Invitation`, pages under `/{account_slug}`, an
account switcher, and invitation mail. The code and behaviour come from an earlier app built on
this kit.

What that changes for the oracle's flows, all **intentional** and listed in
`bench/parity/allowed.json`:

| flow | Rails kit | this kit |
|---|---|---|
| sign-in success | → `/dashboard` | → `/{slug}` of the last-used account (same flash) |
| sign-up success | → `/dashboard` | → `/{slug}` of the new personal account `"<name>'s account"` (same flash, same mail) |
| `GET /dashboard`, signed in | the dashboard page | a redirect to the last-used account's overview |
| `GET /`, signed in | the home page (with a Dashboard link) | a redirect to the last-used account |
| signed-in pages | `auth` shared prop | `auth` plus the `accounts` switcher list (a **once** prop) |
| password-reset success and email-verification redirects that end on the dashboard | `/dashboard` | `/{slug}` |

| `GET /nope` (any unknown one-segment path that could be a slug), guest | 404 page | redirect to `/sign_in`: it matches `/{account_slug}`, which needs a signed-in member, as Rails' `scope ":account_slug"` with `authenticate` would. Signed in, a non-member gets the 404 page. Paths that can't be a slug (`/robots.txt`, `/a`) still 404. |
| asset tags in the HTML head | 4 `modulepreload` links | 5: the build splits `frontend/lib/browser.ts` (one line) into its own chunk |

Everything else (texts, errors, mail, sessions, settings, headers) is unchanged. Rows above that
mention `/dashboard` keep the Rails kit's path in the "Rails kit" column.

The oracle after organizations and live updates (2026-10-02, run on a Ryzen 9 9955HX workstation):

```
124 steps compared: 0 unexpected differences, 261 allowed, 0 stale allowlist entries
```

Before the four organization entries were added the same run showed 211 unexpected differences,
all in the rows of the table above. (261, not the 272 of the first organizations run: the
account overview no longer sends a `projects` placeholder.)

## Summary

137 rows: **77 same, 28 fixed** in this pass, **24 intentional**, **8 extra**. Nothing is left
open. Of the fixed rows, 5 are the "missing" kind: `allow_browser`, the 72-byte password limit,
the mailer layout, error pages on 400/406/422/500, the `dbc` alias. The other 23 were
differences.

Four Inertia-layer rows were added later (2026-10-06, ideas from inertia-omega and
inertia-laravel): one fixed (deferred scroll `mergeProps`) and three extra (concurrent lazy props,
fragment redirects, prefetches leave the flash alone). None of them shows up in the oracle's 124
steps, so `allowed.json` is unchanged; the oracle was not re-run for them.

The oracle, with production builds of both kits on one machine, 124 steps:

```
124 steps compared: 0 unexpected differences, 63 allowed, 0 stale allowlist entries
```

Before the fixes the same run found **252 unexpected differences**. The 63 allowed ones are 12
allowlist entries (`bench/parity/allowed.json`). Each is an **intentional** row above:

- csrf meta tags
- text+html mail
- the array-shaped delete-account error (Rails' string crashes its own page)
- the null-challenge check (a security bug in Rails)
- `X-Frame-Options: DENY`
- no ETag
- `Vary: accept-encoding`
- security headers on public files and 404s
- 404 caching
- public-file caching
- no account enumeration on the reset form (added 2026-09-29)
- single-use verification links (added 2026-09-29)

The last two were first matched on purpose and then fixed at Cole's request (2026-09-29): both are
security improvements over the Rails kit. That run reported **0 unexpected, 63 allowed** across
12 allowlist entries. Organizations (2026-10-02) added four entries (16 in all) and reported
**0 unexpected, 261 allowed** (see [Organizations](#organizations-a-deliberate-addition)). The
kit's own name and placeholder icon (2026-10-04) added two more (18 in all): the `<title>` on the
four HTML pages and the two icon files. Re-run on the same workstation that day: **0 unexpected, 267
allowed**, 0 stale.
