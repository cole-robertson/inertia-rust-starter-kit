#!/usr/bin/env bash
# Boot stock Rails (the slice), the Roundhouse ruby emit and the Spinel binary side by side
# and run probe.sh on each. Run from ~/.cache/compile-cmp; RUBY_BIN is Ruby 4.0.6's bin dir.
RUBY_BIN=${RUBY_BIN:-$HOME/.local/share/mise/installs/ruby/4.0.6/bin}
export LANG=C.UTF-8 LC_ALL=C.UTF-8 PATH=$HOME/.cache/compile-cmp/spinel/bin:$RUBY_BIN:$PATH
C=~/.cache/compile-cmp; fuser -k 5340/tcp 5341/tcp 5342/tcp >/dev/null 2>&1; sleep 0.5
cd $C/kit-slice && S=$(openssl rand -hex 64); (RAILS_ENV=production SECRET_KEY_BASE=$S RAILS_LOG_LEVEL=error timeout 30 bin/rails server -b 127.0.0.1 -p 5340 > $C/logs/slice-rails-run.log 2>&1 &)
cd $C/out/slice-ruby && $C/seed.sh storage/development.sqlite3 db/seed.sql && (PORT=5341 RAILS_ENV=production timeout 30 bundle exec puma -C config/puma.rb > $C/logs/slice-ruby-run.log 2>&1 &)
cd $C/out/slice-spinel && $C/seed.sh storage/development.sqlite3 db/seed.sql && (PORT=5342 timeout 30 ./build/bin/blog > $C/logs/slice-spinel-run.log 2>&1 &)
for p in 5340 5341 5342; do for i in $(seq 1 150); do curl -s -o /dev/null http://127.0.0.1:$p/up && break; sleep 0.1; done; done
for p in "5340 rails" "5341 rh-ruby" "5342 rh-spinel" ${EXTRA:-}; do $C/probe.sh $p; echo; done
for f in slice-ruby-run slice-spinel-run; do grep -v "^\s*from\|Warning\|^\*\|Ctrl\|listening\|green thread\|Puma starting" $C/logs/$f.log | sort | uniq -c | tail -4; done
