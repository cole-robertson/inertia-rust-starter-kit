import { URL, fileURLToPath } from "node:url"

import inertia from "@inertiajs/vite"
import babel from "@rolldown/plugin-babel"
import tailwindcss from "@tailwindcss/vite"
import react, { reactCompilerPreset } from "@vitejs/plugin-react"
import { createLogger, defineConfig } from "vite"

const ENTRYPOINT = "frontend/entrypoints/inertia.tsx"
const SSR_ENTRYPOINT = "frontend/entrypoints/ssr.tsx"
const STYLESHEET = "frontend/entrypoints/application.css"

// Mirrors is_sensitive_param in src/inertia/request_log.rs: sid, token,
// password*, *_token (innermost key of user[password]-style names).
const SENSITIVE_PAIR = /([?&])([^=&?#\s"'<>\p{Cc}]+)=([^&#\s"'<>\p{Cc}]+)/gu

function isSensitiveParam(raw: string) {
  let name = raw
  try {
    name = decodeURIComponent(raw.replace(/\+/g, " "))
  } catch {
    // Malformed escapes: judge the raw name.
  }
  name = name.toLowerCase()
  const inner = name.split("[").pop()?.replace(/\]+$/, "") ?? name
  return [name, inner].some(
    (n) =>
      n === "sid" ||
      n === "token" ||
      n.startsWith("password") ||
      n.endsWith("_token"),
  )
}

/** `text` with sensitive query values replaced by [FILTERED]. */
export function redactSensitiveQueryValues(text: string) {
  return text.replace(SENSITIVE_PAIR, (pair, sep: string, name: string) =>
    isSensitiveParam(name) ? `${sep}${name}=[FILTERED]` : pair,
  )
}

// Vite's dev SSR endpoint logs render errors with the page URL (a password-reset
// link's ?sid=...) through this logger, so every message is redacted first.
function redactingLogger() {
  const logger = createLogger()
  const redacted: typeof logger = {
    ...logger,
    info: (msg, options) =>
      logger.info(redactSensitiveQueryValues(msg), options),
    warn: (msg, options) =>
      logger.warn(redactSensitiveQueryValues(msg), options),
    warnOnce: (msg, options) =>
      logger.warnOnce(redactSensitiveQueryValues(msg), options),
    error: (msg, options) =>
      logger.error(redactSensitiveQueryValues(msg), options),
  }
  return redacted
}

export default defineConfig(({ command, isSsrBuild }) => {
  const port = Number(process.env.VITE_PORT ?? 5173)

  return {
    customLogger: redactingLogger(),
    // Built assets are served by the Rust app under /vite/.
    base: command === "build" ? "/vite/" : "/",
    // The Rust app serves public/ itself; don't copy it into the build output.
    publicDir: false,
    resolve: {
      alias: {
        "@": fileURLToPath(new URL("./frontend", import.meta.url)),
      },
    },
    ssr: {
      // Prebuild ssr.js so we can drop node_modules from the container.
      noExternal: command === "build" ? true : undefined,
      // React 19 ships CJS-only — externalize in dev so Node handles require natively.
      external:
        command === "serve"
          ? ["react", "react-dom", "react/jsx-runtime", "react/jsx-dev-runtime"]
          : undefined,
    },
    build: isSsrBuild
      ? {
          outDir: "ssr",
          emptyOutDir: true,
          manifest: false,
          rolldownOptions: {
            input: SSR_ENTRYPOINT,
            output: { entryFileNames: "ssr.js" },
          },
        }
      : {
          // Written to public/vite/.vite/manifest.json; the Rust app looks up
          // ENTRYPOINT and STYLESHEET in it by these exact keys.
          manifest: true,
          outDir: "public/vite",
          emptyOutDir: true,
          rolldownOptions: {
            input: [ENTRYPOINT, STYLESHEET],
          },
        },
    server: {
      port,
      strictPort: true,
      host: "localhost",
      // Absolute asset URLs (e.g. fonts in CSS) must point at the dev server,
      // not at the Rust app that serves the HTML.
      origin: `http://localhost:${port}`,
      hmr: { port },
    },
    plugins: [
      react(),
      babel({ presets: [reactCompilerPreset()] }),
      tailwindcss(),
      // SSR_ENTRYPOINT starts the built server itself, on 127.0.0.1 and the port
      // in INERTIA_SSR_PORT (read when node starts; the app passes ssr.url's port).
      inertia({ ssr: { entry: SSR_ENTRYPOINT } }),
    ],
  }
})
