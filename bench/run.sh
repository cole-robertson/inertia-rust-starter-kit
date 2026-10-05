#!/usr/bin/env bash
# Benchmark this kit (Loco, release build) against the Evil Martians Rails kit
# (production mode, YJIT) on the same machine, same endpoints, SSR off for both
# unless BENCH_SSR=1. Writes bench/results/<timestamp>/ with raw oha JSON + summary.md.
#
#   RAILS_KIT=/tmp/em-kit bench/run.sh
#
# Requires: oha, the Rails kit bundled + `assets:precompile`d, this kit's release
# binary + `npx vite build` output.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

RAILS_KIT="${RAILS_KIT:-/tmp/em-kit}"
RUBY_BIN="${RUBY_BIN:-$HOME/.local/share/mise/installs/ruby/4.0.6/bin}"
DURATION="${DURATION:-15s}"
CONC="${CONC:-32}"
RUN_INDEX="${RUN_INDEX:-0}"
RUST_PORT=5310
# Pin each server to its own 4 cores and the load generator to 4 more, so nothing competes.
RUST_CPUS="${RUST_CPUS:-0-3}"
RAILS_CPUS="${RAILS_CPUS:-4-7}"
OHA_CPUS="${OHA_CPUS:-8-11}"
RAILS_PORT=5311
out="bench/results/${BENCH_LABEL:-run}-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out"
work="$(mktemp -d)"
pids=()
cleanup() {
  for p in "${pids[@]}"; do pkill -TERM -P "$p" 2>/dev/null || true; kill "$p" 2>/dev/null || true; done
  # Puma forks workers; the Rust app forks node when SSR is on. Sweep the ports too.
  for port in "$RUST_PORT" "$RAILS_PORT" 13724 13734; do fuser -k -TERM "$port/tcp" 2>/dev/null || true; done
  wait 2>/dev/null || true; rm -rf "$work"
}
trap cleanup EXIT

rss_kb() { # total RSS of a pid and ALL its descendants (Puma workers, the SSR node child), KiB
  local total=0 p
  for p in $1 $(descendants "$1"); do
    total=$(( total + $(awk '/VmRSS/{print $2}' "/proc/$p/status" 2>/dev/null || echo 0) ))
  done
  echo "$total"
}
descendants() { local c; for c in $(pgrep -P "$1" 2>/dev/null); do echo "$c"; descendants "$c"; done; }

wait_up() { # url -> ms until 200
  local start; start=$(date +%s%N)
  until curl -sf -o /dev/null "$1"; do sleep 0.02; done
  echo $(( ($(date +%s%N) - start) / 1000000 ))
}

secret="$(openssl rand -hex 64)"

# --- Rust (Loco) ---------------------------------------------------------------
rust_db="$work/rust"
mkdir -p "$rust_db"
export_rust() {
  env LOCO_ENV=production SECRET_KEY_BASE="$secret" HOST="https://bench.local" \
    ALLOW_INSECURE_HTTP=true DATABASE_URL="sqlite://$rust_db/app.sqlite?mode=rwc" \
    QUEUE_URL="sqlite://$rust_db/q.sqlite?mode=rwc" MAILER_HOST=localhost \
    MAILER_USER=x MAILER_PASSWORD=x SSR_ENABLED="${BENCH_SSR:-false}" \
    SSR_SPAWN="${BENCH_SSR:-false}" SSR_URL="http://127.0.0.1:13734/render" \
    COMPRESSION=false LOG_LEVEL=error "$@"
}
bin=target/release/inertia_rust_starter_kit-cli
export_rust "$bin" db migrate >/dev/null
export_rust taskset -c "$RUST_CPUS" "$bin" start --no-banner --binding 127.0.0.1 --port "$RUST_PORT" >"$out/rust.log" 2>&1 &
pids+=($!); rust_pid=$!
rust_boot=$(wait_up "http://127.0.0.1:$RUST_PORT/up")

# --- Rails ---------------------------------------------------------------------
# Fresh Rails databases each run, like the Rust side (the kit keeps them in storage/).
rm -f "$RAILS_KIT"/storage/production*.sqlite3*
(cd "$RAILS_KIT" && PATH="$RUBY_BIN:$PATH" RAILS_ENV=production SECRET_KEY_BASE="$secret" \
  bin/rails db:prepare >/dev/null 2>&1)
(
  cd "$RAILS_KIT"
  export PATH="$RUBY_BIN:$PATH" RAILS_ENV=production SECRET_KEY_BASE="$secret" \
    RUBY_YJIT_ENABLE=1 RAILS_LOG_LEVEL=error WEB_CONCURRENCY="${WEB_CONCURRENCY:-4}" \
    RAILS_MAX_THREADS="${RAILS_MAX_THREADS:-5}" PORT="$RAILS_PORT" \
    INERTIA_SSR="${BENCH_SSR:-false}"
  exec taskset -c "$RAILS_CPUS" bin/rails server -b 127.0.0.1 -p "$RAILS_PORT"
) >"$out/rails.log" 2>&1 &
pids+=($!); rails_pid=$!
rails_boot=$(wait_up "http://127.0.0.1:$RAILS_PORT/up")

# The Rails kit enforces `allow_browser versions: :modern`; send a modern UA to both.
UA="Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36"

# Sign in once per app to get an authenticated cookie jar for /dashboard.
signin_rust() {
  local jar="$work/rust.jar" x
  curl -s -A "$UA" -c "$jar" -b "$jar" -o /dev/null "http://127.0.0.1:$RUST_PORT/sign_up"
  x=$(awk '$6=="XSRF-TOKEN"{print $7}' "$jar")
  curl -s -A "$UA" -c "$jar" -b "$jar" -o /dev/null -H "X-XSRF-TOKEN: $x" \
    -H "Origin: https://bench.local" -H "Content-Type: application/json" \
    -d '{"name":"Bench","email":"bench@example.com","password":"bench-password-1","password_confirmation":"bench-password-1"}' \
    "http://127.0.0.1:$RUST_PORT/sign_up"
  sed 's/^#HttpOnly_//' "$jar" | awk '$6=="session_token"{printf "session_token=%s", $7}'
}
signin_rails() {
  local jar="$work/rails.jar" x
  curl -s -A "$UA" -H "Accept: text/html" -c "$jar" -b "$jar" -o /dev/null "http://127.0.0.1:$RAILS_PORT/sign_up"
  x=$(awk '$6=="XSRF-TOKEN"{print $7}' "$jar" | python3 -c 'import sys,urllib.parse;print(urllib.parse.unquote(sys.stdin.read().strip()))')
  curl -s -A "$UA" -c "$jar" -b "$jar" -o /dev/null -H "X-XSRF-TOKEN: $x" \
    -H "Content-Type: application/json" \
    -d '{"name":"Bench","email":"bench@example.com","password":"bench-password-1","password_confirmation":"bench-password-1"}' \
    "http://127.0.0.1:$RAILS_PORT/sign_up"
  sed 's/^#HttpOnly_//' "$jar" | awk '$6=="session_token" || $6 ~ /_session$/ {printf "%s=%s; ", $6, $7}'
}
rust_cookie=$(signin_rust)
rails_cookie=$(signin_rails)
if [ -n "${BENCH_DEBUG:-}" ]; then
  echo "rust_cookie=${rust_cookie:0:40}" >&2; echo "rails_cookie=${rails_cookie:0:120}" >&2
  sed 's/^#HttpOnly_//' "$work/rails.jar" >&2
fi
[ -n "$rust_cookie" ] || { echo "rust sign-in failed" >&2; exit 1; }
[ -n "$rails_cookie" ] || { echo "rails sign-in failed" >&2; exit 1; }
# Both must actually render the dashboard with the cookie (not a redirect).
for pair in "$RUST_PORT|$rust_cookie" "$RAILS_PORT|$rails_cookie"; do
  code=$(curl -s -A "$UA" -o /dev/null -w '%{http_code}' -H "Cookie: ${pair#*|}" "http://127.0.0.1:${pair%%|*}/dashboard")
  [ "$code" = 200 ] || { echo "dashboard on ${pair%%|*} returned $code" >&2; exit 1; }
done

page_version() { # port -> Inertia asset version from the initial page JSON
  curl -s -A "$UA" -H "Accept: text/html" "http://127.0.0.1:$1/sign_in" |
    python3 -c 'import sys,re,json,html
s=sys.stdin.read()
m=re.search(r"<script[^>]*data-page=\"app\"[^>]*>(.*?)</script>", s, re.S)
print(json.loads(m.group(1))["version"] if m else "")'
}
rust_version=$(page_version "$RUST_PORT"); rails_version=$(page_version "$RAILS_PORT")
[ -n "$rust_version" ] && [ -n "$rails_version" ] || { echo "could not read Inertia versions ($rust_version / $rails_version)" >&2; exit 1; }

run() { # name port path [inertia] [cookie]
  local name=$1 port=$2 path=$3 inertia=${4:-} cookie=${5:-}
  # --disable-compression: oha otherwise asks for gzip/br. The Rust app would compress
  # (Loco's compression layer) while Puma sends identity, so the two did different work.
  local args=(-z "$DURATION" -c "$CONC" --no-tui --output-format json --disable-compression -H "User-Agent: $UA")
  local version=$rust_version; [ "$port" = "$RAILS_PORT" ] && version=$rails_version
  [ -n "$inertia" ] && args+=(-H "X-Inertia: true" -H "X-Inertia-Version: $version" -H "X-Requested-With: XMLHttpRequest")
  [ -n "$cookie" ] && args+=(-H "Cookie: $cookie")
  # warm up
  taskset -c "$OHA_CPUS" oha -z 3s -c "$CONC" --no-tui "${args[@]:4}" "http://127.0.0.1:$port$path" >/dev/null 2>&1 || true
  taskset -c "$OHA_CPUS" oha "${args[@]}" "http://127.0.0.1:$port$path" >"$out/$name.json"
}

declare -A cases=(
  [up]="/up||"
  [sign_in_html]="/sign_in||"
  [sign_in_inertia]="/sign_in|1|"
  [dashboard_html]="/dashboard||AUTH"
  [dashboard_inertia]="/dashboard|1|AUTH"
)
order=(up sign_in_html sign_in_inertia dashboard_html dashboard_inertia)

idle_rust=$(rss_kb "$rust_pid"); idle_rails=$(rss_kb "$rails_pid")
for c in "${order[@]}"; do
  IFS='|' read -r path inertia auth <<<"${cases[$c]}"
  rc=""; kc=""; [ -n "$auth" ] && { rc=$rust_cookie; kc=$rails_cookie; }
  # Alternate who goes first per run so drift in background load hits both sides evenly.
  if [ $(( RUN_INDEX % 2 )) -eq 0 ]; then
    run "rust_$c" "$RUST_PORT" "$path" "$inertia" "$rc"
    run "rails_$c" "$RAILS_PORT" "$path" "$inertia" "$kc"
  else
    run "rails_$c" "$RAILS_PORT" "$path" "$inertia" "$kc"
    run "rust_$c" "$RUST_PORT" "$path" "$inertia" "$rc"
  fi
done
cat /proc/loadavg >"$out/loadavg.txt"
peak_rust=$(rss_kb "$rust_pid"); peak_rails=$(rss_kb "$rails_pid")

# Sign-in POST latency (argon2id vs bcrypt), sequential, 20 samples each.
# The Rust kit rate-limits sign-in per client IP (10 / 3 min, Rails 8 style; the Rails
# starter kit has no limiter), so each sample connects from its own loopback address.
signin_ms() { # port extra-curl-args...
  local port=$1; shift
  local jar="$work/si.$port" x t0 total=0 i ip
  for i in $(seq 1 20); do
    ip="127.0.1.$i"
    rm -f "$jar"
    curl -s --interface "$ip" -A "$UA" -H "Accept: text/html" -c "$jar" -b "$jar" -o /dev/null "http://127.0.0.1:$port/sign_in"
    x=$(awk '$6=="XSRF-TOKEN"{print $7}' "$jar" | python3 -c 'import sys,urllib.parse;print(urllib.parse.unquote(sys.stdin.read().strip()))')
    t0=$(date +%s%N)
    code=$(curl -s --interface "$ip" -A "$UA" -c "$jar" -b "$jar" -o /dev/null -w '%{redirect_url}' \
      -H "X-XSRF-TOKEN: $x" "$@" -H "Content-Type: application/json" \
      -d '{"email":"bench@example.com","password":"bench-password-1"}' "http://127.0.0.1:$port/sign_in")
    total=$(( total + ($(date +%s%N) - t0) / 1000 ))
    case "$code" in */dashboard) ;; *) echo "sign-in on $port did not reach /dashboard: $code" >&2; exit 1 ;; esac
  done
  echo $(( total / 20 / 1000 ))
}
rust_signin=$(signin_ms "$RUST_PORT" -H "Origin: https://bench.local")
rails_signin=$(signin_ms "$RAILS_PORT")

python3 - "$out" "$rust_boot" "$rails_boot" "$idle_rust" "$idle_rails" "$peak_rust" "$peak_rails" "$rust_signin" "$rails_signin" "$CONC" "$DURATION" <<'EOF'
import json, sys, os
out, rb, kb, ir, ik, pr, pk, rs, ks, conc, dur = sys.argv[1:]
rows = []
for c in ["up","sign_in_html","sign_in_inertia","dashboard_html","dashboard_inertia"]:
    r = json.load(open(f"{out}/rust_{c}.json")); k = json.load(open(f"{out}/rails_{c}.json"))
    def f(d):
        s = d["summary"]; p = d["latencyPercentiles"]
        codes = d.get("statusCodeDistribution", {})
        return s["requestsPerSec"], p["p50"]*1000, p["p99"]*1000, s["successRate"], codes
    rr, r50, r99, rok, rc = f(r); kr, k50, k99, kok, kc = f(k)
    rows.append((c, rr, kr, r50, k50, r99, k99, rc, kc))
lines = [f"# Benchmark: Loco (Rust) kit vs Rails kit\n",
         "oha, " + dur + " per case after a 3s warm-up, " + conc + " concurrent connections, localhost, SSR " + ("on" if os.environ.get("BENCH_SSR")=="true" else "off") + ".\n",
         "| Endpoint | Rust req/s | Rails req/s | × | Rust p50 ms | Rails p50 ms | Rust p99 ms | Rails p99 ms | status codes (Rust / Rails) |",
         "|---|---:|---:|---:|---:|---:|---:|---:|---|"]
for c, rr, kr, r50, k50, r99, k99, rc, kc in rows:
    lines.append(f"| {c} | {rr:,.0f} | {kr:,.0f} | {rr/kr:,.1f} | {r50:.2f} | {k50:.2f} | {r99:.2f} | {k99:.2f} | {rc} / {kc} |")
lines += ["", "| Metric | Rust | Rails |", "|---|---:|---:|",
          f"| Boot to first 200 on /up | {rb} ms | {kb} ms |",
          f"| RSS idle (after boot + sign-in) | {int(ir)/1024:.0f} MiB | {int(ik)/1024:.0f} MiB |",
          f"| RSS after load | {int(pr)/1024:.0f} MiB | {int(pk)/1024:.0f} MiB |",
          f"| Sign-in POST mean (password hash verify) | {rs} ms | {ks} ms |"]
open(f"{out}/summary.md","w").write("\n".join(lines)+"\n")
print("\n".join(lines))
EOF
echo "raw results: $out"
