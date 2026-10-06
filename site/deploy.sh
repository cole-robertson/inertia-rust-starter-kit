#!/usr/bin/env bash
# Build inertia-rust.dev and upload it to Cloudflare Pages (project inertia-rust-site).
#
#   site/deploy.sh                 build, check links, upload to production
#   site/deploy.sh --branch NAME   upload as a preview deployment instead
#
# Needs Node 22+ and wrangler logged in to the Cloudflare account that owns the project
# (`npx wrangler login`, or CLOUDFLARE_API_TOKEN + CLOUDFLARE_ACCOUNT_ID).
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

project="${PAGES_PROJECT:-inertia-rust-site}"
branch=main
if [ "${1:-}" = "--branch" ]; then
  branch="${2:?--branch needs a name}"
fi

npm ci
npm run build
npx wrangler pages deploy .vitepress/dist --project-name "$project" --branch "$branch" --commit-dirty=true
