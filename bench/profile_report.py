#!/usr/bin/env python3
"""Summarize folded stacks from `PROF_CTL=1 bench/profile.sh` (docs/PROFILING.md).

    bench/profile_report.py OUT/<path>.folded ...          # cost buckets + top-N self/inclusive
    bench/profile_report.py --short DIR OUT/*.folded       # also DIR/<path>.folded, compact, for
                                                           # `inferno-flamegraph --minwidth 0.3`

Frame names are shortened (generic arguments and closure environments dropped), so a flamegraph
of the short stacks is a few hundred KB instead of hundreds of MB.
"""
import argparse
import os
import re
from collections import Counter

# A sample is charged to the FIRST bucket whose pattern matches any frame of its stack. pprof names
# a frame by its short function name plus generic arguments (`fetch_one<...sqlx...>`), so most
# buckets match on the whole frame. Tracing is the exception: tower-http's trace layer wraps the
# whole service stack, so every `drop_glue<MapFuture<...Trace...>>` mentions it; that bucket only
# counts frames whose first generic argument is a tracing type, plus our span builder.
BUCKETS = [
    ("argon2 (password hash)", r"argon2|blake2"),
    ("AppContext clone (axum state, extractors)", r"^(clone<loco_rs::app::AppContext>|"
     r"<loco_rs::app::AppContext as core::clone::Clone>|clone<[^,]*AuthState>|"
     r"clone<axum::handler::service::HandlerService<)"),
    ("SQLite / sqlx / sea-orm", r"sqlx|sqlite3|sea_orm"),
    ("cookie keys, HMAC, AES", r"derive_key|cookie::|hmac|sha2|aes|_mm_sha256"),
    ("tracing / request log", r"^[^<]*<(tracing|tracing_core|tracing_subscriber)::|"
     r"^(make_span|redact_uri)\b|RedactedSpan as"),
    ("page JSON + HTML", r"serde_json::ser|document::|script_safe_json|vite::|to_json"),
    ("props resolve", r"inertia::resolver|inertia::props"),
]
# Allocator self time: the leaf frame only (allocations inside the buckets above stay there).
MIMALLOC = ("mimalloc (self time, outside the buckets above)", re.compile(r"^_?mi_[a-z_0-9]*$"))


def head(frame: str) -> str:
    """`<a::B as c::D>::f` and `f<T, U>` -> the path without generic arguments."""
    out, depth = [], 0
    for ch in frame:
        if ch == "<":
            depth += 1
        elif ch == ">":
            depth = max(0, depth - 1)
        elif depth == 0:
            out.append(ch)
    return "".join(out)


# Runtime/plumbing frames hidden from the inclusive table (they are ~100% everywhere).
PLUMBING = re.compile(r"^(std|core|alloc|tokio|hyper|tower|futures|__rust|call_once|poll|run|thread_start|::new::thread_start|"
                      r"catch_unwind|do_call|\{closure|\{async|block_on|with|enter|set|scoped|"
                      r"spawn|try_|budget|thread_start|call$|clone$)")


def short(frame: str) -> str:
    """`clone<axum::Foo<Bar<Baz>>, X>` -> `clone<axum::Foo, X>`: pprof names a frame by its last
    path segment plus generics, so the first generic level is what says whose `clone` it is."""
    frame = re.sub(r"\{closure_env#\d+\}|\{async_fn_env#\d+\}", "{env}", frame)
    out, depth = [], 0
    for ch in frame:
        if ch == "<":
            depth += 1
            if depth == 1:
                out.append(ch)
        elif ch == ">":
            depth = max(0, depth - 1)
            if depth == 0:
                out.append(ch)
        elif depth <= 1:
            out.append(ch)
    return "".join(out).replace(" ", "")[:140] or "?"


def report(path: str, top: int, short_dir: str | None) -> None:
    buckets, self_c, incl, folded, total = Counter(), Counter(), Counter(), Counter(), 0
    for line in open(path, encoding="utf-8", errors="replace"):
        stack, _, n = line.rstrip("\n").rpartition(" ")
        if not stack:
            continue
        n = int(n)
        total += n
        raw = stack.split(";")
        for name, pattern in BUCKETS:
            if any(re.search(pattern, f) for f in raw):
                buckets[name] += n
                break
        else:
            if MIMALLOC[1].match(head(raw[-1])):
                buckets[MIMALLOC[0]] += n
            else:
                buckets["other (hyper, tokio, syscalls, axum routing)"] += n
        frames = [short(f) for f in raw]
        self_c[frames[-1]] += n
        for f in set(frames[1:]):
            incl[f] += n
        folded[";".join(frames)] += n
    name = os.path.basename(path).removesuffix(".folded")
    print(f"\n### {name} ({total} samples)\n\n| share | cost bucket |\n|---:|---|")
    for b, n in buckets.most_common():
        print(f"| {100 * n / total:.1f}% | {b} |")
    print(f"\n| self | frame |\n|---:|---|")
    for f, n in self_c.most_common(top):
        print(f"| {100 * n / total:.1f}% | `{f}` |")
    print(f"\n| inclusive | frame |\n|---:|---|")
    for f, n in [x for x in incl.most_common() if not PLUMBING.match(x[0])][:top]:
        print(f"| {100 * n / total:.1f}% | `{f}` |")
    if short_dir:
        os.makedirs(short_dir, exist_ok=True)
        with open(os.path.join(short_dir, f"{name}.folded"), "w", encoding="utf-8") as out:
            for stack, n in folded.items():
                out.write(f"{stack} {n}\n")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("folded", nargs="+")
    ap.add_argument("--top", type=int, default=12)
    ap.add_argument("--short", metavar="DIR")
    args = ap.parse_args()
    for path in args.folded:
        report(path, args.top, args.short)


if __name__ == "__main__":
    main()
