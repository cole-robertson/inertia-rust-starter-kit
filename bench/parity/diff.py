#!/usr/bin/env python3
"""Compare two probe transcripts (Rails kit vs this kit) field by field.

    diff.py <rails.json> <rust.json> <allowed.json>

Every difference is printed as `step :: json.path: rails=… rust=…`. A difference whose
`step :: path` matches a pattern in allowed.json is an intentional one (each entry carries
its reason, repeated in docs/PARITY.md) and is reported as ALLOWED. Any other difference
fails the run (exit 1), as does an allowlist entry that matched nothing (a stale entry
hides nothing and should be deleted).
"""
import json
import re
import sys

rails_path, rust_path, allowed_path = sys.argv[1:4]
rails = json.load(open(rails_path, encoding="utf-8"))
rust = json.load(open(rust_path, encoding="utf-8"))
allowed = json.load(open(allowed_path, encoding="utf-8"))
patterns = [(re.compile(a["match"]), a) for a in allowed]
used = set()

diffs = []


def walk(a, b, path):
    if isinstance(a, dict) and isinstance(b, dict):
        for k in sorted(set(a) | set(b)):
            walk(a.get(k, "<absent>"), b.get(k, "<absent>"), f"{path}.{k}")
    elif isinstance(a, list) and isinstance(b, list) and len(a) == len(b):
        for i, (x, y) in enumerate(zip(a, b)):
            walk(x, y, f"{path}[{i}]")
    elif a != b:
        diffs.append((path, a, b))


by_step_rails = {s["step"]: s for s in rails}
by_step_rust = {s["step"]: s for s in rust}
for name in list(by_step_rails) + [n for n in by_step_rust if n not in by_step_rails]:
    walk(by_step_rails.get(name, "<absent>"), by_step_rust.get(name, "<absent>"), name + " ::")

failed = 0
allowed_count = 0


def short(v):
    s = json.dumps(v, ensure_ascii=False)
    return s if len(s) <= 400 else s[:400] + "…"


for path, a, b in diffs:
    hit = next((entry for rx, entry in patterns if rx.search(path)), None)
    if hit:
        used.add(hit["match"])
        allowed_count += 1
        print(f"ALLOWED  {path}\n         rails={short(a)}\n         rust ={short(b)}\n         why: {hit['reason']}")
    else:
        failed += 1
        print(f"DIFF     {path}\n         rails={short(a)}\n         rust ={short(b)}")

stale = [a["match"] for a in allowed if a["match"] not in used]
for s in stale:
    print(f"STALE    allowlist entry matched nothing: {s}")

print(f"\n{len(rails)} steps compared: {failed} unexpected differences, {allowed_count} allowed, {len(stale)} stale allowlist entries")
sys.exit(1 if failed or stale else 0)
