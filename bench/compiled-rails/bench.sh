#!/usr/bin/env bash
# Same-box benchmark: the Rust kit vs the Evil Martians Rails kit vs the stand-in slice on
# stock Rails / Roundhouse ruby emit / Spinel binary / Roundhouse rust emit, plus the
# Roundhouse blog fixture on the same lanes. Method follows docs/BENCHMARK.md:
# production mode, 4 CPUs (taskset 0-3), compression off, oha 32 connections,
# 3 s warm-up + 15 s measured, run order rotated per run.
#
#   RUN_INDEX=0 LANES="rust-kit rails-kit ..." bench/compiled-rails/bench.sh
#
# Runs from ~/.cache/compile-cmp (see README.md for the tree it expects).
set -uo pipefail
C=${C:-$HOME/.cache/compile-cmp}
export LANG=C.UTF-8 LC_ALL=C.UTF-8
RUBY_BIN=${RUBY_BIN:-$HOME/.local/share/mise/installs/ruby/4.0.6/bin}
OHA=${OHA:-$HOME/.cargo/bin/oha}
APP_CPUS=${APP_CPUS:-0-3}; OHA_CPUS=${OHA_CPUS:-4-7,16-19}
DURATION=${DURATION:-15s}; CONC=${CONC:-32}; RUN_INDEX=${RUN_INDEX:-0}
PORT=5350
UA="Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36"
out="$C/results/run-$(printf %02d "$RUN_INDEX")"; mkdir -p "$out"
LANES=(${LANES:-rust-kit rails-kit slice-rails slice-rhruby slice-spinel blog-rails blog-rhruby blog-spinel blog-rhrust})

descendants() { local c; for c in $(pgrep -P "$1" 2>/dev/null); do echo "$c"; descendants "$c"; done; }
tree() { echo "$1"; descendants "$1"; }
rss_kb() { local t=0 p; for p in $(tree "$1"); do t=$(( t + $(awk '/VmRSS/{print $2}' /proc/$p/status 2>/dev/null || echo 0) )); done; echo $t; }
cpu_ticks() { # utime+stime of the whole process tree (incl. reaped children), in clock ticks
  local t=0 p; for p in $(tree "$1"); do t=$(( t + $(awk '{print $14+$15+$16+$17}' /proc/$p/stat 2>/dev/null || echo 0) )); done; echo $t; }
HZ=$(getconf CLK_TCK)

start_lane() { # lane -> sets PID, CASES; logs to $out/$lane.log
  local lane=$1 secret; secret=$(openssl rand -hex 64)
  fuser -k $PORT/tcp >/dev/null 2>&1; sleep 0.3
  case $lane in
    rust-kit)
      local d=$C/state/rust-kit; rm -rf "$d"; mkdir -p "$d"
      rk() { (cd $C/rust-kit && env LOCO_ENV=production SECRET_KEY_BASE=$secret HOST=https://bench.local \
        ALLOW_INSECURE_HTTP=true DATABASE_URL="sqlite://$d/app.sqlite?mode=rwc" QUEUE_URL="sqlite://$d/q.sqlite?mode=rwc" \
        MAILER_HOST=localhost MAILER_USER=x MAILER_PASSWORD=x SSR_ENABLED=false SSR_SPAWN=false COMPRESSION=false \
        LOG_LEVEL=error "$@"); }
      rk $C/target-rust-kit/release/inertia_rust_starter_kit-cli db migrate >/dev/null 2>&1
      (cd $C/rust-kit && exec env LOCO_ENV=production SECRET_KEY_BASE=$secret HOST=https://bench.local \
        ALLOW_INSECURE_HTTP=true DATABASE_URL="sqlite://$d/app.sqlite?mode=rwc" QUEUE_URL="sqlite://$d/q.sqlite?mode=rwc" \
        MAILER_HOST=localhost MAILER_USER=x MAILER_PASSWORD=x SSR_ENABLED=false SSR_SPAWN=false COMPRESSION=false LOG_LEVEL=error \
        taskset -c $APP_CPUS $C/target-rust-kit/release/inertia_rust_starter_kit-cli start --no-banner --binding 127.0.0.1 --port $PORT) >$out/$lane.log 2>&1 &
      ;;
    rails-kit|slice-rails|blog-rails)
      local dir=$C/em-kit; [ $lane = slice-rails ] && dir=$C/kit-slice; [ $lane = blog-rails ] && dir=$C/blog/real-blog
      rm -f $dir/storage/production*.sqlite3*
      (cd $dir && PATH=$RUBY_BIN:$PATH RAILS_ENV=production SECRET_KEY_BASE=$secret bin/rails db:prepare >/dev/null 2>&1)
      if [ $lane = slice-rails ]; then
        sqlite3 $dir/storage/production.sqlite3 "INSERT INTO users (name,email,password_digest,verified,created_at,updated_at) VALUES ('Bench','bench@example.com','\$2a\$12\$wmZQgvAFNzzyArAZ1c10r.osYBuL14vhMF4eGA37/BWVpj1tVTTNy',0,datetime('now'),datetime('now'))"
      fi
      (cd $dir && exec env PATH=$RUBY_BIN:$PATH RAILS_ENV=production SECRET_KEY_BASE=$secret RUBY_YJIT_ENABLE=1 \
        RAILS_LOG_LEVEL=error WEB_CONCURRENCY=4 RAILS_MAX_THREADS=3 PORT=$PORT INERTIA_SSR=false \
        taskset -c $APP_CPUS bin/rails server -b 127.0.0.1 -p $PORT) >$out/$lane.log 2>&1 &
      ;;
    slice-rhruby|blog-rhruby)
      local dir=$C/out/slice-ruby; [ $lane = blog-rhruby ] && dir=$C/out/blog-ruby
      if [ $lane = slice-rhruby ]; then $C/seed.sh $dir/storage/development.sqlite3 $dir/db/seed.sql
      else rm -f $dir/storage/development.sqlite3*; mkdir -p $dir/storage; sqlite3 $dir/storage/development.sqlite3 < $dir/db/seed.sql; fi
      (cd $dir && exec env PATH=$RUBY_BIN:$PATH BUNDLE_GEMFILE=$dir/Gemfile BLOG_DB=storage/development.sqlite3 \
        SECRET_KEY_BASE=$secret RAILS_ENV=production RUBYOPT=--yjit WEB_CONCURRENCY=4 RAILS_MAX_THREADS=3 PORT=$PORT \
        taskset -c $APP_CPUS bundle exec puma -C config/puma.rb config.ru) >$out/$lane.log 2>&1 &
      ;;
    slice-spinel|blog-spinel)
      local dir=$C/out/slice-spinel; [ $lane = blog-spinel ] && dir=$C/out/blog-spinel
      if [ $lane = slice-spinel ]; then $C/seed.sh $dir/storage/development.sqlite3 $dir/db/seed.sql
      else rm -f $dir/storage/development.sqlite3*; mkdir -p $dir/storage; sqlite3 $dir/storage/development.sqlite3 < $dir/db/seed.sql; fi
      # sysconf() ignores the affinity mask (it reports 24 under taskset 0-3), so cap the OS workers
      # explicitly to the 4 cores the app is pinned to.
      (cd $dir && exec env SECRET_KEY_BASE=$secret SPINEL_WORKERS=4 PORT=$PORT taskset -c $APP_CPUS ./build/bin/blog) >$out/$lane.log 2>&1 &
      ;;
    blog-rhrust)
      local dir=$C/out/blog-rust
      rm -f $dir/storage/development.sqlite3*; mkdir -p $dir/storage; sqlite3 $dir/storage/development.sqlite3 < $dir/db/seed.sql
      # tokio sizes its pool from sched_getaffinity, so taskset alone gives it 4 workers.
      (cd $dir && exec env PORT=$PORT taskset -c $APP_CPUS $C/target-blog-rust/release/app) >$out/$lane.log 2>&1 &
      ;;
  esac
  PID=$!
}

wait_up() { local path=$1 t0; t0=$(date +%s%N)
  for _ in $(seq 1 600); do curl -sf -o /dev/null -A "$UA" "http://127.0.0.1:$PORT$path" && { echo $(( ($(date +%s%N) - t0) / 1000000 )); return; }; sleep 0.02; done
  echo -1; }

cookie_for() { # lane -> Cookie header value for a signed-in user
  local lane=$1; local jar=$out/$lane.jar x; rm -f $jar
  case $lane in
    rust-kit)
      curl -s -A "$UA" -c $jar -b $jar -o /dev/null http://127.0.0.1:$PORT/sign_up
      x=$(awk '$6=="XSRF-TOKEN"{print $7}' $jar)
      curl -s -A "$UA" -c $jar -b $jar -o /dev/null -H "X-XSRF-TOKEN: $x" -H "Origin: https://bench.local" \
        -H "Content-Type: application/json" -d '{"name":"Bench","email":"bench@example.com","password":"bench-password-1","password_confirmation":"bench-password-1"}' \
        http://127.0.0.1:$PORT/sign_up ;;
    rails-kit)
      curl -s -A "$UA" -H "Accept: text/html" -c $jar -b $jar -o /dev/null http://127.0.0.1:$PORT/sign_up
      x=$(awk '$6=="XSRF-TOKEN"{print $7}' $jar | python3 -c 'import sys,urllib.parse;print(urllib.parse.unquote(sys.stdin.read().strip()))')
      curl -s -A "$UA" -c $jar -b $jar -o /dev/null -H "X-XSRF-TOKEN: $x" -H "Content-Type: application/json" \
        -d '{"name":"Bench","email":"bench@example.com","password":"bench-password-1","password_confirmation":"bench-password-1"}' \
        http://127.0.0.1:$PORT/sign_up ;;
    slice-*)
      curl -s -A "$UA" -c $jar -b $jar -o $out/$lane.signin.html http://127.0.0.1:$PORT/sign_in
      x=$(python3 -c 'import re,sys;m=re.search(r"name=\"csrf-token\" content=\"([^\"]+)\"",open(sys.argv[1]).read());print(m.group(1) if m else "")' $out/$lane.signin.html)
      curl -s -A "$UA" -c $jar -b $jar -o /dev/null -H "X-CSRF-Token: $x" \
        -d 'email=bench@example.com&password=bench-password-1' http://127.0.0.1:$PORT/sign_in ;;
    *) return ;;
  esac
  sed 's/^#HttpOnly_//' $jar | awk '$6=="session_token" || $6 ~ /_session$/ {printf "%s=%s; ", $6, $7}'
}

inertia_version() { curl -s -A "$UA" -H "Accept: text/html" "http://127.0.0.1:$PORT/sign_in" | python3 -c '
import sys,re,json
m=re.search(r"<script[^>]*data-page=\"app\"[^>]*>(.*?)</script>", sys.stdin.read(), re.S)
print(json.loads(m.group(1))["version"] if m else "")'; }

oha_case() { # name path [xhr] [cookie]
  local name=$1 path=$2 xhr=${3:-} cookie=${4:-}
  local args=(-c $CONC --no-tui --disable-compression -H "User-Agent: $UA")
  [ -n "$xhr" ] && args+=(-H "X-Inertia: true" -H "X-Inertia-Version: $VERSION" -H "X-Requested-With: XMLHttpRequest")
  [ -n "$cookie" ] && args+=(-H "Cookie: $cookie")
  taskset -c $OHA_CPUS $OHA -z 3s "${args[@]}" "http://127.0.0.1:$PORT$path" >/dev/null 2>&1
  local c0 t0 c1 t1; c0=$(cpu_ticks $PID); t0=$(date +%s%N)
  taskset -c $OHA_CPUS $OHA -z $DURATION "${args[@]}" --output-format json "http://127.0.0.1:$PORT$path" >$out/${LANE}__$name.json
  c1=$(cpu_ticks $PID); t1=$(date +%s%N)
  echo "cpu_ticks=$((c1 - c0)) hz=$HZ wall_ms=$(( (t1 - t0) / 1000000 ))" >$out/${LANE}__$name.cpu
}

# Rotate lane order per run so slow drift hits every lane evenly.
n=${#LANES[@]}; order=()
for i in $(seq 0 $((n - 1))); do order+=("${LANES[$(( (i + RUN_INDEX) % n ))]}"); done
[ $(( RUN_INDEX % 2 )) -eq 1 ] && order=($(printf '%s\n' "${order[@]}" | tac))
echo "run $RUN_INDEX order: ${order[*]}" | tee $out/order.txt
for LANE in "${order[@]}"; do
  start_lane $LANE
  case $LANE in blog-*) up=/articles ;; *) up=/up ;; esac
  boot=$(wait_up $up)
  [ "$boot" = -1 ] && { echo "$LANE did not boot"; tail -5 $out/$LANE.log; kill $PID; continue; }
  sleep 1
  cookie=$(cookie_for $LANE)
  idle=$(rss_kb $PID)
  case $LANE in
    blog-*)
      oha_case articles /articles; oha_case article_1 /articles/1 ;;
    *)
      VERSION=$(inertia_version)
      code=$(curl -s -o /dev/null -w '%{http_code}' -A "$UA" -H "Cookie: $cookie" http://127.0.0.1:$PORT/dashboard)
      [ "$code" = 200 ] || { echo "$LANE: signed-in /dashboard returned $code"; }
      oha_case up /up; oha_case sign_in_html /sign_in; oha_case sign_in_inertia /sign_in 1
      oha_case dashboard_html /dashboard "" "$cookie"; oha_case dashboard_inertia /dashboard 1 "$cookie" ;;
  esac
  peak=$(rss_kb $PID)
  echo "boot_ms=$boot idle_kib=$idle peak_kib=$peak load=$(cut -d' ' -f1 /proc/loadavg)" >$out/$LANE.meta
  echo "$LANE: $(cat $out/$LANE.meta)"
  for p in $(tree $PID | tac); do kill -TERM $p 2>/dev/null; done; wait $PID 2>/dev/null
  fuser -k $PORT/tcp >/dev/null 2>&1
done
