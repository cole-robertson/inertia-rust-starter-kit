// Every request goes to ONE container instance running the app's Docker image, so there is
// exactly one SQLite database. See docs/DEPLOY_CLOUDFLARE.md.
import { Container } from "@cloudflare/containers"
import { env } from "cloudflare:workers"

interface Env {
  APP: DurableObjectNamespace<App>
  HOST: string
  // Bound only for a demo login (DEMO_ADMIN_* in deploy/cloudflare/.env.local).
  DEMO_ADMIN_EMAIL?: string
  // Worker secrets (deploy/cloudflare/deploy.sh sets them; never in git).
  SECRET_KEY_BASE: string
  DEMO_ADMIN_PASSWORD?: string
}

const vars = env as unknown as Env

export class App extends Container<Env> {
  // The image listens on PORT=80 by default, but it runs as a non-root user and Cloudflare's
  // runtime (unlike Docker's default) doesn't let it bind a port below 1024, so use 8080.
  defaultPort = 8080
  // Sleep after 6 idle hours: a waking container costs ~1 s on the first request and, because
  // the disk is ephemeral, resets the database. Awake, `basic` costs about $0.01/hour
  // (1 GiB memory + 4 GB disk, CPU billed only when used); asleep, nothing.
  sleepAfter = "6h"
  envVars = {
    PORT: "8080",
    SECRET_KEY_BASE: vars.SECRET_KEY_BASE,
    HOST: vars.HOST,
    // With both set, bin/docker-entrypoint creates that user at boot.
    ...(vars.DEMO_ADMIN_EMAIL &&
      vars.DEMO_ADMIN_PASSWORD && {
        DEMO_ADMIN_EMAIL: vars.DEMO_ADMIN_EMAIL,
        DEMO_ADMIN_PASSWORD: vars.DEMO_ADMIN_PASSWORD,
      }),
    LOG_LEVEL: "info",
  }
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    // The app takes the client IP from the rightmost X-Forwarded-For entry (rate limiting,
    // session records). This Worker is the only way in, so it sets that entry itself.
    const headers = new Headers(request.headers)
    headers.set("X-Forwarded-For", request.headers.get("CF-Connecting-IP") ?? "0.0.0.0")
    return env.APP.getByName("app").fetch(new Request(request, { headers }))
  },
} satisfies ExportedHandler<Env>
