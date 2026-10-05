# Rust (this kit) vs compiled Rails (Roundhouse + Spinel)

**The question.** Is it better to write the app in Rust, or to keep Rails and add a compile step?
To find out, we compared this kit with the original Evil Martians inertia-rails React starter kit,
compiled with Roundhouse (Sam Ruby) and Spinel (Matz).

**Answer, 2026-09-29:**
- **The Evil Martians kit does not compile to a working app on any Roundhouse lane today.**
  - The Ruby lane emits and boots, but every page is a 500 or an empty 204.
  - The Spinel lane refuses to build.
  - The Rust lane fails `cargo build` with 200 errors.
- **The Inertia layer is the blocker**: `inertia_rails`, the `inertia` route, `inertia_share` and
  implicit render are not modeled. So are three model DSLs the kit's auth relies on.
- **A cut-down stand-in does compile and run: this is not the kit.** It keeps the kit's
  `/up`, `/sign_in` and `/dashboard`, and writes the Inertia protocol by hand.
  - The Spinel binary gets within **1.1–1.8×** of this Rust kit, at **less memory**
    (29 vs 68 MiB idle).
  - It also **drops** behavior while still compiling: email normalization (with a warning), plus the old-browser gate, two email validators and two auth callbacks (with no diagnostic).
  - Its signed-in pages are held back by a slow Base64 decode in the Roundhouse runtime.
- **Recommendation:** for a new Inertia + React app you want to run fast today, the Rust kit is the
  working option. Section 7 explains why.

Everything below was measured on one machine in one session, and the stand-in is labelled
wherever it appears.

## 1. Summary table

Throughput is req/s, median (min–max) of 5 runs. Every number in this document was measured on
the same 24-thread Linux workstation in one session. §2 has the method.

| | Rust kit (Loco) | Rails kit (stock) | Slice, stock Rails | Slice, Roundhouse Ruby emit | Slice, Spinel binary |
|---|---:|---:|---:|---:|---:|
| **Is it the kit?** | yes (the port) | yes | **stand-in** | **stand-in** | **stand-in** |
| `GET /sign_in` HTML, req/s | 69,898 (59,061–80,167) | 4,435 (3,328–5,301) | 6,403 (4,130–6,839) | 23,770 (21,952–31,637) | 58,434 (45,374–69,816) |
| `GET /dashboard` HTML (signed in), req/s | 30,793 (22,961–37,762) | 2,972 (2,805–3,735) | 4,593 (3,797–4,924) | 4,116 (3,018–5,738) | 17,170 (15,826–21,176) |
| `GET /dashboard` Inertia XHR, req/s | 33,533 (13,570–41,509) | 3,522 (3,242–4,381) | 6,581 (4,393–7,067) | 4,517 (3,208–6,570) | 17,292 (16,484–21,168) |
| `/dashboard` XHR p50 / p99 ms | 0.94 / 1.77 | 7.94 / 28.91 | 4.07 / 16.59 | 5.61 / 24.50 | 1.55 / 5.12 |
| CPU µs per `/dashboard` XHR | 114 | 1,107 | 595 | 865 | 183 |
| Boot to first 200 | 32 ms | 1,063 ms | 1,027 ms | 295 ms | 8 ms |
| RSS idle / after load | 68 / 67 MiB | 627 / 874 MiB | 602 / 879 MiB | 231 / 376 MiB | 29 / 54 MiB |
| Cold build of the server | 4 m 18 s (release, 8 threads) | `bundle install` 37 s | same as Rails | emit 0.17 s + bundle | emit 0.17 s + `spin build` 32 s |
| One-controller edit → runnable | 48 s release; `cargo check` 0.8 s warm | reload, ~0 | reload | re-emit 0.17 s + restart | 33 s |
| Server artifact | 46 MB static-ish binary (libc, libgcc, libm) | Ruby + 194 MB of gems | same | Ruby + 63 MB of gems + 2.1 MB emitted tree | 2.0 MB binary + libsqlite3, libjemalloc, libcrypt |
| CSRF check on POST | yes | yes | yes | yes | yes |
| `allow_browser versions: :modern` (406 for old UAs) | not ported (the kit has no browser gate) | yes | yes | **dropped, 200** | **dropped, 200** |
| `normalizes :email` (sign in with `BENCH@EXAMPLE.COM`) | yes | yes | yes | **dropped, sign-in fails** | **dropped, sign-in fails** |

Per-run paired ratios (the median of each run's ratio):

| Comparison | `/sign_in` HTML | `/dashboard` HTML | `/dashboard` XHR |
|---|---:|---:|---:|
| Rust kit ÷ Rails kit (both real) | **15.1×** (13.7–22.8) | **10.4×** (7.9–10.7) | **9.5×** (3.9–9.7) |
| Slice Spinel ÷ slice stock Rails | 10.5× (7.1–14.1) | 4.2× (3.4–4.5) | 2.9× (2.6–3.9) |
| Slice Roundhouse Ruby ÷ slice stock Rails | 4.8× (3.4–5.7) | — | 0.9× (0.5–1.0) |
| Rust kit ÷ slice Spinel | 1.1× (1.1–1.5) | 1.8× (1.1–2.4) | 1.6× (0.8–2.3) |

The Rust ÷ Rails ratios are larger here than in [BENCHMARK.md](BENCHMARK.md) (6.0–9.6×). This run is on
bare metal with no Docker, on four 5.1 GHz Zen 5 cores, and oha gets 8 logical CPUs. Rust scales with
the faster cores, while Puma 4×3 stays at about 3–4k req/s. Treat these ratios as belonging to this
box, and BENCHMARK.md as the dedicated-VM reference.

## 2. Method

- **Machine.** a 24-thread Linux workstation: AMD Ryzen AI 9 HX 370, 12 cores / 24 threads, 93 GB, Arch Linux 7.2,
  `performance` governor. It is a desktop with a session open, not a dedicated host: the 1-min load
  average after each lane was 9.1 median (4.5–14.1), and most of that is the benchmark itself.
- **Pinning.** App on logical CPUs 0–3 (Zen 5, 5.16 GHz). oha on 4–7 and 16–19. Builds on 8–11 and 20–23.
  - I ran four timing builds on the build cores during runs 0–1. They did not share cores with
    the app or oha, but they did share memory bandwidth and the L3.
  - Spinel sizes its worker pool from `sysconf(_SC_NPROCESSORS_ONLN)`, which returns 24 under
    `taskset -c 0-3` (checked). So its lanes run with `SPINEL_WORKERS=4`.
  - tokio sizes its pool from the affinity mask, so it gets 4 without a setting.
- **Same method as BENCHMARK.md otherwise:**
  - production mode, response compression off (`--disable-compression`), and a Chrome user agent;
  - oha 1.16, 32 connections, 3 s warm-up, then 15 s measured;
  - 5 runs, with lane order rotated each run and reversed on odd runs;
  - every oha file contains only HTTP 200s, 0 errors (`summarize.py` exits on anything else).
  - **No Docker:** everything runs natively from `~/.cache/compile-cmp`.
- **Servers.**
  - The Rust kit is the release binary, CSR, with its mimalloc build.
  - Every Ruby lane runs Puma with `WEB_CONCURRENCY=4 RAILS_MAX_THREADS=3`, BENCHMARK.md's best
    config, on Ruby 4.0.6 with YJIT. That includes the Roundhouse Ruby emit, which ships its own
    `config/puma.rb`.
  - Spinel: `SPINEL_WORKERS=4`, one process (its README says prefork is not ready, roundhouse#79).
  - The Roundhouse Rust emit uses axum/tokio.
- **Signed-in cookie.**
  - The Rust kit and the Rails kit: a fresh sign-up per lane.
  - Slice lanes: a real CSRF-carrying sign-in POST for a user seeded with the same bcrypt-12 digest
    on every lane.
  - Before each lane is measured, a signed-in `/dashboard` must return 200.
- **CPU µs/req.** utime+stime of the server's whole process tree, read from `/proc/<pid>/stat`
  around the 15 s window, divided by requests served.
- **RSS.** The sum over the process tree, idle after sign-in and again after the last case.
- **Boot.** Launch to the first 200 (`/up`, or `/articles` for the blog). It excludes `db:prepare`
  and seeding, which BENCHMARK.md's 7 s Docker figure includes.
- **Versions.**

| Component | Version |
|---|---|
| Roundhouse | rubys/roundhouse `d0190ec3` (2026-09-29), release 2026.9.18 + 45 commits since the 07 report |
| Spinel | matz/spinel `585cff624` (`2026.09.12-2148`, 2026-09-29), 1,074 commits since the 07 report |
| Rails kit | inertia-rails/react-starter-kit `f808193` + `bench/rails-kit.patch`, Rails 8.1.4, inertia_rails 3.22.0 |
| Rust kit | this repo `bdcac44`. The app source is byte-identical to `origin/main` (`6aac4c1`); only docs and bench files changed. Loco 1.2, Rust 1.98.1 |
| Blog fixture | Roundhouse `fixtures/real-blog` (Rails 8.1.4), generated 2026-09-26 |
| Toolchain | Ruby 4.0.6 + YJIT (mise), clang 22.1.8 (Spinel `CC=clang`), sqlite 3.53.4, jemalloc 5.3.1 |

- **Reproduce.** See [bench/compiled-rails/README.md](../bench/compiled-rails/README.md): `bench.sh` for
  `RUN_INDEX` 0–4, then `python3 summarize.py results/run-*`. The raw oha JSON is in
  `bench/compiled-rails/results/`.

## 3. What compiled and what didn't

### 3a. The actual kit, as is

`roundhouse check` (strict) stops at the first construct:

```
roundhouse-check: ingest failed: unsupported construct in em-kit/config/routes.rb: unsupported routes DSL: `inertia`
```

`roundhouse check --continue` takes 18 ms and reports:
`0 parse error(s), 1 error(s), 19 warning(s), 3 gap-attributed note(s), 4 survey gap(s)`.
- **Gems:** 4 unknown: `authentication-zero, inertia_rails, rails_vite, typelizer`.
- **The error:** `application_controller.rb:24:25: error[send_dispatch_failed]: no known method 'find_by_id' on Session`.
  This is the dynamic finder the kit uses for its session lookup.
- **Survey gaps:** `inertia` (routes), `inertia_share`, `inertia_config`, and `defined?(Capybara::Lockstep)`
  in the layout. That last one also leaves `vite_tags` and `inertia_ssr_head` untyped.

The emit also reports what it drops. Every item below is a warning, not an error:

```
user.rb:6:3:  warning[unsupported]: generates_token_for not supported (all targets): model DSL call on `User` not lowered
user.rb:10:3: warning[unsupported]: generates_token_for not supported (all targets): model DSL call on `User` not lowered
user.rb:20:3: warning[unsupported]: normalizes not supported (all targets): model DSL call on `User` not lowered
application_mailer.rb: `default` and `layout` ... not modelled and is dropped
```

Several things vanish with no diagnostic at all:
- `allow_browser versions: :modern`. Roundhouse has the runtime gate and uses it on its blog, but the
  kit's gate isn't wired into the emitted controller.
- `before_validation if: :email_changed?` (which clears `verified`).
- `after_update if: :password_digest_previously_changed?` (which logs out other sessions on a
  password change).
- the `uniqueness:` and `format:` validators on `email`.

The emitted `User#validate` has only presence and password length
(`out/em-ruby/app/models/user.rb:328`).

What the kit's auth needs, against Roundhouse's coverage table:

| Feature | Status on Roundhouse |
|---|---|
| `has_secure_password`, `authenticate_by` | modeled, real bcrypt |
| `Current` attributes | modeled |
| `cookies.signed.permanent` | modeled |
| `generates_token_for` / `find_by_token_for!` | **not modeled** (email verification, password reset) |
| `normalizes` | **not modeled** |
| conditional model callbacks (`if:` + `on: :update`) | **dropped silently here** |
| `find_by_id` | **not modeled** (`find_by(id:)` is) |
| `inertia_rails` (render, share, route DSL, errors / `clear_history` redirects) | **not modeled**. `render inertia:` passes through as text |

**(a) Roundhouse Ruby lane**, with `--survey --allow-unsupported`: 463 files, 0.2 s, exit 0.
- Every emitted file parses, and Puma boots.
- **`/up` returns 404.** The route is emitted, but the `rails/health` controller has no arm in the
  dispatcher.
- **`/sign_in`, `/dashboard` and `/` return 500:** `NoMethodError: undefined method 'ip' for an
  instance of ActionDispatch::Request`. `set_current_request_details` calls `request.ip`; the
  runtime has `remote_ip` only.
- As a diagnostic, I patched `ip` in a copy. The next failure is
  `NoMethodError: undefined method 'find_by_id' for class Session`.
- With that rewritten too, `/sign_in` returns **204 with an empty body**. The emitted `new_action` is
  `head(:no_content)`: `inertia_config default_render: true` is a survey gap, so the implicit
  Inertia render becomes Roundhouse's "no template" default.
- `POST /sign_up` then 500s: `undefined method 'permit' for an instance of Hash`
  (`params.permit(...)` without `require`).

**(b) Spinel lane**: 469 files emitted; `spin build` fails in 18 s:

```
spinel: method 'session=' param 'value' has unsupported type nil
spin: build failed
```

`Current.session ||= Session.find_by_id(...)` types `session` as `nil`, because `find_by_id` is
unresolved and `Current.session = nil` is the only other writer. So the generated `current.rbs` says
`def session=: (nil value) -> nil`.

Path (i) was to fix the kit minimally in a copy (`patches/01-kit-minimal-for-spinel.patch`):
- `find_by(id:)` instead of `find_by_id`;
- an explicit `sig/current.rbs`. Roundhouse did not apply it to the instance-side writer;
- `||=` rewritten as `if nil?`.

With those, the analysis is clean, and Spinel gets further and stops with 3 refusals:

```
spinel: .../identity/email_verifications_controller.rb:20: undefined method 'find_by_token_for!' for a Class: no class in the program defines a class method 'find_by_token_for!' (NoMethodError)
spinel: .../identity/password_resets_controller.rb:26: ... (NoMethodError)
spinel: .../identity/password_resets_controller.rb:45: ... (NoMethodError)
spinel: 3 refusals, nothing written
```

Past this point you would need a `generates_token_for` implementation, and then an Inertia runtime
for every page.

**(c) Rust lane.** Roundhouse does have a Rust target: the guide lists 13, including
`typescript-worker`.
- The kit emits 61 files, and `cargo build --release` fails with **200 errors**.
  - Missing modules: `crate::importmap`, and `controllers::rails::health_controller` for `/up`.
  - Namespaced controllers emitted as `Identity::…` and `Settings::…` paths that don't exist.
  - `User::new` missing `password` / `password_confirmation` bindings.
  - `!` applied to a `serde_json::Value`.
  - And more (error totals in `logs/rust-lane-error-counts.txt`).
- The Rust lane also has **no CSRF verification**. The docs scope enforcement to "the ruby family",
  and roundhouse#23 is still open.

**Is there a model-a-gem mechanism?** Partly:
- `sig/**/*.rbs` can type things the analyzer can't infer;
- a class that extends an unknown gem's base keeps its body.

There is no plugin API for a gem's controller DSL, route DSL or render path. Facades are compiled
into Roundhouse (`src/facades.rs`), so modeling `inertia_rails` means patching Roundhouse itself.
This comparison doesn't patch Roundhouse, so path (i) stops here.

### 3b. The stand-in slice: what does compile

Path (ii) is `patches/02-stand-in-slice.patch`: **the kit cut down to the benchmarked pages, with
Inertia written by hand.**
- **Routes:** `/up`, `GET/POST /sign_in`, `/dashboard`. The kit's `SessionsController`, `Session`,
  `User` and `Current` are kept, and so is the schema.
- **`InertiaController`** builds the kit's exact page object: component, `errors`, `auth.user`,
  `auth.session`, `url`, `version`, `encryptHistory`, `sharedProps`.
  - An XHR gets JSON.
  - A full visit gets the kit's layout with the `<script data-page="app">` element.
- **`ApplicationController`** now writes `protect_from_forgery with: :exception`. Roundhouse does not
  apply Rails' implicit default, and I wanted CSRF enforced on every lane.
- **Layout:** `vite_tags` is replaced by the literal tags it printed, so the same built bundle loads
  unchanged.
- **Removed:** `generates_token_for`, the mailers, and the settings, identity, users and home
  controllers.

Four more changes were needed only to get past emit and runtime bugs. Each is a legal Rails
rewrite, and each Roundhouse gap is noted:
- **`render` inside the action, not a helper.** Roundhouse appends `head(:no_content)` to any action
  whose body lacks a literal `render`.
- **`render plain: json_string, content_type:`, not `render json: hash`.** On Spinel, `JsonRender.encode`
  of a nested Hash 500s with `NoMethodError: undefined method 'encode' for unknown`.
- **`Session.create!(user: user)`, not `user.sessions.create!`.** The association reader returns a
  plain Array (`undefined method 'create!' for an instance of Array`).
- **A local variable, not `@session`.** An ivar named `@session` in a controller clobbers the
  framework's session (`undefined method 'to_cookie' for an instance of Session`).
- **Timestamps formatted with `.utc.iso8601(3)`.** Otherwise the Ruby emit prints `2026-09-29 09:17:57 +0000`,
  and Spinel prints `#<:0x00007f426014e600>`.

The result: `roundhouse check`, strict, is clean:
`0 parse error(s), 0 error(s), 1 warning(s), 0 survey gap(s)`, 0 unknown gems.
- The strict emit succeeds on all three lanes: Ruby 310 files, Spinel 316, Rust 55.
- **Ruby lane:** runs.
- **Spinel:** builds in 32 s (clang) or 34 s (gcc) into a 2.0 MB binary.
- **Rust lane:** still fails `cargo build`, with **79 errors** (`logs/rust-lane-error-counts.txt`). So there
  is no stand-in number for Roundhouse → Rust; only the blog fixture below has one.

Even strict mode lets `normalizes` through with only a warning:
`roundhouse: kit-slice/app/models/user.rb:12:3: warning[unsupported]: normalizes not supported`.

The same-page check (`probe.sh`, output in `logs/probe-slice.txt`):
- The page JSON from both compiled lanes is structurally identical to stock Rails, apart from row
  ids and timestamps.
- The HTML differs only in the CSRF token (Rails masks it; the emit sends the raw session token)
  and whitespace: 1,688 vs 1,731 bytes for `/sign_in`, 1,841 vs 1,884 for `/dashboard`.
- The slice on stock Rails serves the same bytes as the real Rails kit, give or take the
  `data-inertia` head attribute. It is therefore the same work, minus inertia_rails' own overhead.
  Per-run medians, slice on stock Rails ÷ the real Rails kit: `/sign_in` HTML 1.25×, `/sign_in` XHR
  1.66×, `/dashboard` HTML 1.30×, `/dashboard` XHR 1.58×. `/up` is 0.83×: the slice renders a view,
  and the kit uses Rails' built-in health controller.

### 3c. Roundhouse's own blog fixture (the upstream-supported reference)

`check` is clean. All three lanes emit and build:
- Rust lane: `cargo build --release` in 1 m 21 s;
- Spinel: 34 s.

On this box:

| Blog, `/articles` | req/s | ÷ stock Rails | p99 ms | CPU µs/req | RSS idle / loaded |
|---|---:|---:|---:|---:|---:|
| Stock Rails 8.1.4, 4×3 | 2,760 (2,272–2,781) | 1× | 28.35 | 1,428 | 691 / 849 MiB |
| Roundhouse Ruby emit, 4×3 | 26,984 (17,716–28,405) | **9.9×** | 3.39 | 142 | 246 / 342 MiB |
| Spinel binary, 4 workers | 50,045 (28,326–52,111) | **18.3×** | 2.12 | 62 | 29 / 61 MiB |
| Roundhouse Rust emit, tokio | 74,715 (56,080–79,994) | **27.1×** | 0.75 | 49 | 9 / 11 MiB |

`/articles/1`: 2,534 / 27,520 / 57,119 / 113,704 req/s. This reproduces the 07 report's ~11×
Ruby-emit result on a different box. It is the upside when your app sits entirely inside the
supported subset.

## 4. The measurements in full

Median (min–max) req/s over 5 runs; the other columns are medians.

### The real kits

| App | Endpoint | req/s | p50 ms | p99 ms | CPU µs/req |
|---|---|---:|---:|---:|---:|
| Rust kit (Loco) | `/up` | 95,808 (71,557–100,465) | 0.33 | 0.59 | 38 |
| Rust kit (Loco) | `/sign_in` HTML | 69,898 (59,061–80,167) | 0.43 | 0.89 | 51 |
| Rust kit (Loco) | `/sign_in` XHR | 75,923 (66,291–84,577) | 0.38 | 0.83 | 49 |
| Rust kit (Loco) | `/dashboard` HTML | 30,793 (22,961–37,762) | 1.03 | 1.93 | 124 |
| Rust kit (Loco) | `/dashboard` XHR | 33,533 (13,570–41,509) | 0.94 | 1.77 | 114 |
| Rails kit (stock, 4×3 Puma) | `/up` | 14,891 (13,606–17,911) | 1.81 | 7.64 | 258 |
| Rails kit (stock, 4×3 Puma) | `/sign_in` HTML | 4,435 (3,328–5,301) | 6.09 | 19.55 | 821 |
| Rails kit (stock, 4×3 Puma) | `/sign_in` XHR | 4,984 (4,372–6,342) | 5.04 | 24.14 | 779 |
| Rails kit (stock, 4×3 Puma) | `/dashboard` HTML | 2,972 (2,805–3,735) | 9.03 | 32.70 | 1,317 |
| Rails kit (stock, 4×3 Puma) | `/dashboard` XHR | 3,522 (3,242–4,381) | 7.94 | 28.91 | 1,107 |

### Stand-in slice (NOT the Inertia kit, see §3b)

| App | Endpoint | req/s | p50 ms | p99 ms | CPU µs/req |
|---|---|---:|---:|---:|---:|
| Slice on stock Rails (4×3) | `/up` | 13,844 (11,680–14,810) | 1.79 | 10.25 | 279 |
| Slice on stock Rails (4×3) | `/sign_in` HTML | 6,403 (4,130–6,839) | 4.15 | 17.16 | 610 |
| Slice on stock Rails (4×3) | `/sign_in` XHR | 9,955 (7,852–10,676) | 2.33 | 14.94 | 390 |
| Slice on stock Rails (4×3) | `/dashboard` HTML | 4,593 (3,797–4,924) | 6.20 | 19.57 | 853 |
| Slice on stock Rails (4×3) | `/dashboard` XHR | 6,581 (4,393–7,067) | 4.07 | 16.59 | 595 |
| Slice, Roundhouse Ruby emit (4×3) | `/up` | 68,591 (52,136–72,341) | 0.40 | 1.49 | 43 |
| Slice, Roundhouse Ruby emit (4×3) | `/sign_in` HTML | 23,770 (21,952–31,637) | 1.13 | 3.43 | 122 |
| Slice, Roundhouse Ruby emit (4×3) | `/sign_in` XHR | 53,921 (45,664–63,860) | 0.54 | 1.67 | 55 |
| Slice, Roundhouse Ruby emit (4×3) | `/dashboard` HTML | 4,116 (3,018–5,738) | 6.90 | 24.98 | 942 |
| Slice, Roundhouse Ruby emit (4×3) | `/dashboard` XHR | 4,517 (3,208–6,570) | 5.61 | 24.50 | 865 |
| Slice, Spinel binary (4 workers) | `/up` | 111,652 (108,260–139,097) | 0.26 | 0.93 | 24 |
| Slice, Spinel binary (4 workers) | `/sign_in` HTML | 58,434 (45,374–69,816) | 0.45 | 3.10 | 48 |
| Slice, Spinel binary (4 workers) | `/sign_in` XHR | 87,055 (67,988–109,646) | 0.33 | 1.35 | 32 |
| Slice, Spinel binary (4 workers) | `/dashboard` HTML | 17,170 (15,826–21,176) | 1.60 | 5.09 | 187 |
| Slice, Spinel binary (4 workers) | `/dashboard` XHR | 17,292 (16,484–21,168) | 1.55 | 5.12 | 183 |

### Boot and memory

| App | Boot to first 200 | RSS idle | RSS after load |
|---|---:|---:|---:|
| Rust kit (Loco) | 32 (31–61) ms | 68 (68–71) MiB | 67 (57–71) MiB |
| Rails kit (stock, 4×3 Puma) | 1,063 (1,059–1,073) ms | 627 (606–627) MiB | 874 (864–896) MiB |
| Slice on stock Rails (4×3) | 1,027 (1,023–1,045) ms | 602 (578–622) MiB | 879 (839–906) MiB |
| Slice, Roundhouse Ruby emit (4×3) | 295 (293–309) ms | 231 (229–235) MiB | 376 (367–401) MiB |
| Slice, Spinel binary (4 workers) | 8 (7–8) ms | 29 (29–31) MiB | 54 (52–56) MiB |
| Blog on stock Rails (4×3) | 1,119 (1,117–1,429) ms | 691 (690–693) MiB | 849 (846–859) MiB |
| Blog, Roundhouse Ruby emit (4×3) | 457 (408–472) ms | 246 (246–247) MiB | 342 (338–344) MiB |
| Blog, Spinel binary (4 workers) | 41 (39–44) ms | 29 (29–31) MiB | 61 (58–62) MiB |
| Blog, Roundhouse Rust emit (tokio, 4) | 7 (6–8) ms | 9 (9–9) MiB | 11 (11–11) MiB |

### Why the compiled slice's signed-in pages are slow

On the Roundhouse Ruby emit:
- signed-out `/sign_in` XHR: 55 µs;
- the signed-in `/dashboard` XHR: 865 µs, **slower than stock Rails' 595 µs**.

The cause is found and measured:
- **Where the time goes.** Verifying the signed session cookie runs the runtime's pure-Ruby
  `Base64.strict_decode64`. Its `char_value` does a linear 64-step scan of the alphabet for every
  character (`runtime/base64.rb:86`). A TracePoint count on one request shows 10,535 `String#[]`
  calls from that one method.
- **The measured cost.** In-process with Rack::Test on one core
  (`bench/compiled-rails/dashboard-inproc.rb`), one `/dashboard` costs **987 µs as emitted and
  266 µs** when `char_value` is replaced by a Hash lookup. That test change was diagnostic only and
  was not benchmarked.
- **Spinel carries the same decoder** (`out/slice-spinel/runtime/base64.rb`). That is likely why its
  dashboard is 3–5× slower than its `/sign_in`, while the Rust kit's is 2.3×. I didn't profile
  Spinel: `perf` isn't installed on that host.

This is a Roundhouse runtime bug, not a limit of compiling, and it is probably a one-line fix
upstream. The blog fixture never shows it, because it has no signed cookies. As shipped today,
though, it costs every signed-in page. The compiled lanes' dashboard numbers therefore
**understate** what they would do after that fix. I didn't measure the fixed build under oha,
so I don't estimate it.

### Other measured costs

| | Rust kit | Roundhouse → Spinel (slice) |
|---|---:|---:|
| Analyze + emit | n/a | check 14 ms; emit 0.17 s (Ruby, Spinel), 0.05 s (Rust) |
| Cold compile of the server | 4 m 18 s wall, 31 m 43 s CPU (release, 8 threads) | 32 s wall, single-threaded (`spin build`, clang) |
| Edit one controller → new binary | 48 s release build | 33 s (re-emit + full `spin build`, no incremental) |
| Type-check loop | `cargo check` 0.8 s warm | `roundhouse check` 14 ms |
| Roundhouse's own build (once) | | 2 m 24 s; Spinel 1 m 28 s |
| Emitted code size | | 130 app lines → 27.9k Ruby / 33.5k Spinel-Ruby / 6.1k Rust, incl. runtime |

## 5. The non-performance comparison

**Correctness and security coverage**

Measured with `probe.sh` on the stand-in slice unless noted.

| | Rust kit | Roundhouse Ruby / Spinel | Evidence |
|---|---|---|---|
| CSRF verified on POST | yes (header token, Origin check) | **yes now**, but only if the app *writes* `protect_from_forgery`. Rails' implicit default is not applied, so the real kit, which relies on it, would be **unprotected**. Tokens are unmasked. | Probe: POST without a token gets 422 on all lanes. `rails-coverage.md` §Security posture. The 07 report's "never verified" dates from before 2026-09-27 (commit `09567dd7`). |
| CSRF on Roundhouse's Rust lane | n/a | **not verified**; roundhouse#23 is open | `docs/guide/rails-coverage.md` |
| Session store (the framework `_session` cookie, which holds the CSRF token and flash) | the kit's own; its `session_token` cookie is signed, as the Rails kit's is | **signed, not encrypted** (Rails encrypts it); the client can read it | `rails-coverage.md` |
| `allow_browser` 406 gate | not ported to the Rust kit either | **dropped silently**: an MSIE 6 UA gets 200 | probe |
| `normalizes :email` | yes | **dropped with a warning**: `BENCH@EXAMPLE.COM` can't sign in, and a mixed-case sign-up would create a second account | probe; emit log |
| `uniqueness` / `format` email validators, conditional callbacks | yes | **dropped silently** (emitted `User#validate`) | `out/em-ruby/app/models/user.rb:328` |
| Email verification / password reset tokens | yes | **not modeled**; Spinel refuses to build | §3a |
| Masked CSRF tokens (BREACH) | n/a | not masked | `rails-coverage.md` |

**Which Rails features and gems survive.**
- **Survive:** the kit's Active Record basics, `has_secure_password`, `authenticate_by`,
  `Current`, signed permanent cookies, `before_action` / `skip_before_action` with `only:`,
  redirects with flash.
- **Do not:** `inertia_rails` (the whole reason the kit exists), `rails_vite` (`vite_tags`),
  `typelizer` (a build-time tool, harmless), `generates_token_for`, `normalizes`, mailer
  `default` / `layout`, `find_by_id`, `params.permit` on a bare Hash, `association.create!`,
  `render json:` of a nested Hash on Spinel, and timestamp JSON encoding on Spinel.
- **The census:** Roundhouse models 13 gems in total.

**Dev loop.**
- **Rails:** code reload, about 0 s.
- **Roundhouse's Ruby lane:** re-emit (0.17 s) plus a Puma restart. In practice you would develop
  on stock Rails and emit for production. Nothing tells you the two diverge until you run the
  compare oracle.
- **Spinel:** 33 s for every edit, since there is no incremental build.
- **The Rust kit:** `cargo check` 0.8 s, a 48 s release build, and a ~4 s debug build (BENCHMARK.md).
- **Where errors surface:**
  - Rust surfaces its errors at compile time, in your own code.
  - Roundhouse surfaces them in three places: its own analysis, Spinel's type inference (errors
    point into *emitted* files, e.g. `out/a-spinel/app/controllers/identity/...rb:20`), and at
    runtime in emitted code.

**Debugging.**
- **Ruby emit:** a 500 is a normal Ruby exception with a backtrace into the emitted tree
  (`runtime/*.rb`, `app/controllers/*.rb`). Those files are readable and follow the source
  file-for-file, but they are not your source.
- **Spinel:** a 500 logs class and message only, e.g.
  `[react_starter_kit] 500 GET /sign_in -- NoMethodError: undefined method 'encode' for unknown`.
  Spinel's `docs/limitations.md` says `Exception#backtrace` and `Kernel#caller` return `[]`.
  `spin build --debug` gives a `-O0` binary for gdb/lldb, over generated C.
- **Rust:** panics and errors carry `file:line` in your own code.

**Deploy artifact.**
- **Spinel:** a 2.0 MB binary plus `libsqlite3`, `libjemalloc` and `libcrypt`, the `public/`
  assets, and a writable `storage/`. `spin pack` makes a C-only build tree, and Roundhouse's
  Campfire image is ~160 MB.
- **Rust kit:** a 46 MB binary that needs only libc, libgcc and libm, plus `public/`. Its Docker
  image is 137 MB (BENCHMARK.md).
- **The Ruby emit:** needs Ruby plus 63 MB of gems, against stock Rails' 194 MB.

**Maturity**, as of 2026-09-29:

| | Roundhouse | Spinel | Loco (this kit's framework) |
|---|---|---|---|
| First commit | 2026-04-17 | 2026-03-16 | 2022 |
| Releases | one: 2026.9.18 | one: 2026.09.12 | 1.x, semver |
| Commits / top author | 3,032 / Sam Ruby 2,968 (98%) | 9,845 / Matz 7,582 (77%) | many contributors |
| Claude co-authored commits | 2,895 of 3,032 | 6,733+ (07 report) | — |
| Open issues | 66, three filed today (#163–165: an `ActionController::API` app 500s; an app with no views or no root route fails to build) | ~46 open issues+PRs (it triages to zero daily) | — |
| Pace since 2026-09-26 | 47 commits in 3 days | 1,074 commits in 3 days | — |
| Stated scope | "Rails as notation"; blog = all targets, Campfire = Ruby/Spinel | Ruby subset AOT compiler | Rails-like framework for Rust |

- **Bus factor:** effectively 1 for each compiler. Both are moving very fast, and the upside of that
  pace is visible: CSRF enforcement, signed cookies and signed flash all landed on 2026-09-27.
- **Pin stability:** the 07 report found that conformance holds only at a pinned app commit.

## 6. What this does and doesn't show

- **Measured:** every req/s, latency, CPU, RSS, boot and build number above, on one box, in one session.
  The same-page correctness probe, and every compile, emit, boot and 500 error quoted.
- **Stand-in, not the kit:** every "slice" row. It does the kit's work for those 5 requests
  (same session lookup, same page JSON, same HTML shell), minus inertia_rails.
- **Not measured:**
  - Roundhouse → Rust on the kit or the slice: both fail to build. It is measured on the blog only.
  - The effect of fixing the Base64 decoder under load.
  - SSR, and the I/O scenarios.
  - Anything on a dedicated VM: this is a shared desktop, and its 1-min load was 4.5–14 during the runs.
  - A Spinel profile (no `perf`).
- **One outlier:** the Rust kit's dashboard XHR in run 4 is 13,570 req/s against 24.5–41.5k in the
  other runs. That lane ran last-but-one in that run's order. No cause was found; it is kept in
  the data.

## 7. Recommendation

**Is writing the app in Rust better than keeping Rails and adding a compile step?**

For this kit, and for anything built on Inertia today: **yes, Rust is the only one of the two that
works.**

The compile step works on a Rails subset: scaffold-shaped apps like the blog, and Campfire on the
Ruby and Spinel lanes. The Evil Martians kit sits just outside that subset, and its main
dependency, inertia_rails, is not modeled at all. After ~12 hand edits and deleting half the app,
the compiled slice comes within 1.1–1.8× of the Rust kit, uses half its memory, and boots in 8 ms.
That is an impressive result. But the same run silently changed the app's auth behavior in four
places, and the Rust lane of the same compiler didn't build.

- **The pattern to watch for is "compiled, but silently lost a behavior".** The kit lost
  normalization, the browser gate, two validators and two callbacks, and none of them errored.
  Transpiling an existing app would need Roundhouse's compare oracle plus the app's own test suite
  as the gate.
- **Transpiling is not an increment.** Roundhouse is whole-program: there is no way to compile
  "just the hot controllers" and keep the rest on Rails.
- **Use this Rust kit** where a new, small, performance- or memory-sensitive service is worth a
  second stack: edge devices, a high-fan-out I/O service. The I/O numbers in BENCHMARK.md are the
  argument for that, more than raw req/s.
- **Re-check later.**
  - Both compilers moved by dozens to a thousand commits in 3 days.
  - The things to look for: `inertia_rails` in the gem census, `generates_token_for` / `normalizes`
    lowered, Rails' implicit `protect_from_forgery` default applied, and the Rust lane building the
    slice.
  - If all four land, re-run `bench/compiled-rails/bench.sh` against the unmodified kit.
