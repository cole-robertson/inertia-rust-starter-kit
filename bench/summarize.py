#!/usr/bin/env python3
"""Aggregate bench/results/<run>/ directories into medians + min–max spread.

  python3 bench/summarize.py bench/results/csr-* > table.md

Each run dir holds oha JSON per case (rust_<case>.json / rails_<case>.json) and a
summary.md with boot/RSS/sign-in rows. The throughput ratio is computed PER RUN
(Rust and Rails were measured back to back in that run) and then the median of those
paired ratios is reported, so slow drift in machine load cancels out.
"""
import json, re, statistics as st, sys
from pathlib import Path

CASES = [
    ("up", "`GET /up`"),
    ("sign_in_html", "`GET /sign_in` (HTML)"),
    ("sign_in_inertia", "`GET /sign_in` (Inertia XHR)"),
    ("dashboard_html", "signed-in page (HTML)"),
    ("dashboard_inertia", "signed-in page (Inertia XHR)"),
]
# The signed-in page is each kit's first page after sign-in: the Rails kit's `/dashboard`, and this
# kit's account overview `/{account_slug}` since 2026-10-02 (`/dashboard` before; see <run>/<app>.page).

runs = [Path(p) for p in sys.argv[1:] if (Path(p) / "summary.md").exists() or (Path(p) / "rust.meta").exists()]
if not runs:
    sys.exit("no complete runs given")


def load(run, app, case):
    d = json.loads((run / f"{app}_{case}.json").read_text())
    codes = d.get("statusCodeDistribution", {})
    bad = {k: v for k, v in codes.items() if k != "200"}
    if bad:
        sys.exit(f"{run}/{app}_{case}: non-200 responses {bad}")
    p = d["latencyPercentiles"]
    return d["summary"]["requestsPerSec"], p["p50"] * 1000, p["p99"] * 1000


def spread(xs, fmt="{:,.0f}"):
    return f"{fmt.format(st.median(xs))} ({fmt.format(min(xs))}–{fmt.format(max(xs))})"


pages = {f"{app}: {(run / f'{app}.page').read_text().strip()}" for run in runs for app in ("rust", "rails")
         if (run / f"{app}.page").exists()}
print(f"_{len(runs)} runs; each cell is median (min–max). Ratio = median of per-run Rust÷Rails._")
if pages:
    print(f"_Signed-in page: {'; '.join(sorted(pages))}._")
print()
print("| Endpoint | Rust req/s | Rails req/s | Rust ÷ Rails | p50 ms Rust / Rails | p99 ms Rust / Rails |")
print("|---|---:|---:|---:|---:|---:|")
for case, label in CASES:
    r = [load(run, "rust", case) for run in runs]
    k = [load(run, "rails", case) for run in runs]
    ratios = [a[0] / b[0] for a, b in zip(r, k)]
    print(
        f"| {label} | {spread([x[0] for x in r])} | {spread([x[0] for x in k])} "
        f"| **{st.median(ratios):.1f}×** ({min(ratios):.1f}–{max(ratios):.1f}) "
        f"| {st.median(x[1] for x in r):.2f} / {st.median(x[1] for x in k):.2f} "
        f"| {st.median(x[2] for x in r):.2f} / {st.median(x[2] for x in k):.2f} |"
    )

rows = {}
def meta(run, app):
    f = run / f"{app}.meta"
    return dict(l.split("=", 1) for l in f.read_text().split()) if f.exists() else None
for run in runs:
    r, k = meta(run, "rust"), meta(run, "rails")
    if r and k:
        for key, m in [("Boot", "boot_ms"), ("RSS idle", "idle_mib"), ("RSS after load", "peak_mib"), ("Sign-in POST", "signin_ms")]:
            rows.setdefault(key, []).append((float(r[m]), float(k[m])))
        continue
    for line in (run / "summary.md").read_text().splitlines():
        m = re.match(r"\| (Boot|RSS idle|RSS after load|Sign-in POST)[^|]*\| ([\d.]+) \w+ \| ([\d.]+) \w+ \|", line)
        if m:
            rows.setdefault(m.group(1), []).append((float(m.group(2)), float(m.group(3))))
loads = [float((run / "loadavg.txt").read_text().split()[0]) for run in runs if (run / "loadavg.txt").exists()]
print("\n| Metric | Rust | Rails |")
print("|---|---:|---:|")
units = {"Boot": "ms", "RSS idle": "MiB", "RSS after load": "MiB", "Sign-in POST": "ms"}
names = {"Boot": "Boot to first 200 on /up", "RSS idle": "Memory idle (RSS, all processes)",
         "RSS after load": "Memory after load", "Sign-in POST": "Sign-in POST (hash verify + session)"}
for key in ["Boot", "RSS idle", "RSS after load", "Sign-in POST"]:
    if key in rows:
        u = units[key]
        print(f"| {names[key]} | {spread([a for a, _ in rows[key]])} {u} | {spread([b for _, b in rows[key]])} {u} |")
if loads:
    print(f"\n_1-min load average during runs: median {st.median(loads):.1f} (range {min(loads):.1f}–{max(loads):.1f})._")
