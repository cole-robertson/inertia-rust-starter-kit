#!/usr/bin/env bash
# Functional + security probe for one running app on 127.0.0.1:$1.
#   probe.sh PORT LABEL
# Assumes a user bench@example.com / bench-password-1 exists (seed.sh creates it).
# Prints one line per check; every line is data for docs/RUST_VS_COMPILED_RAILS.md.
set -uo pipefail
P=$1; L=${2:-app}
UA="Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36"
B="http://127.0.0.1:$P"; W=$(mktemp -d "$HOME/.cache/probe.XXXX"); trap 'rm -rf "$W"' EXIT
J=$W/jar
code() { curl -s -o "$W/body" -w '%{http_code} %{redirect_url}' "$@"; }
page() { python3 - "$W/body" <<'PY'
import sys,re,json
s=open(sys.argv[1],encoding="utf-8",errors="replace").read()
m=re.search(r'<script[^>]*data-page="app"[^>]*>(.*?)</script>',s,re.S)
try:
  d=json.loads(m.group(1) if m else s); print(d.get("component"), "auth.user=%s" % (d["props"]["auth"]["user"] or {}).get("email"))
except Exception as e: print("no-page-json", len(s), "bytes")
PY
}
echo "[$L] GET /up               -> $(code -A "$UA" $B/up)"
echo "[$L] GET /sign_in (html)   -> $(code -A "$UA" -c $J -b $J $B/sign_in) $(page) $(wc -c <"$W/body")B"
csrf=$(python3 -c 'import re,sys;m=re.search(r"name=\"csrf-token\" content=\"([^\"]+)\"",open(sys.argv[1]).read());print(m.group(1) if m else "")' "$W/body")
echo "[$L] GET /sign_in (xhr)    -> $(code -A "$UA" -H 'X-Inertia: true' -H 'X-Requested-With: XMLHttpRequest' $B/sign_in) $(page) $(wc -c <"$W/body")B"
echo "[$L] old browser UA        -> $(code -A 'Mozilla/4.0 (compatible; MSIE 6.0; Windows NT 5.1)' $B/sign_in)   (Rails: 406)"
echo "[$L] POST sign_in no token -> $(code -A "$UA" -c $J -b $J -d 'email=bench@example.com&password=bench-password-1' $B/sign_in)   (Rails: 422)"
rm -f $J; curl -s -A "$UA" -c $J -b $J -o "$W/body" $B/sign_in
csrf=$(python3 -c 'import re,sys;m=re.search(r"name=\"csrf-token\" content=\"([^\"]+)\"",open(sys.argv[1]).read());print(m.group(1) if m else "")' "$W/body")
echo "[$L] POST sign_in w/ token -> $(code -A "$UA" -c $J -b $J -H "X-CSRF-Token: $csrf" -d 'email=bench@example.com&password=bench-password-1' $B/sign_in)"
U=$W/ujar; curl -s -A "$UA" -c $U -b $U -o "$W/body" $B/sign_in
ut=$(python3 -c 'import re,sys;m=re.search(r"name=\"csrf-token\" content=\"([^\"]+)\"",open(sys.argv[1]).read());print(m.group(1) if m else "")' "$W/body")
echo "[$L] POST sign_in UPPER    -> $(code -A "$UA" -c $U -b $U -H "X-CSRF-Token: $ut" -d 'email=BENCH@EXAMPLE.COM&password=bench-password-1' $B/sign_in)   (Rails, normalizes: /dashboard; without: /sign_in)"
echo "[$L] GET /dashboard (html) -> $(code -A "$UA" -b $J $B/dashboard) $(page) $(wc -c <"$W/body")B"; cp "$W/body" "${OUT:-$HOME/.cache/compile-cmp/capture}/$L-dash.html"
echo "[$L] GET /dashboard (xhr)  -> $(code -A "$UA" -b $J -H 'X-Inertia: true' -H 'X-Requested-With: XMLHttpRequest' $B/dashboard) $(page) $(wc -c <"$W/body")B"; cp "$W/body" "${OUT:-$HOME/.cache/compile-cmp/capture}/$L-dash.json"
tok=$(sed 's/^#HttpOnly_//' $J | awk '$6=="session_token"{print $7}')
echo "[$L] forged session cookie -> $(code -A "$UA" -H "Cookie: session_token=1" $B/dashboard)   (must redirect to /sign_in)"
echo "[$L] no cookie /dashboard  -> $(code -A "$UA" $B/dashboard)"
echo "[$L] cookie=$(echo "$tok" | cut -c1-24)..."
