import { defineConfig } from "vitepress"

import { REPO, SITE, sections, sourceOf } from "../pages.mjs"

// GitHub's heading ids: lowercase, drop punctuation, one dash per space.
function githubSlug(text: string) {
  return text
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\p{M}\s_-]/gu, "")
    .replace(/\s/g, "-")
}

const title = "Inertia Rust"
const description =
  "React pages, Rust server, no API: an Inertia.js v3 + React 19 starter kit on Loco. Auth, accounts, live updates, generators and agent skills, served by one Rust binary."

export default defineConfig({
  lang: "en-US",
  title,
  titleTemplate: ":title · Inertia Rust",
  description,
  cleanUrls: true,
  lastUpdated: false,
  // Dead links fail the build; only http://localhost:5150 and friends (the app you run) are exempt.
  ignoreDeadLinks: "localhostLinks",
  // The generated snippets are included into index.md, not pages of their own.
  srcExclude: ["snippets/**", "partials/**", "README.md", "og/**"],
  sitemap: { hostname: SITE },
  // The docs are written for GitHub and link to its heading anchors (`#flyio`,
  // `#single-binary--systemd`), so the site makes the same ids.
  markdown: { anchor: { slugify: githubSlug } },

  head: [
    ["link", { rel: "icon", type: "image/svg+xml", href: "/icon.svg" }],
    ["link", { rel: "apple-touch-icon", href: "/icon.png" }],
    ["meta", { name: "theme-color", content: "#CE422B" }],
    ["meta", { property: "og:type", content: "website" }],
    ["meta", { property: "og:site_name", content: title }],
    ["meta", { property: "og:image", content: `${SITE}/og.png` }],
    ["meta", { property: "og:image:width", content: "1200" }],
    ["meta", { property: "og:image:height", content: "630" }],
    ["meta", { name: "twitter:card", content: "summary_large_image" }],
    ["meta", { name: "twitter:image", content: `${SITE}/og.png` }],
  ],

  // Per-page title, description and canonical URL for Open Graph.
  transformPageData(pageData) {
    const path = pageData.relativePath.replace(/(^|\/)index\.md$/, "$1").replace(/\.md$/, "")
    const pageTitle = pageData.frontmatter.title || pageData.title
    const fullTitle = pageTitle && pageTitle !== title ? `${pageTitle} · ${title}` : `${title}: React pages, Rust server, no API`
    const desc = pageData.frontmatter.description || pageData.description || description
    pageData.frontmatter.editSource = sourceOf[pageData.relativePath] ?? `site/${pageData.relativePath}`
    pageData.frontmatter.head ??= []
    pageData.frontmatter.head.push(
      ["link", { rel: "canonical", href: `${SITE}/${path}` }],
      ["meta", { property: "og:url", content: `${SITE}/${path}` }],
      ["meta", { property: "og:title", content: fullTitle }],
      ["meta", { property: "og:description", content: desc }],
    )
  },

  themeConfig: {
    logo: { src: "/mark.svg", alt: "" },
    siteTitle: "Inertia Rust",
    nav: [
      { text: "Guide", link: "/guide/", activeMatch: "^/guide/" },
      { text: "Reference", link: "/reference/rails-to-loco", activeMatch: "^/reference/" },
      { text: "Demo", link: "https://demo.inertia-rust.dev" },
      {
        text: "v0.1",
        items: [
          { text: "Changelog", link: "/reference/changelog" },
          { text: "Releases", link: `${REPO}/releases` },
        ],
      },
    ],
    sidebar: {
      "/": sections.map((s) => ({
        text: s.text,
        items: s.pages.map(([, page, label]) => ({
          text: label,
          link: "/" + page.replace(/(index)?\.md$/, ""),
        })),
      })),
    },
    outline: { level: [2, 3] },
    search: { provider: "local" },
    socialLinks: [{ icon: "github", link: REPO }],
    editLink: {
      // The pages are copies made at build time; edit the repo file they come from
      // (transformPageData sets editSource). VitePress ships this function to the browser as
      // source text, so it can't use the imports above.
      pattern: ({ frontmatter }) =>
        `https://github.com/cole-robertson/inertia-rust-starter-kit/edit/main/${frontmatter.editSource}`,
      text: "Edit this page on GitHub",
    },
    footer: {
      message: "Released under the MIT License.",
      copyright: `Inertia Rust Starter Kit · <a href="${REPO}">GitHub</a>`,
    },
  },
})
