// The SSR entry: `npx vite build --ssr` bundles it into ssr/ssr.js (the app spawns
// `node ssr/ssr.js`), and the Vite dev server renders through it at /__inertia_ssr.
// @inertiajs/vite rewrites the `createServer` call below: it becomes the module's
// default export, and the HTTP server only starts in the production bundle.
import { createInertiaApp } from "@inertiajs/react"
import createServer from "@inertiajs/react/server"
import { renderToString } from "react-dom/server"

import { defaults, layout, serverHead, title } from "@/entrypoints/app"

// Node's global; the app tsconfig has only browser types.
declare const process: { env: Record<string, string | undefined> }

const render = await createInertiaApp({
  title,
  strictMode: true,
  pages: "../pages",
  layout,
  serverHead,
  defaults,
})

if (!render) throw new Error("createInertiaApp returned no SSR render function")
type RenderedPage = Parameters<typeof render>[0]

createServer((page) => render(page as RenderedPage, renderToString), {
  // Loopback only: /render and /shutdown are unauthenticated. The port is read at
  // startup (the app passes the port of settings.ssr.url), not baked into the bundle.
  host: "127.0.0.1",
  port: Number(process.env.INERTIA_SSR_PORT ?? 13714),
})
