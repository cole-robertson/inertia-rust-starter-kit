// Builds the site's generated inputs from the repo, so the repo's markdown stays the only copy.
// Everything written here is git-ignored (site/.gitignore). Run by `npm run dev` / `npm run build`.
//
// - pages.mjs's sources -> site/guide/*.md, site/reference/*.md, with links rewritten: a link to
//   another served page stays a site link, a link to any other repo file becomes a GitHub link
//   (and must exist, or this fails), and images are served from /repo/<path>.
// - README's controller/page example and quick start -> site/snippets/ for the home page.
// - The favicon and wordmark -> site/public/; the mark (the wordmark's gear) -> public/mark.svg.
// - /llms.txt (an index) and /llms-full.txt (every page, links made absolute).
import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"

import { REPO, SITE, pages, sections } from "../pages.mjs"

const site = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")
const root = path.resolve(site, "..")
const servedBySource = Object.fromEntries(pages.map(([src, page]) => [src, page]))
const IMAGE = /\.(svg|png|jpe?g|gif|webp)$/i

const problems = []
const write = (rel, text) => {
  const file = path.join(site, rel)
  fs.mkdirSync(path.dirname(file), { recursive: true })
  fs.writeFileSync(file, text)
}
const copyAsset = (repoPath) => {
  const to = path.join(site, "public/repo", repoPath)
  fs.mkdirSync(path.dirname(to), { recursive: true })
  fs.copyFileSync(path.join(root, repoPath), to)
  return `/repo/${repoPath}`
}

for (const dir of ["guide", "reference", "snippets", "public/repo"]) {
  fs.rmSync(path.join(site, dir), { recursive: true, force: true })
}

// Rewrites one link target found in `src` (a repo path). mode "site": relative to the page;
// mode "absolute": full URLs, for llms-full.txt.
function rewrite(target, src, page, mode, isImage) {
  if (/^([a-z][a-z0-9+.-]*:|#|\/\/)/i.test(target)) return target
  const [rawPath, hash = ""] = target.split(/(?=#)/)
  const repoPath = path.posix.normalize(path.posix.join(path.posix.dirname(src), rawPath))
  if (repoPath.startsWith("..")) {
    problems.push(`${src}: link outside the repo: ${target}`)
    return target
  }
  const onDisk = path.join(root, repoPath)
  if (!fs.existsSync(onDisk)) {
    problems.push(`${src}: link to a missing file: ${target}`)
    return target
  }
  const served = servedBySource[repoPath]
  if (served) {
    if (mode === "absolute") return `${SITE}/${served.replace(/(index)?\.md$/, "")}${hash}`
    return path.posix.relative(path.posix.dirname(page), served) + hash
  }
  if (isImage || IMAGE.test(repoPath)) {
    const url = copyAsset(repoPath)
    return mode === "absolute" ? `${SITE}${url}` : url
  }
  const kind = fs.statSync(onDisk).isDirectory() ? "tree" : "blob"
  return `${REPO}/${kind}/main/${repoPath.replace(/\/$/, "")}${hash}`
}

// HTML the docs use on purpose; any other `<` in prose (`"<app name> is …"`) is text, and Vue
// would read it as an unclosed tag.
const HTML = /^<\/?(a|details|summary|img|p|br|sup|sub)\b/i
const escapeProse = (part) =>
  part.replace(/<(?=[A-Za-z/!])/g, (lt, at, s) => (HTML.test(s.slice(at)) ? lt : "&lt;"))

// Applies `fn` to markdown links outside fenced code blocks and inline code spans, and escapes
// stray `<` there.
function rewriteLinks(text, fn) {
  const fence = /^(```|~~~)/
  let inFence = false
  return text
    .split("\n")
    .map((line) => {
      if (fence.test(line.trimStart())) {
        inFence = !inFence
        return line
      }
      if (inFence) return line
      // Code spans are left alone, but a link's label may itself be one: [`src/x.rs`](../src/x.rs).
      const spans = [...line.matchAll(/`+[^`]*`+/g)].map((m) => [m.index, m.index + m[0].length])
      const inCode = (at) => spans.some(([a, b]) => at >= a && at < b)
      const linked = line.replace(
        /(!?)\[((?:`[^`]*`|[^\]`])*)\]\(([^)\s]+)(\s+"[^"]*")?\)/g,
        (whole, bang, label, target, title = "", at) =>
          inCode(at) ? whole : `${bang}[${label}](${fn(target, bang === "!")}${title})`,
      )
      return linked
        .split(/(`+[^`]*`+)/)
        .map((part, i) => (i % 2 === 1 ? part : escapeProse(part)))
        .join("")
    })
    .join("\n")
}

function source(src) {
  let text = fs.readFileSync(path.join(root, src), "utf8")
  // The README's centred wordmark: the site has its own header.
  if (src === "README.md") text = text.replace(/^<p align="center"><img [^\n]*\n+/, "")
  // Agent skills' YAML front matter is for agents, not readers.
  return text.replace(/^---\n[\s\S]*?\n---\n+/, "")
}

const full = []
for (const [src, page, label] of pages) {
  const text = source(src)
  write(page, rewriteLinks(text, (t, img) => rewrite(t, src, page, "site", img)))
  full.push({ src, page, label, text: rewriteLinks(text, (t, img) => rewrite(t, src, page, "absolute", img)) })
}

// The home page's code: the README's example and quick start, verbatim.
const readme = fs.readFileSync(path.join(root, "README.md"), "utf8")
const block = (lang, after) => {
  const from = readme.indexOf(after)
  const m = from < 0 ? null : readme.slice(from).match(new RegExp("```" + lang + "\\n([\\s\\S]*?)```"))
  if (!m) problems.push(`README.md: no \`\`\`${lang} block after "${after}"`)
  return m ? m[1] : ""
}
write("snippets/controller.rs", block("rust", "A controller loads the data"))
write("snippets/page.tsx", block("tsx", "A controller loads the data"))
write("snippets/quick-start.sh", block("sh", "## Quick start"))

// Icons. The mark is the wordmark's first group (the gear and chevron), on its own canvas.
fs.copyFileSync(path.join(root, "public/icon.svg"), path.join(site, "public/icon.svg"))
fs.copyFileSync(path.join(root, "public/icon.png"), path.join(site, "public/icon.png"))
const wordmark = fs.readFileSync(path.join(root, "docs/logo/wordmark.svg"), "utf8")
fs.writeFileSync(path.join(site, "public/wordmark.svg"), wordmark)
const mark = wordmark.match(/<g transform="[^"]*">[\s\S]*?<\/g>/)
if (!mark) problems.push("docs/logo/wordmark.svg: no <g> holding the mark")
else
  fs.writeFileSync(
    path.join(site, "public/mark.svg"),
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 24" width="320" height="240">\n${mark[0]}\n</svg>\n`,
  )
copyAsset("docs/screenshots/home-desktop-light.png")
copyAsset("docs/screenshots/home-desktop-dark.png")

// llms.txt (https://llmstxt.org): title, summary, then the pages by section.
const summaryOf = (text) =>
  (text.replace(/^#[^\n]*\n+/, "").split(/\n\n/)[0] || "").replace(/\s+/g, " ").trim()
const url = (page) => `${SITE}/${page.replace(/(index)?\.md$/, "")}`
const llms = [
  "# Inertia Rust",
  "",
  "> An Inertia.js v3 + React 19 starter kit on Loco (Rust), ported from the Inertia Rails React",
  "> Starter Kit: auth, accounts (organizations), live updates, generators and agent skills, served",
  "> by one Rust binary with SQLite.",
  "",
  `Source: ${REPO}. Live demo: https://demo.inertia-rust.dev. All docs in one file: ${SITE}/llms-full.txt`,
  "",
]
for (const section of sections) {
  llms.push(`## ${section.text}`, "")
  for (const [, page, label] of section.pages) {
    const entry = full.find((f) => f.page === page)
    const summary = summaryOf(entry.text).replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    llms.push(`- [${label}](${url(page)}): ${summary.length > 220 ? summary.slice(0, 217) + "..." : summary}`)
  }
  llms.push("")
}
write("public/llms.txt", llms.join("\n"))
write(
  "public/llms-full.txt",
  full.map((f) => `<!-- ${url(f.page)} (source: ${REPO}/blob/main/${f.src}) -->\n\n${f.text.trim()}\n`).join("\n\n"),
)

if (problems.length) {
  console.error(`sync: ${problems.length} problem(s):\n  ${problems.join("\n  ")}`)
  process.exit(1)
}
console.log(`sync: ${pages.length} pages, llms.txt, llms-full.txt`)
