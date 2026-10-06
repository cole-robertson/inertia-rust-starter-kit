---
layout: home
title: "Inertia Rust: Inertia Rails, in Rust"
titleTemplate: false
description: "The Inertia Rails starter kit, ported to Rust. React 19 pages, a Loco server with auth, accounts, live updates and generators, one binary and SQLite. No API layer."

hero:
  name: Inertia Rust
  text: Inertia Rails, in Rust.
  tagline: "The Inertia Rails starter kit, ported to Loco. React 19 and shadcn/ui pages, rendered by a Rust server that has auth, accounts, live updates and generators built in. One binary, SQLite, no API layer."
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
  - icon: 👥
    title: Accounts built in
    details: "Organizations, members with roles and email invitations. Routes live under /{account_slug}, and every generated query is scoped to the account."
    link: /guide/accounts
    linkText: Accounts guide
  - icon: ⚡
    title: Live updates
    details: "The kit's Action Cable: channels, broadcast_to, presence and perform over Server-Sent Events. Broadcast after a write; the page reloads its props."
    link: /guide/live-updates
    linkText: Live updates guide
  - icon: 🏗️
    title: Generators
    details: "cargo loco generate scaffold writes the migration, model, controller, routes, React pages and tests, scoped to the account by default."
    link: /guide/new-resource
    linkText: Scaffold guide
  - icon: 📝
    title: Forms and Precognition
    details: "Rails-style validation messages as an error bag, Inertia's useForm, and Precognition to validate as you type."
    link: /guide/forms-and-validation
    linkText: Forms guide
  - icon: 🖥️
    title: SSR when you want it
    details: "Off by default. Turn it on and the Rust binary starts and supervises the Node renderer, falling back to client rendering on timeout."
    link: /reference/inertia#ssr-ssrrs
    linkText: SSR reference
  - icon: 📬
    title: Jobs, mail, scheduler
    details: "A job queue on SQLite (no Redis), mailers, and scheduled tasks, all run by the same binary."
    link: /guide/background-job
    linkText: Background jobs guide
  - icon: 🤖
    title: Agent skills
    details: "AGENTS.md plus two skills in .claude/skills: Loco with its full API index, and one recipe per task for extending this app."
    link: /guide/agents
    linkText: For coding agents
  - icon: 📦
    title: One-binary deploy
    details: "Kamal, Cloudflare Containers, Docker Compose, systemd, Fly.io or Render. One Rust binary serves the app and runs the job queue."
    link: /guide/deploy
    linkText: Deploy guide
---

<div class="home-section">

## A controller and the page it renders

<p class="lede">The controller loads the data and hands it to a React page as props. There's no JSON API in between, and nothing to keep in sync.</p>

::: code-group

<<< @/snippets/controller.rs [Rust controller]

<<< @/snippets/page.tsx [React page]

:::

`cargo loco generate scaffold projects name:string!` writes both, plus the model, migration, routes and tests.

</div>

<div class="home-section">

## The Inertia way

<div class="steps">
<div>
<strong>No API layer</strong>
<p>Controllers render pages, not JSON. Inertia sends the page's props on navigation and React renders them.</p>
</div>
<div>
<strong>Props from Rust to React</strong>
<p>A controller passes props to <code>render(inertia, "projects/index", …)</code>. Routes are typed on both sides, generated from one route table.</p>
</div>
<div>
<strong>Mutations redirect</strong>
<p>Forms post, the controller validates and redirects with a flash or an error bag. The same loop as Rails, with the same messages.</p>
</div>
</div>

If you know [Inertia Rails](https://inertia-rails.dev), you already know how this app works: the routes, pages and flash messages match the [Inertia Rails React Starter Kit](https://github.com/inertia-rails/react-starter-kit) it was ported from. [Rails → Loco](/reference/rails-to-loco) maps every `rails` command to its equivalent here.

<img class="screenshot light-only" src="/repo/docs/screenshots/home-desktop-light.png" alt="The kit's home page, light theme" width="1280" height="800">
<img class="screenshot dark-only" src="/repo/docs/screenshots/home-desktop-dark.png" alt="The kit's home page, dark theme" width="1280" height="800">

</div>

<div class="home-section">

## The numbers

<p class="lede">Both kits as production Docker images on the same workstation, 4 pinned CPUs each, SQLite, SSR off, Rails with YJIT and Puma 4×3. Medians of 5 runs, 2026-10-04.</p>

<div class="stats">
<div class="stat"><div class="value">57 ms</div><div class="label">boot to first response, vs 1.6 s for the Rails kit</div></div>
<div class="stat"><div class="value">5–8× less</div><div class="label">memory: 42 / 65 MiB idle / after load, vs 210 / 509 MiB</div></div>
<div class="stat"><div class="value">4.5–11×</div><div class="label">requests per second, e.g. 26,435 vs 5,928 on the signed-in page (Inertia visit)</div></div>
<div class="stat"><div class="value">2.1 ms</div><div class="label">p99 on the signed-in page, vs 11.2 ms (5.4× lower)</div></div>
<div class="stat"><div class="value">52 MB</div><div class="label">compressed Docker image, vs 207 MB</div></div>
</div>

The caveats are real. Under a 4 GB container limit, 0.17% of SQLite writes from 32 writers failed with a 500 (pool timeouts during write stalls; none with 32 GB), where Rails had none. With SSR on, Node is the bottleneck and throughput is about equal. The Rails kit builds its image faster from cold and reloads code instantly. Method, ranges and every caveat: [the full benchmark](/reference/benchmark).

</div>

<div class="home-section">

## Get started

Click [Use this template](https://github.com/cole-robertson/inertia-rust-starter-kit/generate) on GitHub, or clone it. You need Rust ([rustup](https://rustup.rs)) and Node 22.

<<< @/snippets/quick-start.sh

Open http://localhost:5150 and sign in as `one@example.com` / `Secret1*3*5*`. Then [build your app](/guide/building-your-app): rename it, scaffold a resource, add jobs and mail, deploy.

</div>

<div class="home-section">

## FAQ

### Why Rust?

For one small binary that boots in milliseconds, uses a fraction of the memory and serves more requests per core, with the compiler checking the server code and typed routes shared with React. [Loco](https://loco.rs) brings Rails' shape to Rust: generators, migrations, jobs, mailers, tasks. The trade is slower builds and no autoloading; see [the benchmark](/reference/benchmark) for both sides.

### How close is it to the Rails kit?

It's a port of the [Inertia Rails React Starter Kit](https://github.com/inertia-rails/react-starter-kit): the same routes, pages, texts and flash messages, checked by a parity oracle that compares both kits' responses. Organizations are added on top. Every intended difference is listed in [Parity with the Rails kit](/reference/parity).

### Can I use Postgres?

Not out of the box. The kit ships SQLite (WAL mode) for the app and its job queue, and the benchmark, the deploy targets and some code (`src/db.rs`) assume it. Loco and SeaORM support Postgres, so a switch is possible, but you'd be doing it yourself.

### Do I need to know Rust?

You'll read and write some. The generators write most of the server code, the pages are plain React and TypeScript, and `AGENTS.md` plus the kit's skills give a coding agent the exact steps for each task. Rails experience carries over: [Rails → Loco](/reference/rails-to-loco).

### Is it production-ready?

It's v0.1. It has tests (Rust request and protocol tests, Playwright in client- and server-rendered modes), a security review and CI on every change, and the [live demo](https://demo.inertia-rust.dev) runs it. But it's new, it has few users yet, and SQLite means one app server per database. Read the [changelog](/reference/changelog) and judge for yourself.

### Why accounts by default?

Most business software keeps data per organization, and accounts are cheap to keep but expensive to add to an app that already has data. If your app has no teams, the accounts guide shows [how to flatten it](/guide/accounts#single-user-apps).

</div>
