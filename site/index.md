---
layout: home
title: "Inertia Rust: React pages, Rust server, no API"
titleTemplate: false
description: "An Inertia.js v3 + React 19 starter kit on a Rust server (Loco): auth, accounts, live updates and generators built in. One binary, SQLite, no API layer."

hero:
  name: Inertia Rust
  text: React pages. Rust server. No API.
  tagline: "An Inertia.js starter kit for Rust: React 19 and shadcn/ui pages, served by a Loco server with auth, accounts, live updates and generators built in. One binary, SQLite, and nothing to keep in sync between them."
  image:
    src: /mark.svg
    alt: Inertia Rust
  actions:
    - theme: brand
      text: Use this template
      link: https://github.com/cole-robertson/inertia-rust-starter-kit/generate
    - theme: alt
      text: Try the live demo
      link: https://demo.inertia-rust.dev
    - theme: alt
      text: Read the guide
      link: /guide/

features:
  - icon: { src: /features/accounts.svg, width: 48, height: 48 }
    title: Accounts built in
    details: "Organizations, members with roles and email invitations. Routes live under /{account_slug}, and every generated query is scoped to the account."
    link: /guide/accounts
    linkText: Accounts guide
  - icon: { src: /features/live-updates.svg, width: 48, height: 48 }
    title: Live updates
    details: "The kit's Action Cable: channels, broadcast_to, presence and perform over Server-Sent Events. Broadcast after a write; the page reloads its props."
    link: /guide/live-updates
    linkText: Live updates guide
  - icon: { src: /features/generators.svg, width: 48, height: 48 }
    title: Generators
    details: "cargo loco generate scaffold writes the migration, model, controller, routes, React pages and tests, scoped to the account by default."
    link: /guide/new-resource
    linkText: Scaffold guide
  - icon: { src: /features/forms.svg, width: 48, height: 48 }
    title: Forms and Precognition
    details: "Rails-style validation messages as an error bag, Inertia's useForm, and Precognition to validate as you type."
    link: /guide/forms-and-validation
    linkText: Forms guide
  - icon: { src: /features/ssr.svg, width: 48, height: 48 }
    title: SSR when you want it
    details: "Off by default. Turn it on and the Rust binary starts and supervises the Node renderer, falling back to client rendering on timeout."
    link: /reference/inertia#ssr-ssrrs
    linkText: SSR reference
  - icon: { src: /features/jobs-mail.svg, width: 48, height: 48 }
    title: Jobs, mail, scheduler
    details: "A job queue on SQLite (no Redis), mailers, and scheduled tasks, all run by the same binary."
    link: /guide/background-job
    linkText: Background jobs guide
  - icon: { src: /features/agent-skills.svg, width: 48, height: 48 }
    title: Agent skills
    details: "AGENTS.md plus two skills in .claude/skills: Loco with its full API index, and one recipe per task for extending this app."
    link: /guide/agents
    linkText: For coding agents
  - icon: { src: /features/deploy.svg, width: 48, height: 48 }
    title: One-binary deploy
    details: "Kamal, Cloudflare Containers, Docker Compose, systemd, Fly.io or Render. One Rust binary serves the app and runs the job queue."
    link: /guide/deploy
    linkText: Deploy guide
---

<div class="home-section">

## The Inertia way

<div class="steps">
<div>
<strong>No API layer</strong>
<p>Controllers render pages, not JSON. Inertia sends the page's props on navigation and React renders them.</p>
</div>
<div>
<strong>Props from Rust to React</strong>
<p>A controller passes props to <code>render(inertia, "projects/index", …)</code>. Their TypeScript types are generated from the Rust structs, and routes from one route table.</p>
</div>
<div>
<strong>Mutations redirect</strong>
<p>Forms post, the controller validates and redirects with a flash or an error bag. Errors show up on the right fields, no client-side state to manage.</p>
</div>
</div>

Coming from Rails? The kit follows the [Inertia Rails React Starter Kit](https://github.com/inertia-rails/react-starter-kit), and [Rails → Loco](/reference/rails-to-loco) maps each `rails` command to its equivalent here.

<img class="screenshot light-only" src="/repo/docs/screenshots/home-desktop-light.png" alt="The kit's home page, light theme" width="1280" height="800">
<img class="screenshot dark-only" src="/repo/docs/screenshots/home-desktop-dark.png" alt="The kit's home page, dark theme" width="1280" height="800">

</div>

<div class="home-section">

## Get started

Click [Use this template](https://github.com/cole-robertson/inertia-rust-starter-kit/generate) on GitHub, or clone it. You need Rust ([rustup](https://rustup.rs)) and Node 22.

<<< @/snippets/quick-start.sh

Open http://localhost:5150 and sign in as `one@example.com` / `Secret1*3*5*`. Then [build your app](/guide/building-your-app): rename it, scaffold a resource, add jobs and mail, deploy.

</div>

<div class="home-section">

## The numbers

<p class="lede">Both kits as production Docker images on the same workstation, 4 pinned CPUs each, SQLite, SSR off, Rails with YJIT and Puma 4×3. Medians of 5 runs, 2026-10-04. <a href="/reference/benchmark">The full benchmark</a>.</p>

<div class="stats">
<div class="stat"><div class="value">57 ms</div><div class="label">boot to first response, vs 1.6 s for the Rails kit</div></div>
<div class="stat"><div class="value">5–8× less</div><div class="label">memory: 42 / 65 MiB idle / after load, vs 210 / 509 MiB</div></div>
<div class="stat"><div class="value">4.5–11×</div><div class="label">requests per second, e.g. 26,435 vs 5,928 on the signed-in page (Inertia visit)</div></div>
<div class="stat"><div class="value">2.1 ms</div><div class="label">p99 on the signed-in page, vs 11.2 ms (5.4× lower)</div></div>
<div class="stat"><div class="value">52 MB</div><div class="label">compressed Docker image, vs 207 MB</div></div>
</div>

</div>

<div class="home-section">

## FAQ

### Why Rust?

For one small binary that boots in milliseconds, uses a fraction of the memory and serves more requests per core, with the compiler checking the server code and typed routes shared with React. [Loco](https://loco.rs) brings Rails' shape to Rust: generators, migrations, jobs, mailers, tasks. The trade is slower builds and no autoloading; see [the benchmark](/reference/benchmark) for both sides.

### Where does it come from?

It started as a port of the [Inertia Rails React Starter Kit](https://github.com/inertia-rails/react-starter-kit), and it still matches its routes, pages and messages, checked by a test that compares both kits' responses. Organizations are added on top. Every intended difference is listed in [Parity with the Rails kit](/reference/parity).

### Can I use Postgres?

Not out of the box. The kit ships SQLite (WAL mode) for the app and its job queue, and the benchmark, the deploy targets and some code (`src/db.rs`) assume it. Loco and SeaORM support Postgres, so a switch is possible, but you'd be doing it yourself.

### Do I need to know Rust?

You'll read and write some. The generators write most of the server code, the pages are plain React and TypeScript, and `AGENTS.md` plus the kit's skills give a coding agent the exact steps for each task. Rails experience carries over: [Rails → Loco](/reference/rails-to-loco).

### Is it production-ready?

It's v0.1. It has tests (Rust request and protocol tests, Playwright in client- and server-rendered modes), a security review and CI on every change, and the [live demo](https://demo.inertia-rust.dev) runs it. But it's new, it has few users yet, and SQLite means one app server per database. Read the [changelog](/reference/changelog) and judge for yourself.

### Why accounts by default?

Most business software keeps data per organization, and accounts are cheap to keep but expensive to add to an app that already has data. If your app has no teams, the accounts guide shows [how to flatten it](/guide/accounts#single-user-apps).

</div>
