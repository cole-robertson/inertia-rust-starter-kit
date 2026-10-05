# bench/compiled-rails

The scripts, patches and summarised results behind [docs/RUST_VS_COMPILED_RAILS.md](../../docs/RUST_VS_COMPILED_RAILS.md).
Everything ran on a 24-thread Linux workstation (Ryzen AI 9 HX 370, performance governor)
under `~/.cache/compile-cmp/`.

| Path | What it is |
|---|---|
| `bench.sh` | The same-box benchmark. One run = every lane, in rotated order. `RUN_INDEX=0..4`. |
| `summarize.py` | `python3 summarize.py results/run-*` prints the tables in the doc. |
| `probe.sh`, `probe-all.sh` | Functional and security probe: the page component, signed-in props, CSRF, old-browser gate, and email normalization, on each lane. |
| `seed.sh` | Makes a fresh SQLite DB with the one bench user (the same bcrypt-12 digest on every lane). |
| `patches/01-kit-minimal-for-spinel.patch` | The smallest source change to the real kit that we tried in order to get Spinel past its first error. It was not enough; see the doc. |
| `patches/02-stand-in-slice.patch` | The stand-in slice: the kit cut to `/up`, `/sign_in`, `/dashboard`, with the Inertia protocol written by hand. |
| `results/run-0*/` | Raw oha JSON, per-lane CPU ticks, boot and RSS, for all 5 runs. |
| `logs/probe-slice.txt` | `probe-all.sh` output for the slice on each lane. |
| `logs/rust-lane-error-counts.txt` | `cargo build` error totals for the Roundhouse Rust lane. |

## Expected tree

```
~/.cache/compile-cmp/
  roundhouse/   rubys/roundhouse @ d0190ec3, cargo build --release
  spinel/       matz/spinel @ 585cff624 (2026.09.12-2148), make deps && make
  em-kit/       copy of /tmp/em-kit (f808193 + bench/rails-kit.patch), bundle + vite assets
  em-kit-a/     em-kit + patches/01 (git history inside)
  kit-slice/    em-kit-a + patches/02 (git history inside)
  blog/real-blog/  Roundhouse's blog fixture (Rails 8.1.4), from the 07 research scratch
  rust-kit/     this repo @ bdcac44, cargo build --release, npx vite build
  out/{em,slice,blog}-{ruby,spinel,rust}/   roundhouse --target output
```

Ruby 4.0.6 with YJIT (`RUBY_BIN`, default `~/.local/share/mise/installs/ruby/4.0.6/bin`), clang 22, Rust 1.98.1, oha 1.16.0. The app runs on CPUs 0-3,
oha on 4-7,16-19, and builds on 8-11,20-23.
