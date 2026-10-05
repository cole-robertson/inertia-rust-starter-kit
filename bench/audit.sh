#!/usr/bin/env bash
# Sanity-check that bench/run.sh compares like with like: boot both apps exactly as the
# benchmark does, fetch every benchmarked URL once with oha's headers, and print status,
# content type, encoding, byte counts (on the wire and decoded), and the Inertia component,
# so a reader can see both sides return the same page.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
RAILS_KIT="${RAILS_KIT:-/tmp/em-kit}"
RUBY_BIN="${RUBY_BIN:-$HOME/.local/share/mise/installs/ruby/4.0.6/bin}"
RP=5320 KP=5321
work="$(mktemp -d "$HOME/.cache/benchaudit.XXXX")"
cleanup() { for p in $RP $KP; do fuser -k -TERM "$p/tcp" 2>/dev/null || true; done; rm -rf "$work"; }
trap cleanup EXIT
secret="$(openssl rand -hex 64)"
UA="Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36"
ENC="${ENC:-identity}" # bench/run.sh runs oha with --disable-compression

env LOCO_ENV=production SECRET_KEY_BASE="$secret" HOST=https://bench.local ALLOW_INSECURE_HTTP=true \
  DATABASE_URL="sqlite://$work/a.sqlite?mode=rwc" QUEUE_URL="sqlite://$work/q.sqlite?mode=rwc" \
  MAILER_HOST=localhost MAILER_USER=x MAILER_PASSWORD=x SSR_ENABLED=false SSR_SPAWN=false COMPRESSION=false LOG_LEVEL=error \
  sh -c "target/release/inertia_rust_starter_kit-cli db migrate >/dev/null && exec target/release/inertia_rust_starter_kit-cli start --no-banner --binding 127.0.0.1 --port $RP" >"$work/rust.log" 2>&1 &
rm -f "$RAILS_KIT"/storage/production*.sqlite3*
(cd "$RAILS_KIT" && export PATH="$RUBY_BIN:$PATH" RAILS_ENV=production SECRET_KEY_BASE="$secret" RAILS_LOG_LEVEL=error INERTIA_SSR=false &&
  bin/rails db:prepare >/dev/null 2>&1 && exec bin/rails server -b 127.0.0.1 -p $KP) >/dev/null 2>&1 &
for p in $RP $KP; do for _ in $(seq 1 300); do curl -sf -o /dev/null "http://127.0.0.1:$p/up" && break; sleep 0.1; done; curl -sf -o /dev/null "http://127.0.0.1:$p/up" || { echo "server on $p did not start"; cat "$work/rust.log" 2>/dev/null; exit 1; }; done

signup() { # port [origin] -> cookie header
  local jar="$work/$1.jar" x
  curl -s -A "$UA" -H "Accept: text/html" -c "$jar" -b "$jar" -o /dev/null "http://127.0.0.1:$1/sign_up"
  x=$(awk '$6=="XSRF-TOKEN"{print $7}' "$jar" | python3 -c 'import sys,urllib.parse;print(urllib.parse.unquote(sys.stdin.read().strip()))')
  curl -s -A "$UA" -c "$jar" -b "$jar" -o /dev/null -H "X-XSRF-TOKEN: $x" ${2:+-H "Origin: $2"} \
    -H "Content-Type: application/json" \
    -d '{"name":"Audit","email":"audit@example.com","password":"audit-password-1","password_confirmation":"audit-password-1"}' \
    "http://127.0.0.1:$1/sign_up"
  sed 's/^#HttpOnly_//' "$jar" | awk '$6=="session_token" || $6 ~ /_session$/ {printf "%s=%s; ", $6, $7}'
}
rc=$(signup $RP https://bench.local); kc=$(signup $KP)
version() { curl -s -A "$UA" -H "Accept: text/html" "http://127.0.0.1:$1/sign_in" |
  python3 -c 'import sys,re,json;m=re.search(r"<script[^>]*data-page=\"app\"[^>]*>(.*?)</script>",sys.stdin.read(),re.S);print(json.loads(m.group(1))["version"])'; }
rv=$(version $RP); kv=$(version $KP)

header() { awk -v n="$1" 'BEGIN{IGNORECASE=1} tolower($1)==n":" {sub(/^[^:]*: */,""); sub(/\r$/,""); print; exit}' "$work/h"; }

printf '%-22s %-5s %-6s %-26s %-8s %8s %8s  %s\n' case app status content-type encoding wire decoded component
# The signed-in page: where GET /dashboard ends up (Rails: /dashboard; this kit: /{account_slug}).
landing() { curl -s -A "$UA" -o /dev/null -L -w '%{url_effective}' -H "Cookie: $2" "http://127.0.0.1:$1/dashboard" | sed "s#^http://127.0.0.1:$1##"; }
rpage=$(landing $RP "$rc"); kpage=$(landing $KP "$kc")
for spec in "up|/up||" "sign_in_html|/sign_in||" "sign_in_inertia|/sign_in|1|" "page_html|PAGE||1" "page_inertia|PAGE|1|1"; do
  IFS='|' read -r name spath inertia auth <<<"$spec"
  for app in rust rails; do
    port=$RP; cookie=$rc; v=$rv; page=$rpage; [ $app = rails ] && { port=$KP; cookie=$kc; v=$kv; page=$kpage; }
    path=$spath; [ "$path" = PAGE ] && path=$page
    hdr=(-A "$UA" -H "Accept: */*" -H "Accept-Encoding: $ENC")
    [ -n "$inertia" ] && hdr+=(-H "X-Inertia: true" -H "X-Inertia-Version: $v" -H "X-Requested-With: XMLHttpRequest")
    [ -n "$auth" ] && hdr+=(-H "Cookie: $cookie")
    curl -s -m 10 "${hdr[@]}" -D "$work/h" -o "$work/b" "http://127.0.0.1:$port$path"
    wire=$(stat -c %s "$work/b")
    curl -s -m 10 --compressed "${hdr[@]}" -o "$work/d" "http://127.0.0.1:$port$path"
    status=$(head -1 "$work/h" | awk '{print $2}')
    ctype=$(header content-type | cut -c1-26)
    enc=$(header content-encoding); enc=${enc:-none}
    comp=$(python3 - "$work/d" <<'PY'
import sys,re,json
s=open(sys.argv[1],encoding="utf-8",errors="replace").read()
try: print(json.loads(s)["component"]); raise SystemExit
except SystemExit: raise
except Exception: pass
m=re.search(r'<script[^>]*data-page="app"[^>]*>(.*?)</script>',s,re.S)
print(json.loads(m.group(1))["component"] if m else "-")
PY
)
    printf '%-22s %-5s %-6s %-26s %-8s %8s %8s  %s\n' "$name" $app "$status" "$ctype" "$enc" "$wire" "$(stat -c %s "$work/d")" "$comp"
  done
done
