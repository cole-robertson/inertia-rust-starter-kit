#!/usr/bin/env bash
# Load the kit's hot paths with oha against one release binary (docs/PROFILING.md).
#
#   bench/profile.sh BIN OUT_DIR             # oha throughput per path -> OUT_DIR/<path>.json
#   PROF_CTL=1 bench/profile.sh BIN OUT_DIR  # BIN is a profiling build (docs/PROFILING.md):
#                                            # also OUT_DIR/<path>.folded stacks + allocs/request
#   PROF_CTL=allocs ...                      # profiling build, allocation counts only
#
# The app runs in production mode (CSR, compression off, LOG_LEVEL=error) pinned to APP_CPUS;
# oha runs pinned to OHA_CPUS. Needs: oha, public/vite built.
set -euo pipefail
BIN="$(realpath "$1")"; OUT="$(realpath -m "$2")"; mkdir -p "$OUT"
cd "$(dirname "${BASH_SOURCE[0]}")/.."
APP_CPUS="${APP_CPUS:-4-7}"; OHA_CPUS="${OHA_CPUS:-0-3,12-15}"
DURATION="${DURATION:-10s}"; CONC="${CONC:-32}"; PORT="${PORT:-5510}"
PATHS="${PATHS:-up sign_in_html sign_in_xhr account_html account_xhr profile_patch}"
UA="Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36"
mkdir -p "$HOME/.cache"
work="$(mktemp -d -p "$HOME/.cache")"
cleanup() {
  if [ -n "${pid:-}" ]; then
    # The server must go idle once the load stops (one run hung with a spinning worker thread).
    t0=$(awk '{print $14}' "/proc/$pid/stat" 2>/dev/null || echo 0); sleep 2
    t1=$(awk '{print $14}' "/proc/$pid/stat" 2>/dev/null || echo 0)
    (( t1 - t0 > 50 )) && { echo "SPIN: server busy after the load stopped" >&2; cp "$work/server.log" "$OUT/" 2>/dev/null; }
    kill -9 "$pid" 2>/dev/null
  fi
  wait 2>/dev/null || true; rm -rf "$work"
}
trap cleanup EXIT

count_allocs="$([ "${PROF_CTL:-}" = allocs ] && echo 1 || true)"
LOCO_ENV=production SECRET_KEY_BASE="$(openssl rand -hex 64)" HOST=https://bench.local \
  ALLOW_INSECURE_HTTP=true PORT="$PORT" BINDING=127.0.0.1 COMPRESSION=false LOG_LEVEL=error \
  DATABASE_URL="sqlite://$work/app.sqlite?mode=rwc" QUEUE_URL="sqlite://$work/q.sqlite?mode=rwc" \
  PROF_CTL="${PROF_CTL:+$work}" PROF_COUNT="$count_allocs" \
  taskset -c "$APP_CPUS" "$BIN" start >"$work/server.log" 2>&1 &
pid=$!
base="http://127.0.0.1:$PORT"
until curl -sf -o /dev/null "$base/up"; do kill -0 "$pid" || { cat "$work/server.log"; exit 1; }; sleep 0.05; done

# Sign up once; the jar then holds session_token, _csrf and XSRF-TOKEN.
jar="$work/jar"
xsrf() { sed 's/^#HttpOnly_//' "$jar" | awk '$6=="XSRF-TOKEN"{print $7}' | python3 -c 'import sys,urllib.parse;print(urllib.parse.unquote(sys.stdin.read().strip()))'; }
curl -s -A "$UA" -c "$jar" -b "$jar" -o /dev/null "$base/sign_up"
curl -s -A "$UA" -c "$jar" -b "$jar" -o /dev/null -H "X-XSRF-TOKEN: $(xsrf)" -H "Content-Type: application/json" \
  -d '{"name":"Bench","email":"bench@example.com","password":"bench-password-1","password_confirmation":"bench-password-1"}' "$base/sign_up"
# The signed-in page, the account overview /{account_slug} (where /dashboard redirects); the GET
# also refreshes XSRF-TOKEN after the rotation.
page="$(curl -s -A "$UA" -c "$jar" -b "$jar" -o /dev/null -L -w '%{url_effective}' "$base/dashboard" | sed "s#^$base##")"
cookie="$(sed 's/^#HttpOnly_//' "$jar" | awk 'NF==7{printf "%s=%s; ", $6, $7}')"
token="$(xsrf)"
version="$(curl -s -A "$UA" "$base/sign_in" | python3 -c 'import sys,re,json;m=re.search(r"<script[^>]*data-page=\"app\"[^>]*>(.*?)</script>",sys.stdin.read(),re.S);print(json.loads(m.group(1))["version"])')"
case "$page" in /sign_in | /dashboard | "") echo "not signed in (landed on '$page')" >&2; exit 1 ;; esac
[ "$(curl -s -o /dev/null -w '%{http_code}' -H "Cookie: $cookie" "$base$page")" = 200 ] || { echo "not signed in" >&2; exit 1; }
xhr=(-H "X-Inertia: true" -H "X-Inertia-Version: $version" -H "X-Requested-With: XMLHttpRequest")

count() { # -> "allocations bytes" so far (profiling build only)
  rm -f "$work/count.out"; touch "$work/count"
  until [ -s "$work/count.out" ]; do sleep 0.02; done
  cat "$work/count.out"
}

for name in $PATHS; do
  args=(-c "$CONC" --no-tui --disable-compression --redirect 0 -H "User-Agent: $UA")
  url="$base"
  case "$name" in
    up) url+=/up ;;
    sign_in_html) url+=/sign_in ;;
    sign_in_xhr) url+=/sign_in; args+=("${xhr[@]}") ;;
    account_html) url+=$page; args+=(-H "Cookie: $cookie") ;;
    account_xhr) url+=$page; args+=("${xhr[@]}" -H "Cookie: $cookie") ;;
    profile_patch) url+=/settings/profile
      args+=("${xhr[@]}" -H "Cookie: $cookie" -H "X-XSRF-TOKEN: $token" -H "Content-Type: application/json"
        -m PATCH -d '{"name":"Bench"}') ;;
    *) echo "unknown path $name" >&2; exit 1 ;;
  esac
  taskset -c "$OHA_CPUS" oha -z 2s "${args[@]}" "$url" >/dev/null 2>&1
  # PROF_CTL=1: sample stacks with pprof; PROF_CTL=allocs: count allocations (not both at once:
  # the counter's atomics would show up in the samples).
  case "${PROF_CTL:-}" in
    allocs) before=$(count) ;;
    ?*) echo "$name" >"$work/start" ;;
  esac
  cpu0=$(awk '{print $14 + $15}' "/proc/$pid/stat")
  taskset -c "$OHA_CPUS" oha -z "$DURATION" --output-format json "${args[@]}" "$url" >"$OUT/$name.json"
  echo $(( $(awk '{print $14 + $15}' "/proc/$pid/stat") - cpu0 )) >"$OUT/$name.cpu_ticks"
  case "${PROF_CTL:-}" in
    allocs) echo "$before $(count)" >"$OUT/$name.allocs" ;;
    ?*) touch "$work/stop"; until [ -e "$work/done" ]; do sleep 0.05; done; rm -f "$work/done"
      mv "$work/$name.folded" "$OUT/" ;;
  esac
  python3 - "$OUT/$name.json" "$name" "$OUT/$name.allocs" <<'EOF'
import json, os, sys
d = json.load(open(sys.argv[1]))
rps, p50 = d["summary"]["requestsPerSec"], d["latencyPercentiles"]["p50"] * 1e3
n = sum(d["statusCodeDistribution"].values())
ticks = int(open(sys.argv[1].replace(".json", ".cpu_ticks")).read())
cpu_us = ticks / os.sysconf("SC_CLK_TCK") * 1e6 / n
line = f"{sys.argv[2]:16} {rps:>10.0f} req/s  p50 {p50:.3f} ms  app CPU {cpu_us:.1f} us/req  {d['statusCodeDistribution']}"
if os.path.exists(sys.argv[3]):
    a0, b0, a1, b1 = map(int, open(sys.argv[3]).read().split())
    line += f"  allocs/req {(a1 - a0) / n:.0f}  bytes/req {(b1 - b0) / n:.0f}"
print(line)
EOF
done
