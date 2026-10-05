#!/usr/bin/env python3
"""Aggregate bench/io-run.sh output directories into the I/O tables of docs/BENCHMARK.md.

  python3 bench/summarize_io.py bench/results-io-2026-09-28/io-r*            # scenarios 1-3, writers unthrottled
  python3 bench/summarize_io.py bench/results-io-2026-09-28/io-mixed-q500-*   # scenario 3, writers capped at 500 req/s

Each scenario's table is printed only when its files are present in every run given.

Every cell is the median with the (min–max) range across runs. Exits 1 when any measured
request was not a 2xx or hit a client-side error, or when a write case's row count differs
from oha's 2xx count, so a table can never hide errors. (The tables are still printed first.)
"""
import json, statistics as st, sys
from pathlib import Path

runs = sorted(Path(p) for p in sys.argv[1:] if (Path(p) / "env.txt").exists())
if not runs:
    sys.exit("no complete runs given")
problems = []


def load(run, name):
    f = run / f"{name}.json"
    d = json.loads(f.read_text())
    codes = d.get("statusCodeDistribution", {})
    bad = {k: v for k, v in codes.items() if not k.startswith("2")}
    errs = d.get("errorDistribution", {})
    if bad or errs:
        problems.append(f"{f}: non-2xx {bad} errors {errs}")
    p = d["latencyPercentiles"]
    return {"rps": d["summary"]["requestsPerSec"], "p50": p["p50"] * 1000, "p99": p["p99"] * 1000,
            "bad": sum(bad.values()) + sum(errs.values())}


def rows_match(run, name):
    kv = dict(l.split("=", 1) for l in (run / f"{name}.rows").read_text().split())
    added, ok = int(kv["rows_added"]), int(kv["oha_2xx"])
    if added != ok:
        problems.append(f"{run}/{name}: {added} rows added but oha counted {ok} 2xx")
    return added == ok


def cell(xs, fmt="{:,.0f}"):
    return f"{fmt.format(st.median(xs))} ({fmt.format(min(xs))}–{fmt.format(max(xs))})"


def ms(xs):
    return cell(xs, "{:,.1f}") if st.median(xs) < 100 else cell(xs)


def has(name):
    return all((r / f"{name}.json").exists() for r in runs)


def row(label, name, check_rows=False):
    ds = [load(r, name) for r in runs]
    checked = ""
    if check_rows:
        ok = [rows_match(r, name) for r in runs]
        checked = f" | {sum(ok)}/{len(ok)} runs"
    print(f"| {label} | {cell([d['rps'] for d in ds])} | {ms([d['p50'] for d in ds])} "
          f"| {ms([d['p99'] for d in ds])} | {sum(d['bad'] for d in ds)}{checked} |")


env = dict(l.split("=", 1) for l in (runs[0] / "env.txt").read_text().splitlines() if "=" in l)
threads = env.get("rails_thread_sweep", "3 16 32").split()
db_threads = env.get("rails_db_threads", "3").split()
print(f"_{len(runs)} runs; each cell is median (min–max). Errors = non-2xx responses plus "
      f"client-side errors, summed over all runs._\n")

if has("upstream_direct_512"):
  print("### 1. Slow outbound call (`GET /bench/upstream`, upstream sleeps 100 ms)\n")
  print("| App, clients | req/s | p50 ms | p99 ms | errors |")
  print("|---|---:|---:|---:|---:|")
  row("Mock upstream hit directly, 512", "upstream_direct_512")
  for c in ["32", "128", "512"]:
    row(f"Rust, {c}", f"rust_upstream_c{c}")
    for t in threads:
        row(f"Rails 4×{t}, {c}", f"rails_t{t}_upstream_c{c}")

if has("rust_write_c8"):
  print("\n### 2. Concurrent SQLite writers (`POST /bench/write`)\n")
  print("| App, writers | req/s | p50 ms | p99 ms | errors | rows = 2xx |")
  print("|---|---:|---:|---:|---:|---|")
  for c in ["8", "32", "128"]:
    row(f"Rust, {c}", f"rust_write_c{c}", True)
    if has(f"rust_kit_write_c{c}"):
        row(f"Rust, before the per-connection PRAGMA fix (pre-48b412f), {c}", f"rust_kit_write_c{c}", True)
    for t in db_threads:
        row(f"Rails 4×{t}, {c}", f"rails_t{t}_write_c{c}", True)

qps = env.get("mixed_write_qps", "")
if has("rust_read_alone"):
  print(f"\n### 3. Reads while writes run (`GET /bench/read`, 32 readers; 32 writers, "
        f"{'capped at ' + qps + ' req/s' if qps else 'unthrottled'})\n")
  print("| App, case | req/s | p50 ms | p99 ms | errors | rows = 2xx |")
  print("|---|---:|---:|---:|---:|---|")
variants = [("Rust", "rust")]
if has("rust_kit_read_alone"):
    variants.append(("Rust, before the per-connection PRAGMA fix (pre-48b412f)", "rust_kit"))
variants += [(f"Rails 4×{t}", f"rails_t{t}") for t in db_threads]
for label, p in variants if has("rust_read_alone") else []:
    row(f"{label}: read alone", f"{p}_read_alone")
    row(f"{label}: read under writes", f"{p}_read_under_writes")
    row(f"{label}: the writes", f"{p}_mixed_writes", True)

loads = [float((r / "loadavg.txt").read_text().split()[0]) for r in runs if (r / "loadavg.txt").exists()]
if loads:
    print(f"\n_1-min load average at the end of each run: median {st.median(loads):.1f} "
          f"(range {min(loads):.1f}–{max(loads):.1f})._")
if problems:
    print("\n".join(problems), file=sys.stderr)
    sys.exit(1)
