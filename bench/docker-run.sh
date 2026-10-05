#!/usr/bin/env bash
# Benchmark the two production Docker images side by side on a quiet host.
#
#   bench/docker-run.sh                  # SSR off
#   BENCH_SSR=true bench/docker-run.sh   # SSR on (both images ship their Node renderer)
#
# The signed-in page is the first page after sign-in in each kit: the Rails kit's `/dashboard`, and
# here the account overview `/{account_slug}` (`/dashboard` is a redirect to it since accounts
# became core; it also loads the account and the membership). Each app's page is where
# `GET /dashboard` ends up for the signed-up user, recorded in <run>/<app>.page.
#
# Images: irsk:bench (this kit, `docker build -t irsk:bench .`) and emkit:bench (the Evil
# Martians Rails kit, `docker build -t emkit:bench .` in its checkout). Each container is
# pinned to its own CPUs (--cpuset-cpus) with the same --cpus/--memory, and oha runs pinned to
# the remaining CPUs. The Rust and Rails containers are measured one after the other per case,
# alternating who goes first (RUN_INDEX), with compression off on both sides. The Rust
# SSR timeout is raised to 60 s to match Rails' Net::HTTP default; with the kit's 1.5 s
# default an overloaded renderer makes Rust fall back to cheap client-side pages.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

DURATION="${DURATION:-15s}"; CONC="${CONC:-32}"; RUN_INDEX="${RUN_INDEX:-0}"
APP_CPUS="${APP_CPUS:-0-3}"; OHA_CPUS="${OHA_CPUS:-4-7}"
RAILS_WORKERS="${RAILS_WORKERS:-4}"; RAILS_THREADS="${RAILS_THREADS:-3}"
RAILS_FRONT="${RAILS_FRONT:-puma}" # puma | thruster (the kit's Docker CMD puts Thruster in front)
SSR="${BENCH_SSR:-false}"
PORT=5410
out="bench/results/docker-${BENCH_LABEL:-run}-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out"
UA="Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36"
secret="$(openssl rand -hex 64)"
cleanup() { docker rm -f bench-rust bench-rails >/dev/null 2>&1 || true; }
trap cleanup EXIT
cleanup

wait_up() { local s; s=$(date +%s%N); for _ in $(seq 1 600); do curl -sf -o /dev/null "http://127.0.0.1:$PORT/up" && { echo $(( ($(date +%s%N)-s)/1000000 )); return; }; sleep 0.05; done; echo "never came up" >&2; docker logs "$1" 2>&1 | tail -20 >&2; exit 1; }
mem_mib() { docker stats --no-stream --format '{{.MemUsage}}' "$1" | awk '{v=$1; if (v ~ /GiB/) {sub("GiB","",v); v*=1024} else sub("MiB","",v); printf "%.0f", v}'; }

start_rust() {
  docker run -d --name bench-rust --cpuset-cpus "$APP_CPUS" --memory 4g -p 127.0.0.1:$PORT:80 \
    -e SECRET_KEY_BASE="$secret" -e HOST=https://bench.local -e ALLOW_INSECURE_HTTP=true \
    -e MAILER_HOST=localhost -e MAILER_USER=x -e MAILER_PASSWORD=x \
    -e SSR_ENABLED="$SSR" -e SSR_SPAWN="$SSR" -e SSR_TIMEOUT_MS=60000 -e COMPRESSION=false -e LOG_LEVEL=error \
    irsk:bench >/dev/null
  wait_up bench-rust
}
start_rails() {
  # The kit's entrypoint runs db:prepare only when the command ENDS in `./bin/rails server`,
  # so keep that suffix and pass the port/bind through the environment instead of flags.
  local cmd=(./bin/rails server) port_env=(-e PORT=80 -e BINDING=0.0.0.0)
  [ "$RAILS_FRONT" = thruster ] && { cmd=(./bin/thrust ./bin/rails server); port_env=(-e HTTP_PORT=80 -e TARGET_PORT=3000 -e PORT=3000); }
  docker run -d --name bench-rails --cpuset-cpus "$APP_CPUS" --memory 4g -p 127.0.0.1:$PORT:80 \
    -e SECRET_KEY_BASE="$secret" -e RAILS_LOG_LEVEL=error -e RUBY_YJIT_ENABLE=1 \
    -e WEB_CONCURRENCY="$RAILS_WORKERS" -e RAILS_MAX_THREADS="$RAILS_THREADS" -e INERTIA_SSR="$SSR" \
    "${port_env[@]}" \
    emkit:bench "${cmd[@]}" >/dev/null
  wait_up bench-rails
}

signup() { # [origin] -> cookie header for a freshly signed-up user
  local jar; jar=$(mktemp); local x
  curl -s -A "$UA" -H "Accept: text/html" -c "$jar" -b "$jar" -o /dev/null "http://127.0.0.1:$PORT/sign_up"
  x=$(awk '$6=="XSRF-TOKEN"{print $7}' "$jar" | python3 -c 'import sys,urllib.parse;print(urllib.parse.unquote(sys.stdin.read().strip()))')
  curl -s -A "$UA" -c "$jar" -b "$jar" -o /dev/null -H "X-XSRF-TOKEN: $x" ${1:+-H "Origin: $1"} \
    -H "Content-Type: application/json" \
    -d '{"name":"Bench","email":"bench@example.com","password":"bench-password-1","password_confirmation":"bench-password-1"}' \
    "http://127.0.0.1:$PORT/sign_up"
  sed 's/^#HttpOnly_//' "$jar" | awk '$6=="session_token" || $6 ~ /_session$/ {printf "%s=%s; ", $6, $7}'
  rm -f "$jar"
}
version() { curl -s -A "$UA" -H "Accept: text/html" "http://127.0.0.1:$PORT/sign_in" |
  python3 -c 'import sys,re,json;m=re.search(r"<script[^>]*data-page=\"app\"[^>]*>(.*?)</script>",sys.stdin.read(),re.S);print(json.loads(m.group(1))["version"])'; }

signin_ms() { # page_path [origin] -> mean ms over 10 sequential sign-ins (separate cookie jars; limiter-safe: 10 ≤ limit)
  local page=$1 total=0 i jar x t0 loc; shift
  for i in $(seq 1 10); do
    jar=$(mktemp)
    curl -s -A "$UA" -H "Accept: text/html" -c "$jar" -b "$jar" -o /dev/null "http://127.0.0.1:$PORT/sign_in"
    x=$(awk '$6=="XSRF-TOKEN"{print $7}' "$jar" | python3 -c 'import sys,urllib.parse;print(urllib.parse.unquote(sys.stdin.read().strip()))')
    t0=$(date +%s%N)
    loc=$(curl -s -A "$UA" -c "$jar" -b "$jar" -o /dev/null -w '%{redirect_url}' -H "X-XSRF-TOKEN: $x" ${1:+-H "Origin: $1"} \
      -H "Content-Type: application/json" -d '{"email":"bench@example.com","password":"bench-password-1"}' \
      "http://127.0.0.1:$PORT/sign_in")
    total=$(( total + ($(date +%s%N) - t0) / 1000 )); rm -f "$jar"
    case "$loc" in *"$page") ;; *) echo "sign-in did not reach $page: $loc" >&2; exit 1 ;; esac
  done
  echo $(( total / 10 / 1000 ))
}

# PAGE: the signed-in page, set per app in bench_app. The result files keep the dashboard_* names,
# so bench/summarize.py reads old and new runs alike.
CASES=("up|/up||" "sign_in_html|/sign_in||" "sign_in_inertia|/sign_in|1|" "dashboard_html|PAGE||1" "dashboard_inertia|PAGE|1|1")

landing() { # cookie -> the path GET /dashboard ends up on (Rails: /dashboard itself)
  curl -s -A "$UA" -o /dev/null -L -w '%{url_effective}' -H "Cookie: $1" "http://127.0.0.1:$PORT/dashboard" | sed "s#^http://127.0.0.1:$PORT##"
}

bench_app() { # app -> writes $out/<app>_<case>.json + $out/<app>.meta
  local app=$1 boot cookie v idle peak signin origin="" page
  [ "$app" = rust ] && { origin=https://bench.local; boot=$(start_rust); } || boot=$(start_rails)
  cookie=$(signup "$origin"); [ -n "$cookie" ] || { echo "$app: sign-up failed" >&2; exit 1; }
  page=$(landing "$cookie")
  case "$app:$page" in *: | *:/sign_in | rust:/dashboard) echo "$app: signed-in page is '$page'" >&2; exit 1 ;; esac
  [ "$(curl -s -A "$UA" -o /dev/null -w '%{http_code}' -H "Cookie: $cookie" "http://127.0.0.1:$PORT$page")" = 200 ] ||
    { echo "$app: $page not 200 with cookie" >&2; exit 1; }
  echo "$page" >"$out/$app.page"
  v=$(version)
  sleep 2; idle=$(mem_mib "bench-$app")
  for spec in "${CASES[@]}"; do
    IFS='|' read -r name path inertia auth <<<"$spec"
    [ "$path" = PAGE ] && path=$page
    local args=(-c "$CONC" --no-tui --disable-compression -H "User-Agent: $UA")
    [ -n "$inertia" ] && args+=(-H "X-Inertia: true" -H "X-Inertia-Version: $v" -H "X-Requested-With: XMLHttpRequest")
    [ -n "$auth" ] && args+=(-H "Cookie: $cookie")
    taskset -c "$OHA_CPUS" oha -z 3s "${args[@]}" "http://127.0.0.1:$PORT$path" >/dev/null 2>&1 || true
    taskset -c "$OHA_CPUS" oha -z "$DURATION" --output-format json "${args[@]}" "http://127.0.0.1:$PORT$path" >"$out/${app}_$name.json"
  done
  peak=$(mem_mib "bench-$app")
  signin=$(signin_ms "$page" "$origin")
  printf 'boot_ms=%s\nidle_mib=%s\npeak_mib=%s\nsignin_ms=%s\n' "$boot" "$idle" "$peak" "$signin" >"$out/$app.meta"
  docker rm -f "bench-$app" >/dev/null
}

if [ $(( RUN_INDEX % 2 )) -eq 0 ]; then bench_app rust; bench_app rails; else bench_app rails; bench_app rust; fi
cat /proc/loadavg >"$out/loadavg.txt"
{ echo "ssr=$SSR"; echo "rails_front=$RAILS_FRONT"; echo "rails_workers=$RAILS_WORKERS"; echo "rails_threads=$RAILS_THREADS";
  echo "conc=$CONC"; echo "duration=$DURATION"; echo "app_cpus=$APP_CPUS"; echo "oha_cpus=$OHA_CPUS"; nproc; lscpu | grep 'Model name'; } >"$out/env.txt"
echo "$out"
