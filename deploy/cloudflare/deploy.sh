#!/usr/bin/env bash
# Deploy (or redeploy) the app to Cloudflare Containers.
# docs/DEPLOY_CLOUDFLARE.md explains every step and how to tear it down.
#
#   deploy/cloudflare/deploy.sh            build, push and deploy
#   deploy/cloudflare/deploy.sh --dry-run  only write the Worker/Container config (`cf build`,
#                                          into deploy/cloudflare/.cloudflare/output): no image,
#                                          no login, nothing deployed
#
# Needs: docker, the `cf` CLI logged in (`cf auth whoami`), Node >= 22 with npm.
#
# Settings come from the environment or deploy/cloudflare/.env.local (git-ignored; copy
# .env.example). A variable already set in the environment wins over the file.
#
#   CF_ACCOUNT_ID        required: the Cloudflare account that owns the zone
#   CF_DOMAIN            required: the Worker's custom domain, e.g. app.example.com (HOST is
#                        https://$CF_DOMAIN)
#   DEMO_ADMIN_EMAIL     optional, with DEMO_ADMIN_PASSWORD: a login created at every boot,
#   DEMO_ADMIN_PASSWORD  for a public demo. Leave both unset for a real app.
#   IMAGE_TAG            image tag (default: the short git SHA of HEAD)
#   BUILD_CPUS           docker build --cpuset-cpus (default 0-3)
#   SKIP_IMAGE           set to 1 to reuse an already pushed inertia-rust:$IMAGE_TAG
#   SECRET_KEY_BASE      default: kept from the last deploy (deploy/cloudflare/.secrets.env,
#                        git-ignored), generated with bin/secret on the first one
#
# The image is built from `git archive HEAD` (committed code only, never the working tree)
# in ~/.cache/inertia-rust-deploy.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
dry_run=false
case "${1:-}" in
  --dry-run) dry_run=true ;;
  "") ;;
  *) echo "usage: $0 [--dry-run]" >&2; exit 64 ;;
esac

settings=(CF_ACCOUNT_ID CF_DOMAIN DEMO_ADMIN_EMAIL DEMO_ADMIN_PASSWORD)
if [ -f "$here/.env.local" ]; then
  from_env="$(declare -p "${settings[@]}" 2>/dev/null || true)"
  # shellcheck source=/dev/null
  . "$here/.env.local"
  eval "$from_env"
fi
missing=()
for name in CF_ACCOUNT_ID CF_DOMAIN; do
  [ -n "${!name:-}" ] || missing+=("$name")
done
if [ "${#missing[@]}" -gt 0 ]; then
  echo "deploy/cloudflare/deploy.sh: ${missing[*]} not set. Copy deploy/cloudflare/.env.example to deploy/cloudflare/.env.local and fill it in, or export them." >&2
  exit 1
fi
if { [ -n "${DEMO_ADMIN_EMAIL:-}" ] && [ -z "${DEMO_ADMIN_PASSWORD:-}" ]; } ||
   { [ -z "${DEMO_ADMIN_EMAIL:-}" ] && [ -n "${DEMO_ADMIN_PASSWORD:-}" ]; }; then
  echo "deploy/cloudflare/deploy.sh: set both DEMO_ADMIN_EMAIL and DEMO_ADMIN_PASSWORD for a demo login, or neither." >&2
  exit 1
fi
# cloudflare.config.ts reads these; DEMO_ADMIN_EMAIL only when there is a demo login.
export CF_DOMAIN
if [ -n "${DEMO_ADMIN_EMAIL:-}" ]; then export DEMO_ADMIN_EMAIL; else unset DEMO_ADMIN_EMAIL; fi

cf="${CF:-cf}"
account="$CF_ACCOUNT_ID"
tag="${IMAGE_TAG:-$(git -C "$root" rev-parse --short HEAD)}"
work="${HOME}/.cache/inertia-rust-deploy"
secrets="$here/.secrets.env"

# `cf deploy` bundles the Worker with Node; it needs >= 22.18. Fail before the slow image build.
node_major_minor="$(node -p 'process.versions.node.split(".").slice(0,2).join(".")' 2>/dev/null || echo 0.0)"
if ! awk -v v="$node_major_minor" 'BEGIN{split(v,a,"."); exit !(a[1]>22 || (a[1]==22 && a[2]>=18))}'; then
  echo "Node >= 22.18 is required (found ${node_major_minor}); e.g. PATH=\"\$(mise where node@24)/bin:\$PATH\" $0" >&2
  exit 1
fi

if [ "$dry_run" = true ]; then
  cd "$here"
  npm ci --no-fund --no-audit
  IMAGE_REF="registry.cloudflare.com/$account/inertia-rust:$tag" "$cf" build
  echo "Dry run: config for https://$CF_DOMAIN written to deploy/cloudflare/.cloudflare/output; nothing built or deployed."
  exit 0
fi

"$cf" auth whoami >/dev/null || { echo "cf is not logged in: run cf auth login" >&2; exit 1; }

# 1. Image: CSR build (the Dockerfile default) from committed code.
rm -rf "$work/src"; mkdir -p "$work/src"
git -C "$root" archive HEAD | tar -x -C "$work/src"
if [ -z "${SKIP_IMAGE:-}" ]; then
  docker build --cpuset-cpus "${BUILD_CPUS:-0-3}" --platform linux/amd64 -t "inertia-rust:$tag" "$work/src"
  "$cf" containers push -t "inertia-rust:$tag"
fi

# 2. Secrets: uploaded with the Worker version, never written to git.
if [ ! -f "$secrets" ]; then
  ( umask 077; printf 'SECRET_KEY_BASE=%s\n' "${SECRET_KEY_BASE:-$("$root/bin/secret")}" >"$secrets" )
fi
grep -q '^SECRET_KEY_BASE=' "$secrets" || { echo "$secrets has no SECRET_KEY_BASE" >&2; exit 1; }
tmp_secrets="$work/secrets.env"
trap 'rm -f "$tmp_secrets"' EXIT
( umask 077
  { grep '^SECRET_KEY_BASE=' "$secrets"
    if [ -n "${DEMO_ADMIN_EMAIL:-}" ]; then
      printf 'DEMO_ADMIN_PASSWORD=%s\n' "$DEMO_ADMIN_PASSWORD"
    fi; } >"$tmp_secrets" )

# 3. Worker + Container, with the image just pushed (cloudflare.config.ts reads IMAGE_REF).
cd "$here"
npm ci --no-fund --no-audit
IMAGE_REF="registry.cloudflare.com/$account/inertia-rust:$tag" \
  "$cf" deploy --secrets-file "$tmp_secrets" --containers-rollout immediate --message "inertia-rust $tag"

echo "Deployed inertia-rust:$tag to https://$CF_DOMAIN (first boot takes a few seconds)."
