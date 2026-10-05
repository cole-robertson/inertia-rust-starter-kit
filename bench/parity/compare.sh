#!/usr/bin/env bash
# The parity oracle: boot the Evil Martians Rails kit and this kit side by side, in
# production mode, each on a fresh empty database with its own SMTP sink, drive both through
# the same flows (bench/parity/probe.py) and diff the transcripts (bench/parity/diff.py).
# Differences not listed in bench/parity/allowed.json (see docs/PARITY.md) fail the run.
#
#   RAILS_KIT=~/.cache/parity/rails bench/parity/compare.sh
#
# Needs: python3 (standard library only), sqlite3, Ruby for the Rails kit (bundled, with
# `assets:precompile` done), and this kit's release binary + `npx vite build` output.
# The Rails kit gets one extra initializer (config/initializers/parity_oracle.rb, written by
# this script) that sends its mail to the sink when PARITY_SMTP_PORT is set; nothing else
# in it changes.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

RAILS_KIT="${RAILS_KIT:?set RAILS_KIT to a checkout of inertia-rails/react-starter-kit}"
RUBY_BIN="${RUBY_BIN:-}"
RUST_PORT="${RUST_PORT:-5320}"
RAILS_PORT="${RAILS_PORT:-5321}"
RUST_SMTP="${RUST_SMTP:-2530}"
RAILS_SMTP="${RAILS_SMTP:-2531}"
out="${PARITY_OUT:-tmp/parity}"
bin="${PARITY_BIN:-target/release/inertia_rust_starter_kit-cli}"

rm -rf "$out"
mkdir -p "$out/rust"
out="$(cd "$out" && pwd)"
secret="$(openssl rand -hex 64)"
pids=()
cleanup() {
  for p in "${pids[@]}"; do pkill -TERM -P "$p" 2> /dev/null || true; kill "$p" 2> /dev/null || true; done
  for port in "$RUST_PORT" "$RAILS_PORT" "$RUST_SMTP" "$RAILS_SMTP"; do fuser -k -TERM "$port/tcp" 2> /dev/null || true; done
  wait 2> /dev/null || true
}
trap cleanup EXIT

wait_up() {
  local i
  for i in $(seq 1 300); do
    curl -sf -o /dev/null "$1" && return 0
    sleep 0.1
  done
  echo "timed out waiting for $1" >&2
  return 1
}

python3 bench/parity/smtp_sink.py "$RUST_SMTP" "$out" &
pids+=($!)
python3 bench/parity/smtp_sink.py "$RAILS_SMTP" "$out" &
pids+=($!)

# --- this kit ------------------------------------------------------------------------
rust_env=(
  LOCO_ENV=production SECRET_KEY_BASE="$secret" HOST="http://127.0.0.1:$RUST_PORT"
  ALLOW_INSECURE_HTTP=true
  DATABASE_URL="sqlite://$out/rust/app.sqlite?mode=rwc"
  QUEUE_URL="sqlite://$out/rust/queue.sqlite?mode=rwc"
  MAILER_HOST=127.0.0.1 MAILER_PORT="$RUST_SMTP" MAILER_SECURE=false
  MAILER_USER=parity MAILER_PASSWORD=parity LOG_LEVEL=warn
)
env "${rust_env[@]}" "$bin" db migrate > "$out/rust-migrate.log" 2>&1
env "${rust_env[@]}" "$bin" start --server-and-worker --no-banner --binding 127.0.0.1 \
  --port "$RUST_PORT" > "$out/rust.log" 2>&1 &
pids+=($!)

# --- the Rails kit -------------------------------------------------------------------
cat > "$RAILS_KIT/config/initializers/parity_oracle.rb" << 'RUBY'
# Parity oracle only (bench/parity/compare.sh in the Rust kit): deliver mail to a local sink.
if ENV["PARITY_SMTP_PORT"]
  ActionMailer::Base.delivery_method = :smtp
  ActionMailer::Base.smtp_settings = { address: "127.0.0.1", port: ENV["PARITY_SMTP_PORT"].to_i, enable_starttls_auto: false }
end
RUBY
(
  cd "$RAILS_KIT"
  [ -n "$RUBY_BIN" ] && export PATH="$RUBY_BIN:$PATH"
  export RAILS_ENV=production SECRET_KEY_BASE="$secret" PARITY_SMTP_PORT="$RAILS_SMTP" \
    SOLID_QUEUE_IN_PUMA=true RAILS_LOG_LEVEL=warn PORT="$RAILS_PORT" INERTIA_SSR=false
  rm -f storage/production*.sqlite3*
  bin/rails db:prepare > "$out/rails-prepare.log" 2>&1
  exec bin/rails server -b 127.0.0.1 -p "$RAILS_PORT"
) > "$out/rails.log" 2>&1 &
pids+=($!)

wait_up "http://127.0.0.1:$RUST_PORT/up"
wait_up "http://127.0.0.1:$RAILS_PORT/up"

# A probe that crashes still writes the steps it got through; the diff then shows where.
probes=0
python3 bench/parity/probe.py "http://127.0.0.1:$RUST_PORT" "$out/$RUST_SMTP.mbox" \
  "$out/rust/app.sqlite" "$out/rust.json" || probes=1
python3 bench/parity/probe.py "http://127.0.0.1:$RAILS_PORT" "$out/$RAILS_SMTP.mbox" \
  "$RAILS_KIT/storage/production.sqlite3" "$out/rails.json" || probes=1

status=0
python3 bench/parity/diff.py "$out/rails.json" "$out/rust.json" bench/parity/allowed.json \
  > "$out/diff.txt" || status=$?
cat "$out/diff.txt"
if [ "$probes" != 0 ]; then
  echo "a probe failed (see the traceback above)" >&2
  exit 1
fi
exit "$status"
