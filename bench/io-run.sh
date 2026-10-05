#!/usr/bin/env bash
# I/O benchmark: this kit vs the Rails kit on a slow outbound call, concurrent SQLite writers,
# and reads while writes run (docs/BENCHMARK.md, "I/O-bound workloads").
#
#   RUN_INDEX=0 bench/io-run.sh
#
# Images: irsk:io   (docker build --build-arg CARGO_FEATURES=bench --build-arg SSR_ENABLED=false -t irsk:io .)
#         emkit:io  (the Rails kit + bench/rails-kit.patch + bench/rails-kit-io.patch, SSR_ENABLED=false)
#         bench-upstream (docker build -t bench-upstream bench/upstream)
#
# oha runs with -w on every measured run, so requests in flight at the deadline finish and are
# counted instead of showing up as "aborted due to deadline".
# Same conventions as bench/docker-run.sh: one app container at a time, --cpuset-cpus 0-3
# --memory 4g, order alternating per RUN_INDEX, 3 s warm-up + 15 s measured, oha with
# --disable-compression and JSON output. The mock upstream runs pinned to CPUs 6-7, oha to 4-5.
# Containers use host networking so both apps reach the upstream on 127.0.0.1:9900 over the
# same loopback path. Each app's SQLite storage is a fresh host directory, so the harness can
# count rows with the host's sqlite3 after every write case and compare with oha's 2xx count.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

DURATION="${DURATION:-15s}"; RUN_INDEX="${RUN_INDEX:-0}"
APP_CPUS="${APP_CPUS:-0-3}"; OHA_CPUS="${OHA_CPUS:-4-5}"; UP_CPUS="${UP_CPUS:-6-7}"
MEMORY="${MEMORY:-4g}"   # app container --memory; the README's 32 GB write row uses MEMORY=32g
RAILS_WORKERS="${RAILS_WORKERS:-4}"
RAILS_THREAD_SWEEP="${RAILS_THREAD_SWEEP:-3 16 32}"   # Puma threads per worker, upstream case; 3 is the page-benchmark best
RAILS_DB_THREADS="${RAILS_DB_THREADS:-3 16}"          # Puma threads per worker, write and mixed cases
UPSTREAM_CONC="${UPSTREAM_CONC:-32 128 512}"; WRITE_CONC="${WRITE_CONC:-8 32 128}"
MIXED_WRITERS="${MIXED_WRITERS:-32}"; MIXED_READERS="${MIXED_READERS:-32}"
# Scenario 3 caps the writers at the same total rate on both apps (oha -q), so both apps' reads
# race the same write rate and scan a table growing at the same speed. Unset = unthrottled,
# which is NOT equal work: the faster writer then makes its own reads do more.
MIXED_WRITE_QPS="${MIXED_WRITE_QPS:-500}"
SCENARIOS="${SCENARIOS:-upstream write mixed}"
# Since 48b412f the kit sets Rails 8's SQLite PRAGMAs on every pooled connection (src/db.rs).
# The 2026-09-28 results also hold "rust_kit_*" files: the same cases on an image built before
# that fix, where Loco 1.2 set them on ONE connection (BENCH_SQLITE_PRAGMAS=kit, now removed).
OHA="${OHA:-oha}"
PORT=5410
UP="http://127.0.0.1:9900/slow?ms=100"
out="bench/results/io-${BENCH_LABEL:-run}-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out"
UA="Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36"
PAYLOAD="$(printf 'p%.0s' $(seq 1 200))"               # 200-byte payload, identical for both apps
BODY="{\"payload\":\"$PAYLOAD\"}"
secret="$(openssl rand -hex 64)"
stop_all() { docker rm -f bench-io-app bench-upstream >/dev/null 2>&1 || true; }
stop_all
store="$(mktemp -d "${HOME}/.cache/io-bench-store.XXXXXX")"
trap 'stop_all; rm -rf "$store"' EXIT

want() { case " $SCENARIOS " in *" $1 "*) return 0 ;; esac; return 1; }
oha_run() { # out.json [oha args...] -> 3 s warm-up, then the measured run
  local f=$1; shift
  taskset -c "$OHA_CPUS" "$OHA" -z 3s -w --no-tui --disable-compression -H "User-Agent: $UA" "$@" >/dev/null 2>&1 || true
  taskset -c "$OHA_CPUS" "$OHA" -z "$DURATION" -w --no-tui --disable-compression --output-format json -H "User-Agent: $UA" "$@" >"$f"
}
ok_count() { python3 -c 'import json,sys;d=json.load(open(sys.argv[1]));print(sum(v for k,v in d.get("statusCodeDistribution",{}).items() if k.startswith("2")))' "$1"; }
rows() { sqlite3 "$1" 'SELECT count(*) FROM bench_events;'; }
wait_up() { for _ in $(seq 1 600); do curl -sf -o /dev/null "http://127.0.0.1:$PORT/up" && return; sleep 0.05; done
  echo "never came up" >&2; docker logs bench-io-app 2>&1 | tail -20 >&2; exit 1; }

# --- mock upstream: must not be the bottleneck ------------------------------------------------
docker run -d --name bench-upstream --network host --cpuset-cpus "$UP_CPUS" -e ADDR=127.0.0.1:9900 bench-upstream >/dev/null
for _ in $(seq 1 100); do curl -sf -o /dev/null "$UP" && break; sleep 0.05; done
want upstream && taskset -c "$OHA_CPUS" "$OHA" -z 10s -w -c 512 --no-tui --disable-compression --output-format json "$UP" >"$out/upstream_direct_512.json"

start_app() { # app threads dbdir
  local app=$1 threads=$2 dir=$3
  mkdir -p "$dir"; chmod 777 "$dir"
  if [ "$app" = rust ]; then
    docker run -d --name bench-io-app --network host --cpuset-cpus "$APP_CPUS" --memory "$MEMORY" \
      -v "$dir:/app/storage" -e PORT=$PORT -e BINDING=127.0.0.1 \
      -e SECRET_KEY_BASE="$secret" -e HOST=https://bench.local -e ALLOW_INSECURE_HTTP=true \
      -e MAILER_HOST=localhost -e MAILER_USER=x -e MAILER_PASSWORD=x \
      -e SSR_ENABLED=false -e SSR_SPAWN=false -e COMPRESSION=false -e LOG_LEVEL=error \
      -e BENCH_UPSTREAM_URL="$UP" irsk:io >/dev/null
  else
    docker run -d --name bench-io-app --network host --cpuset-cpus "$APP_CPUS" --memory "$MEMORY" \
      -v "$dir:/rails/storage" -e PORT=$PORT -e BINDING=127.0.0.1 \
      -e SECRET_KEY_BASE="$secret" -e RAILS_LOG_LEVEL=error -e RUBY_YJIT_ENABLE=1 \
      -e WEB_CONCURRENCY="$RAILS_WORKERS" -e RAILS_MAX_THREADS="$threads" -e INERTIA_SSR=false \
      -e BENCH_UPSTREAM_URL="$UP" emkit:io ./bin/rails server >/dev/null
  fi
  wait_up
}
db_file() { [ "$1" = rust ] && echo "$2/production.sqlite" || echo "$2/production.sqlite3"; }
stop_app() { docker rm -f bench-io-app >/dev/null; }

# Sanity: each endpoint answers {"ok":true} before anything is measured.
check_endpoints() {
  curl -sf -A "$UA" "http://127.0.0.1:$PORT/bench/upstream" | grep -q '"ok":true' || { echo "$1: /bench/upstream not ok" >&2; exit 1; }
  curl -sf -A "$UA" -H 'Content-Type: application/json' -d "$BODY" "http://127.0.0.1:$PORT/bench/write" | grep -q '"ok":true' || { echo "$1: /bench/write not ok" >&2; exit 1; }
  curl -sf -A "$UA" "http://127.0.0.1:$PORT/bench/read" | grep -q '"ok":true' || { echo "$1: /bench/read not ok" >&2; exit 1; }
}

# Write cases: fresh DB per concurrency; the row count must equal the 2xx count (plus the
# sanity write and warm-up, which are counted separately: rows_before is taken right before
# the measured run).
write_case() { # app tag threads conc
  local app=$1 tag=$2 threads=$3 c=$4 db before after okc
  local dir="$store/$app$tag-w$c"
  start_app "$app" "$threads" "$dir"; check_endpoints "$app"; db=$(db_file "$app" "$dir")
  taskset -c "$OHA_CPUS" "$OHA" -z 3s -w -c "$c" --no-tui --disable-compression -H "User-Agent: $UA" -m POST -T application/json -d "$BODY" "http://127.0.0.1:$PORT/bench/write" >/dev/null 2>&1 || true
  before=$(rows "$db")
  taskset -c "$OHA_CPUS" "$OHA" -z "$DURATION" -w -c "$c" --no-tui --disable-compression --output-format json -H "User-Agent: $UA" \
    -m POST -T application/json -d "$BODY" "http://127.0.0.1:$PORT/bench/write" >"$out/${app}${tag}_write_c$c.json"
  after=$(rows "$db"); okc=$(ok_count "$out/${app}${tag}_write_c$c.json")
  printf 'rows_before=%s\nrows_after=%s\nrows_added=%s\noha_2xx=%s\n' "$before" "$after" $((after-before)) "$okc" >"$out/${app}${tag}_write_c$c.rows"
  stop_app
}

mixed_case() { # app tag threads
  local app=$1 tag=$2 threads=$3 db before after okc
  local dir="$store/$app$tag-mixed"
  start_app "$app" "$threads" "$dir"; check_endpoints "$app"; db=$(db_file "$app" "$dir")
  # Seed 1,000 rows so the read has real data, then measure the read alone.
  taskset -c "$OHA_CPUS" "$OHA" -n 1000 -c 8 --no-tui -H "User-Agent: $UA" -m POST -T application/json -d "$BODY" "http://127.0.0.1:$PORT/bench/write" >/dev/null
  oha_run "$out/${app}${tag}_read_alone.json" -c "$MIXED_READERS" "http://127.0.0.1:$PORT/bench/read"
  # Then the same read while $MIXED_WRITERS writers run. The writer starts 3 s early (its
  # warm-up) and runs 3 s past the reader's 15 s window, so the whole read window sees writes.
  before=$(rows "$db")
  taskset -c "$OHA_CPUS" "$OHA" -z "$(( ${DURATION%s} + 9 ))s" -w -c "$MIXED_WRITERS" ${MIXED_WRITE_QPS:+-q "$MIXED_WRITE_QPS"} --no-tui --disable-compression --output-format json -H "User-Agent: $UA" \
    -m POST -T application/json -d "$BODY" "http://127.0.0.1:$PORT/bench/write" >"$out/${app}${tag}_mixed_writes.json" &
  local wpid=$!
  sleep 3
  taskset -c "$OHA_CPUS" "$OHA" -z "$DURATION" -w --no-tui --disable-compression --output-format json -H "User-Agent: $UA" \
    -c "$MIXED_READERS" "http://127.0.0.1:$PORT/bench/read" >"$out/${app}${tag}_read_under_writes.json"
  wait "$wpid"
  after=$(rows "$db"); okc=$(ok_count "$out/${app}${tag}_mixed_writes.json")
  printf 'rows_before=%s\nrows_after=%s\nrows_added=%s\noha_2xx=%s\n' "$before" "$after" $((after-before)) "$okc" >"$out/${app}${tag}_mixed_writes.rows"
  stop_app
}

upstream_cases() { # app tag threads
  local app=$1 tag=$2 threads=$3 c
  start_app "$app" "$threads" "$store/$app$tag-up"; check_endpoints "$app"
  for c in $UPSTREAM_CONC; do
    oha_run "$out/${app}${tag}_upstream_c$c.json" -c "$c" "http://127.0.0.1:$PORT/bench/upstream"
  done
  stop_app
}

bench_rust() {
  want upstream && upstream_cases rust "" 0
  if want write; then for c in $WRITE_CONC; do write_case rust "" 0 "$c"; done; fi
  if want mixed; then mixed_case rust "" 0; fi
}
bench_rails() {
  local t
  if want upstream; then for t in $RAILS_THREAD_SWEEP; do upstream_cases rails "_t$t" "$t"; done; fi
  for t in $RAILS_DB_THREADS; do
    if want write; then for c in $WRITE_CONC; do write_case rails "_t$t" "$t" "$c"; done; fi
    if want mixed; then mixed_case rails "_t$t" "$t"; fi
  done
}

if [ $(( RUN_INDEX % 2 )) -eq 0 ]; then bench_rust; bench_rails; else bench_rails; bench_rust; fi
docker rm -f bench-upstream >/dev/null
cat /proc/loadavg >"$out/loadavg.txt"
{ echo "run_index=$RUN_INDEX"; echo "rails_workers=$RAILS_WORKERS"; echo "rails_thread_sweep=$RAILS_THREAD_SWEEP";
  echo "rails_db_threads=$RAILS_DB_THREADS"; echo "scenarios=$SCENARIOS"; echo "mixed_write_qps=$MIXED_WRITE_QPS"; echo "upstream=$UP"; echo "duration=$DURATION";
  echo "app_cpus=$APP_CPUS"; echo "memory=$MEMORY"; echo "oha_cpus=$OHA_CPUS"; echo "upstream_cpus=$UP_CPUS"; nproc; lscpu | grep 'Model name'; } >"$out/env.txt"
echo "$out"
