#!/usr/bin/env bash
# seed.sh DBFILE SCHEMA.sql  -> fresh SQLite DB with the one benchmark user (same bcrypt-12
# digest Rails produced for bench-password-1), so every lane authenticates the same row.
set -euo pipefail
db=$1; schema=$2
rm -f "$db" "$db"-wal "$db"-shm; mkdir -p "$(dirname "$db")"
sqlite3 "$db" < "$schema"
now=$(date -u +'%Y-%m-%d %H:%M:%S.000000')
sqlite3 "$db" "PRAGMA journal_mode=WAL; INSERT INTO users (name,email,password_digest,verified,created_at,updated_at)
  VALUES ('Bench','bench@example.com','\$2a\$12\$wmZQgvAFNzzyArAZ1c10r.osYBuL14vhMF4eGA37/BWVpj1tVTTNy',0,'$now','$now');" >/dev/null
