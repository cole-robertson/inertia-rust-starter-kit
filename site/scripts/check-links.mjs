// After `vitepress build`: every internal link in .vitepress/dist must reach a file, and its
// #anchor must be an id on that page. VitePress checks the first half; this adds the anchors.
import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"

const dist = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../.vitepress/dist")
const htmlFiles = []
const walk = (dir) => {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name)
    if (e.isDirectory()) walk(p)
    else if (p.endsWith(".html")) htmlFiles.push(p)
  }
}
walk(dist)

const ids = new Map()
const idsOf = (file) => {
  if (!ids.has(file)) {
    const html = fs.readFileSync(file, "utf8")
    ids.set(file, new Set([...html.matchAll(/\sid="([^"]+)"/g)].map((m) => m[1])))
  }
  return ids.get(file)
}
// /guide/accounts -> dist/guide/accounts.html; /guide/ -> dist/guide/index.html
const fileFor = (urlPath) => {
  const p = decodeURIComponent(urlPath)
  for (const c of [p, `${p}.html`, path.join(p, "index.html")]) {
    const f = path.join(dist, c)
    if (fs.existsSync(f) && fs.statSync(f).isFile()) return f
  }
  return null
}

const broken = []
let checked = 0
for (const file of htmlFiles) {
  const html = fs.readFileSync(file, "utf8")
  const page = "/" + path.relative(dist, file).replace(/(index)?\.html$/, "")
  for (const [, href] of html.matchAll(/\s(?:href|src)="([^"]+)"/g)) {
    if (/^([a-z][a-z0-9+.-]*:|\/\/)/i.test(href)) continue
    const url = new URL(href.replace(/&amp;/g, "&"), `https://site${page}`)
    const target = fileFor(url.pathname)
    checked++
    if (!target) broken.push(`${page}: ${href} (no file)`)
    else if (url.hash && target.endsWith(".html") && !idsOf(target).has(decodeURIComponent(url.hash.slice(1))))
      broken.push(`${page}: ${href} (no #${url.hash.slice(1)} there)`)
  }
}
if (broken.length) {
  console.error(`check-links: ${broken.length} broken of ${checked}:\n  ${[...new Set(broken)].join("\n  ")}`)
  process.exit(1)
}
console.log(`check-links: ${checked} internal links and anchors in ${htmlFiles.length} pages, all found`)
