// The repo markdown the site serves: [source in the repo, page under site/, sidebar label].
// scripts/sync.mjs copies each source to its page at build time (the pages are git-ignored), so
// the repo's files stay the only copy. Links to anything else in the repo become GitHub links.
export const REPO = "https://github.com/cole-robertson/inertia-rust-starter-kit"
export const SITE = "https://inertia-rust.dev"

const recipe = (name, label) => [
  `.claude/skills/starter-kit/recipes/${name}.md`,
  `guide/${name}.md`,
  label,
]

export const sections = [
  {
    text: "Getting started",
    pages: [
      ["README.md", "guide/index.md", "Introduction"],
      ["docs/BUILDING_YOUR_APP.md", "guide/building-your-app.md", "Building your app"],
      ["AGENTS.md", "guide/agents.md", "For coding agents"],
    ],
  },
  {
    text: "Guides",
    pages: [
      recipe("accounts", "Accounts and roles"),
      recipe("new-resource", "A new resource (scaffold)"),
      recipe("inertia-page", "Inertia pages and props"),
      recipe("forms-and-validation", "Forms and validation"),
      recipe("live-updates", "Live updates"),
      recipe("background-job", "Background jobs"),
      recipe("mailer", "Mail"),
      recipe("scheduled-task", "Scheduled tasks"),
      recipe("file-uploads", "File uploads"),
      recipe("cache", "Cache"),
      recipe("performance", "Performance"),
      recipe("admin", "Admin area (sketch)"),
      recipe("billing", "Billing (sketch)"),
      recipe("deploy", "Deploy"),
    ],
  },
  {
    text: "Reference",
    pages: [
      ["docs/RAILS_TO_LOCO.md", "reference/rails-to-loco.md", "Rails → Loco commands"],
      ["docs/INERTIA.md", "reference/inertia.md", "Inertia adapter"],
      ["docs/INERTIA_SECURITY.md", "reference/inertia-security.md", "Cookies, CSRF, headers"],
      ["docs/INERTIA_CONTRACT.md", "reference/inertia-contract.md", "Adapter module contract"],
      ["docs/DEPLOY.md", "reference/deploy.md", "Deploy targets"],
      ["docs/DEPLOY_CLOUDFLARE.md", "reference/deploy-cloudflare.md", "Cloudflare Containers"],
      ["docs/BENCHMARK.md", "reference/benchmark.md", "Benchmark vs Rails"],
      ["docs/PROFILING.md", "reference/profiling.md", "Profiling"],
      ["docs/PARITY.md", "reference/parity.md", "Parity with the Rails kit"],
      ["CHANGELOG.md", "reference/changelog.md", "Changelog"],
    ],
  },
]

export const pages = sections.flatMap((s) => s.pages)

/** site page (e.g. guide/accounts.md) -> repo source (e.g. .claude/.../accounts.md) */
export const sourceOf = Object.fromEntries(pages.map(([src, page]) => [page, src]))
