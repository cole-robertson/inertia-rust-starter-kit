#!/usr/bin/env bash
# Diagnose the Rust image's 500s on POST /bench/write at 32 writers: the same container settings
# as bench/io-run.sh, but LOG_LEVEL=warn and the container's log kept. Three variants, 15 s each:
#   default          (pool 10, acquire timeout = connect_timeout 500 ms)
#   pool1            DB_MAX_CONNECTIONS=1 (one writer connection: serialized in the pool)
#   timeout5000      DB_CONNECT_TIMEOUT=5000 (acquire waits up to 5 s)
# Needs the irsk:io image (bench/io-run.sh) and oha. Output: bench/results/io-diag/<variant>.{json,log}
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
out=bench/results/io-diag; mkdir -p "$out"
UA="Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36"
BODY="{\"payload\":\"$(printf 'p%.0s' $(seq 1 200))\"}"
secret=$(openssl rand -hex 64)
run() { # name extra-env...
  local name=$1; shift
  local dir; dir=$(mktemp -d "$HOME/.cache/io-diag.XXXX"); chmod 777 "$dir"
  docker rm -f diag-app >/dev/null 2>&1
  docker run -d --name diag-app --network host --cpuset-cpus 0-3 --memory 4g -v "$dir:/app/storage" \
    -e PORT=5410 -e BINDING=127.0.0.1 -e SECRET_KEY_BASE="$secret" -e HOST=https://bench.local \
    -e ALLOW_INSECURE_HTTP=true -e MAILER_HOST=localhost -e MAILER_USER=x -e MAILER_PASSWORD=x \
    -e SSR_ENABLED=false -e SSR_SPAWN=false -e COMPRESSION=false -e LOG_LEVEL=warn "$@" irsk:io >/dev/null
  for _ in $(seq 1 200); do curl -sf -o /dev/null http://127.0.0.1:5410/up && break; sleep 0.05; done
  taskset -c 4-5 oha -z 3s -w -c 32 --no-tui --disable-compression -H "User-Agent: $UA" -m POST -T application/json -d "$BODY" http://127.0.0.1:5410/bench/write >/dev/null 2>&1
  local before; before=$(sqlite3 "$dir/production.sqlite" 'select count(*) from bench_events')
  taskset -c 4-5 oha -z 15s -w -c 32 --no-tui --disable-compression --output-format json -H "User-Agent: $UA" \
    -m POST -T application/json -d "$BODY" http://127.0.0.1:5410/bench/write >"$out/$name.json"
  local after; after=$(sqlite3 "$dir/production.sqlite" 'select count(*) from bench_events')
  docker logs diag-app >"$out/$name.log" 2>&1
  docker rm -f diag-app >/dev/null
  python3 - "$out/$name.json" "$name" "$((after - before))" <<'EOF'
import json, sys
d = json.load(open(sys.argv[1])); c = d["statusCodeDistribution"]; n = sum(c.values())
print(f"{sys.argv[2]:12} {d['summary']['requestsPerSec']:8.0f} req/s  p99 {d['latencyPercentiles']['p99']*1000:6.1f} ms  codes {c}  500s {c.get('500',0)/n:.3%}  rows {sys.argv[3]}")
EOF
  grep -oE 'error\.msg=[^=]*error\.details' "$out/$name.log" | sed 's/ error.details//' | sort | uniq -c | sort -rn | head -3
  rm -rf "$dir" 2>/dev/null || true
}
run default
run pool1 -e DB_MAX_CONNECTIONS=1
run timeout5000 -e DB_CONNECT_TIMEOUT=5000
