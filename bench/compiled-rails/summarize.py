#!/usr/bin/env python3
"""Aggregate bench.sh output: python3 summarize.py results/run-*  -> markdown tables.

Each cell is median (min–max) across runs. CPU µs/req = process-tree CPU time during the
15 s measured window (utime+stime from /proc, incl. reaped children) ÷ requests served."""
import json, statistics as st, sys
from pathlib import Path

runs = [Path(p) for p in sys.argv[1:] if Path(p).is_dir()]
KIT = [("rust-kit", "Rust kit (Loco)"), ("rails-kit", "Rails kit (stock, 4×3 Puma)")]
SLICE = [("slice-rails", "Slice on stock Rails (4×3)"), ("slice-rhruby", "Slice, Roundhouse Ruby emit (4×3)"),
         ("slice-spinel", "Slice, Spinel binary (4 workers)")]
BLOG = [("blog-rails", "Blog on stock Rails (4×3)"), ("blog-rhruby", "Blog, Roundhouse Ruby emit (4×3)"),
        ("blog-spinel", "Blog, Spinel binary (4 workers)"), ("blog-rhrust", "Blog, Roundhouse Rust emit (tokio, 4)")]
CASES = [("up", "`/up`"), ("sign_in_html", "`/sign_in` HTML"), ("sign_in_inertia", "`/sign_in` XHR"),
         ("dashboard_html", "`/dashboard` HTML"), ("dashboard_inertia", "`/dashboard` XHR")]
BCASES = [("articles", "`/articles`"), ("article_1", "`/articles/1`")]

def load(run, lane, case):
    f = run / f"{lane}__{case}.json"
    if not f.exists(): return None
    d = json.loads(f.read_text())
    bad = {k: v for k, v in d.get("statusCodeDistribution", {}).items() if k != "200"}
    if bad: sys.exit(f"{f}: non-200 {bad}")
    c = dict(kv.split("=") for kv in (run / f"{lane}__{case}.cpu").read_text().split())
    n = sum(d["statusCodeDistribution"].values())
    cpu_us = int(c["cpu_ticks"]) / int(c["hz"]) * 1e6 / n
    p = d["latencyPercentiles"]
    return d["summary"]["requestsPerSec"], p["p50"] * 1000, p["p99"] * 1000, cpu_us

def cell(xs, fmt):
    return f"{fmt.format(st.median(xs))} ({fmt.format(min(xs))}–{fmt.format(max(xs))})"

def table(lanes, cases):
    print("| App | Endpoint | req/s | p50 ms | p99 ms | CPU µs/req |")
    print("|---|---|---:|---:|---:|---:|")
    for lane, label in lanes:
        for case, clabel in cases:
            xs = [x for x in (load(r, lane, case) for r in runs) if x]
            if not xs: continue
            print(f"| {label} | {clabel} | {cell([x[0] for x in xs], '{:,.0f}')} | {st.median(x[1] for x in xs):.2f} "
                  f"| {st.median(x[2] for x in xs):.2f} | {st.median(x[3] for x in xs):,.0f} |")

def meta(lanes):
    print("| App | Boot to first 200 | RSS idle | RSS after load |")
    print("|---|---:|---:|---:|")
    for lane, label in lanes:
        ms = []
        for r in runs:
            f = r / f"{lane}.meta"
            if f.exists(): ms.append(dict(kv.split("=") for kv in f.read_text().split()))
        if not ms: continue
        g = lambda k, s=1: [float(m[k]) / s for m in ms]
        print(f"| {label} | {cell(g('boot_ms'), '{:,.0f}')} ms | {cell(g('idle_kib', 1024), '{:,.0f}')} MiB "
              f"| {cell(g('peak_kib', 1024), '{:,.0f}')} MiB |")

print(f"_{len(runs)} runs; median (min–max) for req/s, medians for the rest._\n")
print("### The real kits\n"); table(KIT, CASES)
print("\n### Stand-in slice (NOT the Inertia kit: see §3)\n"); table(SLICE, CASES)
print("\n### Roundhouse blog fixture (upstream-supported reference)\n"); table(BLOG, BCASES)
print("\n### Boot and memory\n"); meta(KIT + SLICE + BLOG)
loads = [float(dict(kv.split("=") for kv in (r / f).read_text().split())["load"])
         for r in runs for f in [p.name for p in r.glob("*.meta")]]
if loads: print(f"\n_1-min load average sampled after each lane: median {st.median(loads):.1f} (range {min(loads):.1f}–{max(loads):.1f}); the app used 4 cores and oha 8._")
