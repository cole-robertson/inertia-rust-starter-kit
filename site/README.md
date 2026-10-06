# inertia-rust.dev

The kit's landing page and docs, built with [VitePress](https://vitepress.dev). It isn't part of
the app: an app made from this template can delete `site/`.

The docs aren't copied into this directory. `scripts/sync.mjs` reads the repo's markdown at build
time (the list is in `pages.mjs`), rewrites links to files the site doesn't serve into GitHub
links, and writes `guide/`, `reference/`, `/llms.txt` and `/llms-full.txt` (all git-ignored).
Edit the source files, not those. `index.md` (the home page) is the only page written here.

```sh
cd site
npm ci
npm run dev       # http://localhost:5173
npm run build     # sync, build, then check every internal link and #anchor
./deploy.sh       # build and upload to Cloudflare Pages (project inertia-rust-site)
```

The build fails on a dead link or anchor; `.github/workflows/site.yml` runs it on pull requests
that touch the docs. The Open Graph image is `public/og.png`, rendered from `og/og.svg` with
`rsvg-convert -w 1200 -h 630 og/og.svg -o public/og.png`.
