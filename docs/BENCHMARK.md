# Benchmark: this kit (Loco / Rust) vs the Evil Martians Rails kit

Both kits running their **production Docker images** on one host, with the same React
frontend, SQLite, CPU pinning and method. The current numbers are from **2026-10-04 on a Ryzen 9 9955HX
workstation** (below). The earlier runs (2026-09-28/29, on a GCP VM) are kept further down as history; numbers from
the two hosts are never mixed in one table.

**Headline (client-side rendering, SSR off):** Rust serves **4.5–11× more requests per second**
with **5.4–14× lower p99 latency**, uses **5–8× less memory**, boots in **57 ms vs 1.6 s**, and its
image is **52 MB vs 207 MB** (compressed; 147 vs 531 MB unpacked). Behind a 100 ms outbound
call it serves 5,021 req/s at 512 clients vs Puma's 117 (4×3) or 1,233 (4×32). SQLite writes are
2× faster, **but 0.14–0.21% of them failed** with a 500 (pool acquire timeout, see
[the write row](#2026-10-04-io-bound-rows)); Rails had none.

## 2026-10-04 run (Ryzen 9 9955HX workstation)

### Host and method

- **Host:** a Ryzen 9 9955HX workstation (16 cores / 32 threads, two CCDs with separate L3),
  96 GB RAM, Arch Linux 7.2, Docker 29.7, storage on btrfs (zstd). Not a dedicated machine: it
  also runs CI jobs for other projects.
- **Pinning:** app containers on CPUs 0–3, `oha` 1.16 on 4–7 (I/O: `oha` 4–5, the mock upstream
  6–7), all on CCD0. Their SMT siblings (16–23) were left idle. Image builds ran on 10–15,26–31.
- **How quiet:** each run started only when the host's CI jobs were idle and the 1-minute load
  was under 2.0 for 60 s, and a run was **discarded and redone** if a CI job used more than half
  a CPU at any point during it (2 page runs were). The kept runs had a 1-minute load of 0.4–1.0
  before they started; during them it was 5.5–9.2, which is the benchmark's own 8 busy CPUs.
  Each run's `loadavg.txt` records the load after it; the busiest other processes (a desktop
  session and an idle `buildkitd`) each used under 3% of a CPU.
- **Method:** unchanged from the earlier runs (see [Setup](#setup) below): 32 connections,
  keep-alive, `--disable-compression`, 3 s warm-up + 15 s measured per case, both apps back to
  back alternating which goes first, **5 runs**, cells are median (min–max), ratios per run then
  median. Rails: Puma 4 workers × 3 threads, YJIT; Rust: one process.
- **Images:** this kit at `0079d28` (`docker build --build-arg SSR_ENABLED=false`; `irsk:io` adds
  `--build-arg CARGO_FEATURES=bench`); the Rails kit at `f808193` with `bench/rails-kit.patch`
  (pages) and `bench/rails-kit-io.patch` (I/O), SSR off. Runtime: Loco 1.2.0, axum 0.8.9,
  sea-orm 2.0.4, Rust 1.98.1, mimalloc vs Rails 8.1.4, Ruby 4.0.1, Puma 8.0.2.
- **Raw data:** `bench/results-2026-10-04/` (pages) and `bench/results-io-2026-10-04/` (I/O, plus
  `diag-write-c32/`). Driver: `bench/docker-run.sh` and `bench/io-run.sh`, as before.

**The signed-in page is equivalent, not identical.** `/dashboard` is a redirect in this kit since
organizations (2026-10-02), so the benchmark now loads each kit's first page after sign-in: the
Rails kit's `/dashboard` and this kit's account overview `/{account_slug}` (`/bench-s-account` for
the benchmark user). Both render a small shell with the shared `auth` props. Ours also looks up
the account and the membership and the account switcher's list (more queries per request), and
sends a deferred members count, so it does more work than the Rails page. `bench/docker-run.sh` finds
each page from where `GET /dashboard` ends up and records it in `<run>/<app>.page`.

### Throughput and latency

| Endpoint | Rust req/s | Rails req/s | Rust ÷ Rails | p50 ms Rust / Rails | p99 ms Rust / Rails |
|---|---:|---:|---:|---:|---:|
| `GET /up` | 94,964 (93,952–96,162) | 20,558 (20,407–21,016) | **4.6×** (4.5–4.7) | 0.34 / 1.34 | 0.58 / 4.27 |
| `GET /sign_in` (HTML) | 74,719 (74,448–75,002) | 6,797 (6,759–6,885) | **11.0×** (10.9–11.0) | 0.43 / 4.53 | 0.74 / 10.16 |
| `GET /sign_in` (Inertia XHR) | 77,684 (77,359–78,060) | 8,131 (8,125–8,182) | **9.5×** (9.5–9.6) | 0.41 / 3.77 | 0.71 / 8.47 |
| signed-in page (HTML): `/{account_slug}` vs `/dashboard` | 25,833 (25,568–25,949) | 5,211 (5,168–5,250) | **5.0×** (4.9–5.0) | 1.23 / 5.96 | 2.14 / 12.29 |
| signed-in page (Inertia XHR) | 26,435 (26,285–26,607) | 5,928 (5,858–5,949) | **4.5×** (4.4–4.5) | 1.20 / 5.32 | 2.07 / 11.18 |

All 50 result files contain only HTTP 200s (`bench/summarize.py` exits on anything else).
Absolute numbers are 3.5–6× the GCP VM's on both sides (a much faster CPU); the ratios moved
less: `/sign_in` 9.6× → 11.0×, the signed-in page 6.0× → 4.5–5.0× (it is a heavier page now).

### Resources

| Metric | This kit | Rails kit | Notes |
|---|---:|---:|---|
| Boot, `docker run` → first 200 on `/up` | **57 ms** (57–112) | 1,644 ms (1,632–1,662) | Rails: entrypoint `db:prepare` + 4 workers |
| Container memory, idle | **42 MiB** (40–44) | 210 MiB (207–225) | `docker stats` after sign-up |
| Container memory after the load run | **65 MiB** (62–72) | 509 MiB (504–518) | |
| Sign-in POST (verify password + create session) | **13 ms** (13–15) | 164 ms (163–169) | argon2id vs bcrypt cost 12: mostly the hash settings |
| Docker image, compressed (`docker image ls` content size) | **52 MB** | 207 MB | `SSR_ENABLED=false`, no Node in either |
| Docker image, unpacked (`du -sx /` in the container) | **147 MB** | 531 MB | the earlier 137 / 509 MB rows measured this |
| Cold `docker build --no-cache` | 101 s | **71 s** | one build each, buildx pinned to 12 threads (10–15,26–31), CI idle (`build-times.txt`) |
| Edit one controller → `docker build` | 23.5 s | **8.1 s** | a comment line in a controller; cargo-chef keeps the dependency layer |

### <a id="2026-10-04-io-bound-rows"></a>I/O-bound rows

`bench/io-run.sh` with only the cases the README shows (`UPSTREAM_CONC=512`,
`RAILS_THREAD_SWEEP="3 32"`, `WRITE_CONC=32`, `RAILS_DB_THREADS=3`), 5 runs. The endpoints, mock
upstream and checks are described in [I/O-bound workloads](#io-bound-workloads) below.

| Case | Rust | Rails 4×3 | Rails 4×32 |
|---|---:|---:|---:|
| Mock upstream hit directly, 512 clients (ceiling 5,120) | 5,044 req/s (5,020–5,050) | | |
| `GET /bench/upstream` (100 ms outbound call), 512 clients | **5,021** req/s (4,992–5,022), p99 105 ms | 117 (116–117), p99 4,600 ms | 1,233 (1,229–1,236), p99 471 ms |
| `POST /bench/write`, 32 writers | **11,747** req/s (9,734–12,201), p99 9.3 ms, **0.17% 500s** (0.14–0.21%) | 5,784 (5,558–5,834), p99 15.6 ms, 0 errors | |
| `GET /bench/read` alone, 32 readers | **28,305** (28,141–28,335), p99 2.2 ms | 9,284 (9,112–9,422), p99 7.8 ms | |
| `GET /bench/read` while writes run at 500/s | **23,938** (23,838–24,018), p99 2.5 ms | 8,106 (7,983–8,354), p99 8.8 ms | |

Rust now reaches the mock upstream's own ceiling (5,021 of 5,044) at 512 clients. Puma stays at
threads ÷ 0.1 s, as before. Every write case's row count equals oha's 2xx count on both sides.

**The write errors.** In every run, 264–330 of ~170,000 Rust writes (0.14–0.21%) returned 500,
as did 44 and 110 of the capped writes in the read-under-writes case of runs 1 and 4. Rails had
none. The container log names it: `Failed to acquire connection from pool: Connection pool timed
out` after exactly 500 ms, which is `database.connect_timeout` (sea-orm uses it as the pool's
acquire timeout). There is no SQLite error (`SQLITE_BUSY` or otherwise). Behind it are rare
multi-second stalls: the latency histogram has a cluster at 2.1–4.5 s while p99 is 9 ms.
`bench/io-write-diag.sh` re-ran the 32-writer case three ways (`diag-write-c32/`):

| Variant | req/s | p99 / p99.99 | 500s |
|---|---:|---:|---:|
| as shipped: pool 10, acquire timeout 500 ms | 10,410 | 9.4 / 2,434 ms | 0.20% |
| one connection (`DB_MAX_CONNECTIONS=1`) | 9,138 | 2.5 / 503 ms | 0.33% |
| acquire timeout 5 s (`DB_CONNECT_TIMEOUT=5000`) | 8,095 | 9.3 / 4,007 ms | **0** |

A longer acquire timeout turns the 500s into multi-second waits (Rails' SQLite `timeout: 5000`
behaves the same way), and a single connection doesn't remove the stalls. The Rails kit's slowest
write in the same runs took 36–89 ms.

**The stalls come from the container's 4 GB memory limit, not from SQLite or the kit.**
Timestamped (raising the acquire timeout so stalls show as latency): every ~4 s the whole pool
waits 2.1–4.0 s, with no thread in uninterruptible I/O and no SQLite error. Changing the storage
or the database does not remove them; giving the container memory or RAM-backed storage does
(stalls longer than 1 s per 20–30 s of writes):

| Variant (32 writers) | req/s | stalls > 1 s |
|---|---:|---:|
| as benchmarked: `--memory 4g`, btrfs | 7,100–7,800 | 5–7 |
| a `TRUNCATE` checkpoint every second (WAL ≤ 145 MB) | 7,500–8,000 | 5–6 |
| btrfs `nodatacow` (no copy-on-write, no compression) | 8,400 | 7 |
| `--memory 32g` | 15,000–16,800 | **0** (worst 532 ms) |
| storage on tmpfs | 17,900 | **0** |

The container's page cache stays pinned at ~1.3 GB of its 4 GB, so the WAL's dirty pages hit the
cgroup's share of the host's dirty limit (`vm.dirty_bytes` 256 MB) and the kernel throttles the
writer. Rails writes half as many rows per run (~87k vs ~180k) and stayed under it. So the error
rate is an artifact of the benchmark's memory limit plus a fast writer; what the kit owns is the
500 ms acquire timeout, which turns any multi-second I/O stall into a 500. The published row keeps
the 4 GB limit and its errors, as measured. (Probe: `bench/io-stall-diag.py`.) The 2026-09-29 run on the GCP VM had no write errors at 4,974 req/s.

**The same case at `--memory 32g`** (README row ³; `MEMORY=32g SCENARIOS=write WRITE_CONC=32
RAILS_DB_THREADS=3 bench/io-run.sh`, 5 runs, same quiet/discard rule, none discarded; results in
`bench/results-io-2026-10-04/io-io32g-*`), on kit 009e230, which adds `BEGIN IMMEDIATE` write
transactions and the 5 s production pool timeout: Rust **19,720** req/s (16,152–20,656), p50
1.1 ms, p99 9.4 ms, slowest 331–1,732 ms, **0 errors**; Rails 4×3 5,809 (5,533–5,836), p99
14.7 ms, 0 errors. Rows equal 2xx in every run. In plain terms: per-write latency is single-digit
ms; the 4 GB stalls come from the kernel throttling a container that writes ~2× Rails' rows into a
capped page cache, not from the app.

### Not re-run on this host

- **The allocator/LTO comparison, the profiling numbers, SSR, and the I/O concurrency sweeps**
  (upstream at 32/128, writes at 8/128, Rails 4×16) are not in the README table and were not
  re-run; they stay below with their dates and host.
- **Code size** is measured from the source, not a host: this kit's app code (controllers,
  models without `_entities/`, mailers, workers, auth, migrations, live updates and channels;
  `bench.rs` excluded) is 6,337 non-blank lines vs the Rails kit's 328 (`app/**/*.rb` and
  `db/migrate/`, bench files excluded). Since the earlier count (3,175 vs 472, a different file
  set), this kit gained organizations, live updates and the generators; the Rails kit has neither.

## Earlier runs (GCP e2-standard-8, 2026-09-28/29)

Everything below was measured on a dedicated GCP VM before organizations existed, when
`/dashboard` was a page in both kits. Kept for the record and for the sections that weren't
re-run.

### Setup

| | This kit | Rails kit |
|---|---|---|
| Image | `docker build .` (this repo) | `docker build .` (Rails kit) |
| Runtime | Loco 1.2 (axum 0.8, sea-orm 2), Rust 1.98.1, release build | Rails 8.1.4, Ruby 4.0.6 **with YJIT** |
| Server | 1 process (tokio multi-thread) | Puma 8, **4 workers × 3 threads** (fastest of 5 configs tried) |
| Front proxy | none | none. The kit's default CMD puts Thruster in front, which measured ~30% slower, so we benchmark bare Puma |
| Response compression | **off** | off (Puma sends identity) |
| Container limits | `--cpuset-cpus 0-3 --memory 4g` | same |

- **Host:** GCP e2-standard-8 (8 vCPU Intel Xeon @ 2.20 GHz, 31 GiB RAM, Debian 12), dedicated to this run.
- **Load generator:** [`oha`](https://github.com/hatoo/oha) 1.16 pinned to vCPUs 4–7, with 32 connections,
  keep-alive, `--disable-compression`, and a Chrome user agent (the Rails kit rejects old browsers).
- **Per case:** a 3 s warm-up, then 15 s measured.
- **Per run:** both apps measured back to back, alternating which goes first.
- **Repeats:** 5 runs per mode. Every cell is the **median with the (min–max) range** across runs.
  The ratio is computed within each run, then its median is taken.
- **Correctness:** all 100 result files (50 client-rendered from 2026-09-29, 50 SSR from 2026-09-28) contain only HTTP 200s. `bench/audit.sh` confirms that both apps
  return the same Inertia component for every case, at similar sizes (1.7–2.4 KB of HTML, 0.2–0.6 KB of JSON).
- **Images:** this kit's `docker build --build-arg SSR_ENABLED=false .` at `bdcac44`; the Rails kit's
  default build (SSR switched off at run time with `INERTIA_SSR=false`), the same image as the 2026-09-28 run.

The raw `oha` JSON for every run is committed: client-rendered in `bench/results-2026-09-29/`, SSR in
`bench/results-2026-09-28/`. Reproduce it with `bench/docker-run.sh`, adding `BENCH_SSR=true` for SSR.
Aggregate the output with `python3 bench/summarize.py bench/results/docker-*`.

#### Changes since the 2026-09-28 run

The client-rendered tables were re-run on 2026-09-29 with the same host, harness and Rails image.
Two kit changes landed in between:

- **mimalloc** as the global allocator (`9ebcdc6`, see [Allocator and release profile](#allocator-and-release-profile)).
- **Profiling fixes** (`4604928`, see `docs/PROFILING.md`): the auth layer no longer clones the whole
  `AppContext` per request, unique-key lookups no longer bind `LIMIT ?` (SQLite re-prepared every
  such statement), and the cookie keys are derived once.

| Rust, medians | 2026-09-28 | 2026-09-29 |
|---|---:|---:|
| `GET /sign_in` HTML req/s | 6,675 | 13,698 |
| `GET /dashboard` Inertia XHR req/s | 3,224 | 7,211 |
| `GET /dashboard` Inertia XHR p99 | 18.4 ms | 8.4 ms |
| Memory idle / after load | 10 / 22 MiB | 37 / 58 MiB |
| Image (`SSR_ENABLED=false`) | 124 MB | 137 MB |
| Headline: Rust ÷ Rails req/s, all five endpoints | 2.0–4.5× | 4.3–9.6× |
| Headline: p99 lower than Rails, pages (excl. `/up`) | 3.3–7× | 7.6–14× |

The Rails kit's medians are within 7% of the 2026-09-28 run (dashboard XHR 1,310 → 1,226 req/s is the
largest change), as expected for the same image. Its run 0 was the exception: every Rails case ran at about half speed (`/up` 1,640 req/s, boot
10.5 s) while Rust's run 0 was normal. The cause is unknown; it is kept in the data and shows in the
Rails ranges and the high end of the ratio ranges, not in the medians.

### Throughput and latency

| Endpoint | What it exercises | Rust req/s | Rails req/s | Rust ÷ Rails | p50 ms Rust / Rails | p99 ms Rust / Rails |
|---|---|---:|---:|---:|---:|---:|
| `GET /up` | routing only | 16,302 (15,202–16,749) | 3,880 (1,640–3,965) | **4.3×** (3.8–9.9) | 1.9 / 7.0 | 4.5 / 30.6 |
| `GET /sign_in` (HTML) | CSRF cookie, page JSON, HTML document | 13,698 (11,031–13,782) | 1,434 (754–1,486) | **9.6×** (7.4–18.3) | 2.3 / 19.7 | 4.8 / 66.9 |
| `GET /sign_in` (Inertia XHR) | the page JSON only | 14,480 (12,137–14,642) | 1,742 (914–1,842) | **8.0×** (7.0–15.9) | 2.2 / 16.5 | 4.6 / 49.6 |
| `GET /dashboard` (HTML) | signed in: session + user lookup in SQLite, shared `auth` props | 6,966 (6,487–7,104) | 1,151 (590–1,170) | **6.0×** (5.7–11.8) | 4.5 / 26.0 | 8.6 / 65.1 |
| `GET /dashboard` (Inertia XHR) | signed in, JSON | 7,211 (6,935–7,404) | 1,226 (628–1,341) | **6.0×** (5.4–11.8) | 4.4 / 24.1 | 8.4 / 64.3 |

The 1-minute load average during the runs was 8.1 (6.5–9.8), vs 8.7 (6.5–9.2) on 2026-09-28: the
benchmark itself keeps the 8 vCPUs busy.

### Resources

| Metric | This kit | Rails kit | Notes |
|---|---:|---:|---|
| Boot, `docker run` → first 200 on `/up` | **79 ms** (79–144) | 7,417 ms (7,236–14,196) | Rails: entrypoint `db:prepare` + boots 4 workers |
| Container memory, idle | **37 MiB** (35–40) | 229 MiB (197–252) | `docker stats` after sign-up. mimalloc keeps more resident than glibc malloc did (10 MiB on 2026-09-28) |
| Container memory after the load run | **58 MiB** (57–67) | 456 MiB (451–502) | 22 MiB on 2026-09-28, before mimalloc |
| Sign-in POST (verify password + create session) | **49 ms** | 390 ms | argon2id (19 MiB, t=2) vs bcrypt cost 12. This is mostly the hash settings, not the language |
| Docker image (`SSR_ENABLED=false` build) | **137 MB** | 509 MB | no Node in either. 124 MB before mimalloc and the Server-Timing badge |
| Docker image (default build, includes Node for SSR) | 266 MB | 706 MB | measured 2026-09-28, not rebuilt since |
| Cold `docker build --no-cache` (8 vCPU) | 351 s | **83 s** (4 CPUs) | Rust compiles ~540 crates (≈1,600 CPU-s, ~95% of it dependencies); Ruby installs prebuilt gems |
| Edit one controller → `docker build` | 60 s | seconds | cargo-chef keeps the dependency layer; only our 2 crates recompile |
| Edit one controller → `cargo check` / debug build / release build | 1.6 s / 4.0 s / 54 s | **~30 ms** (code reload) | 8 vCPU. See [Build times](#build-times) |

### Build times

Measured on the same 8-vCPU host with `cargo build --timings`, Rust 1.98.1.

| Where the cold release build goes | CPU seconds |
|---|---:|
| `libsqlite3-sys` build script (compiles SQLite from C; `bundled` feature) | 127 |
| `zstd-sys` build script (compression codec for tower-http, via Loco) | 70 |
| `sqlx-postgres` (**Loco hardcodes Postgres support even in SQLite apps**) | 67 |
| `loco-rs`, `tera` (×2 versions), `sea-orm`, `rustls`, `regex-automata` | ~270 |
| This app (`inertia_rust_starter_kit` lib + bin + `migration`) | ~80 (5%) |
| Everything, 543 compilation units | ~1,600 |

What was fixed, and what's inherent:

- **Fixed: the Docker build compiled every dependency twice.** `rust-toolchain.toml` pinned 1.98.1 while
  the image used 1.95, so rustup swapped toolchains after `COPY . .` and discarded cargo-chef's cooked
  layer. Cold build 661 s → 351 s; one-line edit 383 s → 60 s. The Dockerfile now fails fast if the two
  versions ever disagree.
- **Fixed: dev builds carried full debug info for 540 dependencies** (a 410 MB binary). Line tables for
  our code only: rebuild after an edit 5.8 s → 4.0 s, binary 103 MB, backtraces still show `file:line`.
- **Not fixable here:** Postgres, zstd, Tera, opendal, lettre and the cron scheduler come from Loco's
  own dependency choices. Only a feature-flag change upstream in Loco removes them.
- **Inherent:** a compiled language rebuilds and relinks on every edit. The fast loop is `cargo check`
  (1.6 s) and rust-analyzer while writing code, with a rebuild only to run it.

### Allocator and release profile

Measured on the same host with the page benchmark harness (`bench/docker-run.sh` settings: CSR,
compression off, 4 CPUs, oha with 32 connections, 15 s). Three images were built from the same commit.
Each ran 2 rounds, rotating which went first, so each cell below lists both runs.

| Image | `GET /sign_in` req/s | `GET /dashboard` req/s | dashboard p99 | Memory idle / after load | Cold build / one-file rebuild |
|---|---:|---:|---:|---:|---:|
| glibc malloc, default release profile | 6,713 / 6,533 | 3,070 / 3,010 | 19 ms | 10 / 21 MiB | 402 s / 58 s |
| **mimalloc** (adopted) | **10,666 / 11,380** | **4,605 / 4,585** | **13 ms** | 38–40 / 57–62 MiB | 339 s / 58 s |
| `lto = "thin"`, `codegen-units = 1` | 7,063 / 6,899 | 3,249 / 2,918 | 18–24 ms | 10–13 / 21–22 MiB | 442 s / **140 s** |

- **mimalloc: +50–66% throughput and about a third lower p99**, for one small C dependency and
  ~40 MiB more resident memory (still 6–8× under the Rails kit). The kit now uses it
  (`src/bin/main.rs`). Each image was built once with `--no-cache`, so treat the cold-build column as
  ±1 minute. mimalloc building faster than the base is noise, not an effect.
- **Thin LTO + 1 codegen unit: +3–6% on sign-in, within noise on the dashboard**, but the one-file
  rebuild of the image goes from 58 s to 140 s. It isn't adopted. The image shrinks 132 → 119 MB.
- These are 2-round runs, so treat the ranges as indicative. The headline tables above were re-run
  with mimalloc and the profiling fixes on 2026-09-29 (5 runs; see
  [Changes since the 2026-09-28 run](#changes-since-the-2026-09-28-run)).
- A first attempt at this run left SSR on, and all three images rendered at half speed (base 3,546
  sign-in req/s). It was discarded. The ratios came out the same (mimalloc ×1.9 sign-in, ×1.6
  dashboard; LTO ×1.04).

### Code and tests

| Metric | This kit | Rails kit | Notes |
|---|---:|---:|---|
| App code (controllers, models, mailers, auth, migrations) | 3,175 lines | **472 lines** | non-blank lines. The Rust count includes the rate limiter, atomic updates and the mail worker |
| Inertia server adapter | 4,894 lines, in this repo | 3,218 lines, in the `inertia_rails` gem | |
| Frontend (excluding shadcn `ui/` and generated routes) | 2,439 lines | 2,385 lines | same app |
| Backend tests | 223 tests / 5,264 lines | 35 examples / 425 lines | plus 20 Playwright runs (10 tests × CSR/SSR) vs 1 system spec |
| Test suite time (warm build) | 9.4 s | **4.6 s** | |
| Runtime dependencies | 380 crates | 147 gems | npm dependencies are identical |

### I/O-bound workloads

The tables above are CPU-bound: every request does framework work and nothing waits. Real apps
spend much of their time *waiting*: on outbound HTTP calls (payment, storage or LLM APIs) and on
SQLite writes. These three scenarios measure that on the same two production images, host and
container limits.

#### Method

- **Endpoints.** Both apps get the same three benchmark-only, unauthenticated endpoints. Each returns a tiny `{"ok":true,…}`
  JSON body:
  - `GET /bench/upstream` makes one HTTP GET to a mock upstream that sleeps 100 ms. Rails uses
    stdlib `Net::HTTP` in a Puma thread; Rust uses one shared `reqwest` client.
  - `POST /bench/write` inserts one `bench_events` row with a 200-byte payload through the
    normal ORM and pool (Active Record / sea-orm).
  - `GET /bench/read` returns the latest 20 rows plus `COUNT(*)`.

  Code: `src/controllers/bench.rs` behind the cargo feature `bench`, and `bench/rails-kit-io.patch`
  (a `BenchController`, routes, a migration) on top of `bench/rails-kit.patch`.
- **Not in the shipped kit.** The feature is off by default. The default release binary contains none of the
  bench strings (`strings | grep` finds 0 matches, vs 55 with `--features bench`). The image is built with
  `--build-arg CARGO_FEATURES=bench --build-arg SSR_ENABLED=false`.
- **CSRF.** `/bench/*` skips only the CSRF check: `skip_forgery_protection` in Rails, a
  `/bench/` exemption in `inertia::csrf` in the kit. Every other layer (logging, session cookie, headers, auth
  lookup) runs as in production. `tests/requests/bench.rs` runs with forgery protection on and
  checks that `/bench/write` passes without a token while `POST /sign_in` is still refused.
- **Mock upstream.** `bench/upstream` is a Go `net/http` server, pinned to CPUs 6–7. Hit directly with 512
  clients, it serves **4,971 req/s at p99 114 ms**. The ceiling is 512 ÷ 0.1 s = 5,120 req/s, so it
  is not the bottleneck for any row below. Apps run on CPUs 0–3 and oha on 4–5. Containers
  use host networking, so both apps reach the upstream over the same loopback.
- **Checks.** Every response is checked: all 245 oha files of the 2026-09-28 batches (4.96 M responses) and all 90 of the
  2026-09-29 batch (2.89 M) contain only 2xx and no client errors. After every write case the harness counts rows in the app's SQLite file with
  `sqlite3` and compares them with oha's 2xx count. They match in **all 160** write measurements
  (2026-09-28: 60 in scenario 2, 40 in scenario 3's two batches; 2026-09-29: 45 and 15). `bench/summarize_io.py` exits non-zero on any mismatch or error.
- **Runs.** 5 runs; within each run, Rust and Rails go one after the other, alternating which goes first. Each case gets a 3 s warm-up, then 15 s
  measured with `oha -w` (requests in flight at the deadline finish and are counted). Each write
  case starts on a fresh database. Cells are the **median (min–max)**.
- **Rails config.** Puma 4 workers × 3 threads, the best config from the page benchmark. For the upstream case also 4×16
  and 4×32, because "add threads" is Puma's answer to I/O. For writes and mixed also 4×16. The
  Active Record pool follows `RAILS_MAX_THREADS`, so each thread has a connection.
- **Two dates.** Scenarios 2 and 3 were re-run on 2026-09-29 (`SCENARIOS="write mixed"`, 5 runs,
  image `irsk:io` built from `bdcac44`, the same `emkit:io` patches), after mimalloc and the profiling
  fixes, one of which removes a re-prepared `LIMIT ?` from every unique-key lookup. Scenario 1 and the
  pre-48b412f rows are from 2026-09-28 and were not re-run. Old → new, Rust medians: writes at 32
  writers 3,688 → 4,974 req/s (p99 38.6 → 37.8 ms); reads under writes 2,143 → 3,585 req/s (p99 25.5 →
  16.1 ms); read alone 3,254 → 5,845. The Rails medians moved by 12% or less (read alone 1,815 → 2,025 at 4×3 is the largest).
- **Reproduce.** `bench/io-run.sh`, then `python3 bench/summarize_io.py bench/results/io-*`. The raw
  results are in `bench/results-io-2026-09-29/` (scenarios 2 and 3) and `bench/results-io-2026-09-28/`
  (everything, including scenario 1).

#### SQLite settings (and a bug the benchmark found)

Both kits end up with Rails 8's SQLite settings on every connection: WAL, `synchronous=NORMAL`,
`busy_timeout` 5 s (Rails: `timeout: 5000`), `mmap_size` 128 MB, `cache_size` 2000,
`journal_size_limit` 64 MB, `foreign_keys` on. Rails applies them in `configure_connection` for each new connection.

Loco 1.2 runs the same PRAGMA block **once**, through the pool, so it lands on one connection.
`journal_mode=WAL` is stored in the file, but `synchronous`, `cache_size` and `mmap_size` are
per connection. A probe of 6 pooled connections in the pre-fix kit found 1 at `synchronous=NORMAL` and 5 at
SQLite's default `FULL`. In WAL mode, `FULL` fsyncs the WAL on every commit. On this disk, a
single autocommit insert took **2,020 µs with FULL vs 96 µs with NORMAL** (sqlite3 CLI, 500
inserts each).

The kit now opens every pooled connection with these settings (`src/db.rs`, commit 48b412f), so
the **"Rust" rows are the kit as it ships today**. The rows labelled "before the per-connection PRAGMA fix (pre-48b412f)"
came from an image built before that commit and are kept for comparison. Of every result here, this is the
one that was a plain bug.

#### 1. Slow outbound call (`GET /bench/upstream`, upstream sleeps 100 ms)

Measured 2026-09-28, before mimalloc and the profiling fixes; not re-run.

| App, clients | req/s | p50 ms | p99 ms | errors |
|---|---:|---:|---:|---:|
| Mock upstream hit directly, 512 | 4,971 (4,958–4,995) | 101 (101–101) | 114 (107–119) | 0 |
| Rust, 32 | **312** (308–313) | 102 (102–103) | 105 (104–110) | 0 |
| Rails 4×3, 32 | 110 (110–111) | 315 (265–318) | 466 (445–534) | 0 |
| Rails 4×16, 32 | 302 (291–305) | 104 (104–107) | 120 (113–142) | 0 |
| Rails 4×32, 32 | 304 (290–305) | 104 (104–108) | 117 (114–135) | 0 |
| Rust, 128 | **1,243** (1,137–1,246) | 102 (102–110) | 108 (107–143) | 0 |
| Rails 4×3, 128 | 113 (111–114) | 1,131 (1,123–1,132) | 1,190 (1,186–1,298) | 0 |
| Rails 4×16, 128 | 596 (532–599) | 217 (208–249) | 295 (260–398) | 0 |
| Rails 4×32, 128 | 1,113 (752–1,158) | 110 (109–168) | 143 (134–234) | 0 |
| Rust, 512 | **3,067** (1,356–3,098) | 165 (163–355) | 219 (216–705) | 0 |
| Rails 4×3, 512 | 113 (109–113) | 4,448 (4,435–4,597) | 4,745 (4,706–5,102) | 0 |
| Rails 4×16, 512 | 596 (540–597) | 852 (848–950) | 922 (912–1,152) | 0 |
| Rails 4×32, 512 | 1,151 (967–1,155) | 439 (438–483) | 539 (512–889) | 0 |

**The mechanism: a waiting request holds a Puma thread.** Throughput is capped at
in-flight slots ÷ time per request (Little's law). Rails has 4 × T threads, and each one is busy for at least 100 ms:

| Puma | Slots | Ceiling = slots ÷ 0.1 s | Measured (512 clients) | p50 at 512 clients ≈ 512 ÷ measured |
|---|---:|---:|---:|---:|
| 4×3 | 12 | 120 req/s | 113 | 512 ÷ 113 = 4.5 s (measured 4.4 s) |
| 4×16 | 64 | 640 req/s | 596 | 512 ÷ 596 = 0.86 s (measured 0.85 s) |
| 4×32 | 128 | 1,280 req/s | 1,151 | 512 ÷ 1,151 = 0.44 s (measured 0.44 s) |

With fewer clients than slots, Rails matches Rust. At 32 clients, 4×16 (64 slots) serves
302 req/s vs Rust's 312, which is the client limit 32 ÷ 0.1 s = 320. At 128 clients, 4×32 serves
1,113 vs 1,243. A tokio task parked on a socket costs a few KB and no thread, so Rust has no
slot limit. It tracks clients ÷ 0.1 s up to 128 clients (1,243 of 1,280) and reaches 3,067 at 512.
That is 60% of the 5,120 ceiling; the gap is CPU. At 512 clients, the 4 app CPUs spend their time on 3,000 proxied round trips
per second (accept, one outbound HTTP request, parse its JSON, render JSON). That is why p50 rises
to 165 ms.

"Add threads" works until CPU, memory or the database pool stops it. Each Puma thread
holds an Active Record connection and a Ruby stack. 4×32 is 128 threads, and this app can't use them
for anything but waiting. The GVL means they don't add CPU throughput (compare writes at 4×16 below).
Rust's run 0 at 512 clients (1,356 req/s, p99 705 ms) is an outlier against the other four
(3,060–3,098). Every Rust case in run 0 was ~2.3× slower, while its Rails cases were normal.
The cause is unknown. That run went Rust-first, but so did runs 2 and 4, which were normal. It is
kept in the data, and the ranges show it.

#### 2. Concurrent SQLite writers (`POST /bench/write`)

| App, writers | req/s | p50 ms | p99 ms | errors | rows = 2xx |
|---|---:|---:|---:|---:|---|
| Rust, 8 | **4,287** (3,950–4,315) | 0.7 (0.7–0.7) | 19.1 (19.1–19.3) | 0 | 5/5 runs |
| Rails 4×3, 8 | 1,179 (745–1,270) | 5.5 (5.2–7.3) | 24.1 (20.5–45.0) | 0 | 5/5 runs |
| Rails 4×16, 8 | 1,155 (1,070–1,266) | 5.5 (5.3–5.8) | 25.0 (20.8–30.9) | 0 | 5/5 runs |
| Rust, before the per-connection PRAGMA fix (pre-48b412f, 2026-09-28), 8 | 361 (333–370) | 3.9 (3.8–4.7) | 333 (333–432) | 0 | 5/5 runs |
| Rust, 32 | **4,974** (3,961–5,890) | 5.2 (4.5–6.4) | 37.8 (23.3–40.1) | 0 | 5/5 runs |
| Rails 4×3, 32 | 1,296 (787–1,396) | 23.2 (21.5–38.2) | 53.5 (51.3–107.2) | 0 | 5/5 runs |
| Rails 4×16, 32 | 1,290 (1,279–1,335) | 21.2 (20.9–21.5) | 81.7 (76.6–89.1) | 0 | 5/5 runs |
| Rust, before the per-connection PRAGMA fix (pre-48b412f, 2026-09-28), 32 | 371 (346–385) | 62.2 (60.2–66.5) | 685 (595–697) | 0 | 5/5 runs |
| Rust, 128 | **4,254** (4,094–6,083) | 29.6 (20.5–30.0) | 63.6 (40.4–63.7) | 0 | 5/5 runs |
| Rails 4×3, 128 | 1,279 (778–1,373) | 98.8 (89.5–162.5) | 171 (144–315) | 0 | 5/5 runs |
| Rails 4×16, 128 | 1,318 (1,269–1,385) | 84.3 (79.3–87.0) | 291 (272–306) | 0 | 5/5 runs |
| Rust, before the per-connection PRAGMA fix (pre-48b412f, 2026-09-28), 128 | 369 (322–409) | 323 (289–375) | 846 (815–986) | 0 | 5/5 runs |

**The mechanism: SQLite has one writer, so throughput is 1 ÷ (time holding the write lock).** Both
apps serialize on the same lock. Neither produced a single `SQLITE_BUSY` or 5xx at 128 writers:
the 5 s busy timeout absorbs the queue on both sides.

- **Rust, ~4,300 req/s**: 1 ÷ 4,300 = 230 µs of lock time per write. The insert plus
  commit is about 100 µs (the NORMAL figure above); the rest is the sea-orm/sqlx round trip while the lock is held. Adding writers past 8 changes nothing but queueing. At 128 writers,
  p50 ≈ 128 ÷ 4,254 = 30 ms (measured 29.6).
- **Rails, ~1,300 req/s** at every thread count. If all 4 cores were busy, that is up to
  4 ÷ 1,300 ≈ 3 ms of CPU per write, about 10× the lock hold time. So the likely limit is framework CPU, not
  the lock. This is an inference: CPU utilization was not recorded. It fits the fact that 4×16 is no faster
  than 4×3, and that its p99 is worse (more threads contending for the GVL and the lock).
- **Before the fix, ~370 req/s** at every concurrency. Five of the six pooled connections
  fsynced each commit at about 2 ms, and a single lock holder makes that 1 ÷ 2.7 ms ≈ 370/s.
  The kit's own re-measure after 48b412f agreed with the 2026-09-28 "Rust" rows (3,598 / 3,688 / 3,384
  req/s at 8 / 32 / 128 writers), which were measured with the same settings. It used the pre-fix image vs the fixed image, 3 alternating pairs, scenarios 2
  and 3 only, and reports medians with (min–max). Writers: c8 374 → 2,840 req/s (p99 333 → 21 ms),
  c32 396 → 3,576 (590 → 40 ms), c128 388 → 3,374 (864 → 66 ms). Reads under writes: 280 → 2,268 req/s
  (p99 150 → 24 ms). The fixed image's first run was the low end of every range (c32 2,059, host load
  5.6); the other two were 3,576 and 3,918. Every write's row count matched oha's 2xx count.
- The Rust ranges at 32 and 128 writers are bimodal: runs 2 and 4 reached ~5,900–6,100 req/s, runs
  0, 1 and 3 ~4,000–5,000. Rails 4×3 had two slow runs (0 and 2, ~780 req/s at 32 and 128 writers)
  while its 4×16 cases in the same runs were normal. Neither cause is known; all runs are kept.

#### 3. Reads while writes run (`GET /bench/read`, 32 readers, 32 writers)

The read does `COUNT(*)` plus the latest 20 rows, so its cost grows with the table. If each app wrote
as fast as it could, the faster writer would grow its table faster and its own reads would scan more rows.
That's not equal work: in the first batch, Rust's table ended at ~55,000 rows vs Rails' ~28,000. So
the committed table **caps the writers at 500 req/s on both apps** (`oha -q 500`). Both
tables start at 1,001 rows and end at exactly 13,002 in every run (`bench/results-io-2026-09-29/`, and
`bench/results-io-2026-09-28/io-mixed-q500-*` for the pre-48b412f rows).

| App, case | req/s | p50 ms | p99 ms | errors | rows = 2xx |
|---|---:|---:|---:|---:|---|
| Rust: read alone | **5,845** (5,801–5,885) | 5.3 (5.3–5.4) | 9.9 (9.7–9.9) | 0 | |
| Rust: read under writes | **3,585** (3,430–3,763) | 8.7 (8.3–9.2) | 16.1 (15.1–16.3) | 0 | |
| Rust: the writes | 500 (500–500) | 4.7 (4.5–4.8) | 11.7 (11.5–12.7) | 0 | 5/5 runs |
| Rails 4×3: read alone | 2,025 (953–2,100) | 15.2 (14.9–29.5) | 27.4 (24.4–94.5) | 0 | |
| Rails 4×3: read under writes | 1,310 (499–1,331) | 23.5 (23.2–61.4) | 44.7 (42.2–142.2) | 0 | |
| Rails 4×3: the writes | 500 (500–500) | 16.0 (13.3–40.0) | 38.1 (35.5–123.6) | 0 | 5/5 runs |
| Rails 4×16: read alone | 1,540 (1,189–1,588) | 20.7 (19.8–26.0) | 34.8 (33.2–52.4) | 0 | |
| Rails 4×16: read under writes | 922 (711–969) | 34.4 (33.2–41.9) | 64.1 (60.7–117.8) | 0 | |
| Rails 4×16: the writes | 500 (500–500) | 5.8 (4.4–7.4) | 21.5 (20.9–38.7) | 0 | 5/5 runs |
| Rust, pre-48b412f (2026-09-28): read alone | 3,150 (3,106–3,269) | 10.0 (9.6–10.2) | 18.1 (17.3–18.5) | 0 | |
| Rust, pre-48b412f (2026-09-28): read under writes | 264 (234–292) | 121 (108–131) | 156 (142–335) | 0 | |
| Rust, pre-48b412f (2026-09-28): the writes | 419 (365–450) | 59.7 (56.0–67.4) | 431 (385–512) | 0 | 5/5 runs |

**The mechanism: WAL lets readers run beside the writer, so the writes cost CPU, not blocking.**
Both apps lose a third or more of their read throughput under 500 writes/s: Rust 5,845 → 3,585 (−39%), Rails 4×3 2,025 → 1,310 (−35%). No read waits for the lock. The loss is the
CPU the 500 writes/s take from the same 4 cores, plus larger `COUNT(*)` scans as the table grows
from 1,001 to 13,002 rows. Rust reads 2.7–2.9× faster, alone and under writes, with about a third of the p99.
Rails 4×3's run 0 was slow throughout (953 alone, 499 under writes); the other four runs span
1,917–2,100 and 1,305–1,331. More Puma threads again make it worse (4×16: 922 req/s, p99 64 ms).

Before the fix, reads under writes collapsed to 264 req/s and the capped writers couldn't even keep up
with 500/s (419). The likely cause: the read handler needs a pooled connection, and the pool
(10 connections) was mostly held by writes, each fsyncing for ~2 ms. Pool wait time was not instrumented.

<details><summary>The first batch: writers unthrottled (not equal work, kept for the record)</summary>

With writers unthrottled, Rust wrote 2,231 req/s vs Rails 4×3's 1,117. Its reads fell to 810
(p99 64 ms) vs Rails' 793 (p99 71 ms), but Rust's table was twice as large and took twice the write
load. Full tables: `python3 bench/summarize_io.py bench/results-io-2026-09-28/io-r*`.

</details>

#### What is inherent and what is tunable

- **Inherent to the model: outbound waits.** Puma's ceiling is threads ÷ wait time, and every thread holds a
  DB connection, a stack and GVL turns. You can raise it (4×32 gets within 10% of Rust up to 128
  clients), but you size the pool for the worst upstream latency, and a slow upstream day
  moves the ceiling. A Rust task that waits holds nothing. Rails can get the same shape
  without threads by moving the call into a job (Solid Queue) or with an async server (Falcon);
  neither was measured here.
- **Tunable: SQLite writes.** Both kits hit the same single-writer lock. The 3.3–3.8× gap
  is most likely framework CPU per request (at most ~0.9 ms of core time per write for Rust, 4 ÷ 4,300,
  vs ~3 ms for Rails; CPU utilization was not recorded), the same kind of gap as the
  page benchmark, not a concurrency-model difference. The kit's 10× pre-fix *loss* was a
  configuration bug, now fixed.
- **Not tunable by threads: Rails writes and reads.** 4×16 was never faster than 4×3 for
  SQLite work, and its p99 was always worse.
- **Postgres** would remove the single-writer lock and change scenarios 2 and 3 entirely. It was not
  measured.

### Caveats

- **Rate limiting.** This kit rate-limits credential POSTs (10 per 3 min per IP); the Rails kit
  doesn't. The sign-in timing stays under that limit.
- **Only one machine type.** Absolute numbers depend on hardware. On an 8-core Ryzen laptop
  (a noisier run, not shown here) the ratios came out the same or larger.
- **SQLite on both sides.** Postgres would shift the absolute numbers, but not what dominates
  them: framework overhead per request.

### Bottom line

- **Throughput and tail latency:** 6.0–9.6× the requests per second and 7.6–14× lower p99 without SSR.
- **I/O:** waiting on an outbound call costs Rust nothing, while each Puma thread holds a slot. Rails
  catches up by adding threads until clients outnumber them (4×32: 1,151 vs 3,067 req/s at 512 clients).
  SQLite writes are ~3.8× faster in Rust, and reads under writes ~2.7×; both apps hit the same
  single-writer lock with no errors.
- **Memory and boot:** about 6–8× less memory, and boot in under 100 ms. This is the biggest
  operational difference: many instances fit on a small VPS, and restarts are effectively instant.
- **SSR (optional):** if you enable it, the Node renderer becomes the bottleneck for both kits and the
  gap on server-rendered pages mostly disappears (see the appendix).
- **Where Rails wins:**
  - about 7× less app code, and the Inertia adapter comes as a gem;
  - instant code reload, vs ~4 s debug rebuilds;
  - ~4× faster cold image builds;
  - a much larger ecosystem.

### Appendix: server-side rendering

Only relevant if you run with SSR. Both kits ship with SSR turned on by default (the Docker
build arg `SSR_ENABLED=true`, plus the app's SSR setting); build with `--build-arg SSR_ENABLED=false` and
set `SSR_ENABLED=false` (this kit) or `config.ssr_enabled = false` (Rails kit) to run client-rendered, as
in the main tables above.

Both kits render through the same `@inertiajs/react/server` Node process (one process,
single-threaded). Five runs, SSR timeouts matched (see the note below). Inertia XHR visits never go
through SSR, so they match the table above.

| Endpoint | Rust req/s | Rails req/s | Rust ÷ Rails | p50 ms Rust / Rails | p99 ms Rust / Rails |
|---|---:|---:|---:|---:|---:|
| `GET /sign_in` (HTML, SSR) | 640 (632–687) | 426 (388–460) | **1.5×** (1.4–1.7) | 35 / 68 | 140 / 151 |
| `GET /dashboard` (HTML, SSR, signed in) | 241 (220–253) | 224 (218–230) | **1.1×** (1.0–1.1) | 97 / 123 | **430 / 375** |
| `GET /dashboard` (Inertia XHR) | 3,213 (2,987–3,256) | 1,271 (1,145–1,300) | **2.5×** (2.4–2.7) | 10 / 23 | 18 / 63 |

**With SSR, Node is the bottleneck, not the web framework.** Throughput on SSR pages is nearly equal.
Rust's p99 is *worse* on the SSR dashboard, for a structural reason. Rust passes all 32 concurrent
requests straight to the single renderer, so they queue inside Node. Rails can only have
4 workers × 3 threads = 12 in flight, so its queue is shorter. If you run SSR under load, scale
the renderer (several Node processes behind the SSR URL) or cap concurrent SSR calls.

| Metric, SSR on | This kit | Rails kit |
|---|---:|---:|
| Container memory, idle (includes the Node renderer) | **48 MiB** (48–50) | 250 MiB (243–267) |
| Container memory after the load run | **159 MiB** (137–179) | 546 MiB (542–551) |

SSR timeout: this kit ships a 1.5 s SSR timeout that falls back to client rendering. For these runs
it was raised to 60 s (`SSR_TIMEOUT_MS=60000`) to match Rails' `Net::HTTP` default. Otherwise an
overloaded renderer would let Rust skip some SSR work.
