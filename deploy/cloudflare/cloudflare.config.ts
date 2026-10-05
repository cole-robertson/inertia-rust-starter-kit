// A Worker that forwards every request to one Container running the app's Docker image (CSR
// build). deploy.sh builds and pushes the image and passes its registry reference in IMAGE_REF,
// and the domain and optional demo login from deploy/cloudflare/.env.local (or the
// environment); see docs/DEPLOY_CLOUDFLARE.md.
import { bindings, defineConfig, defineContainer, defineWorker, exports } from "cf/config"

const image = process.env.IMAGE_REF
if (!image) throw new Error("IMAGE_REF is not set; deploy with deploy/cloudflare/deploy.sh")
const domain = process.env.CF_DOMAIN
if (!domain) throw new Error("CF_DOMAIN is not set; deploy with deploy/cloudflare/deploy.sh")
// A login created at every boot, for a public demo: only when deploy.sh has one.
const demoEmail = process.env.DEMO_ADMIN_EMAIL

const app = defineContainer({
  name: "inertia-rust",
  image: { reference: image },
  // One instance: one SQLite database.
  maxInstances: 1,
  // 1/4 vCPU, 1 GiB. `lite` (1/16 vCPU) makes each argon2 sign-in take seconds.
  instanceType: "basic",
})

const worker = defineWorker({
  name: "inertia-rust",
  compatibilityDate: "2026-05-01",
  entrypoint: "src/index.ts",
  observability: { enabled: true },
  domains: [domain],
  exports: {
    App: exports.durableObject({ storage: "sqlite", container: app }),
  },
  env: {
    HOST: bindings.text(`https://${domain}`),
    ...(demoEmail && { DEMO_ADMIN_EMAIL: bindings.text(demoEmail) }),
    SECRET_KEY_BASE: bindings.secret(),
    ...(demoEmail && { DEMO_ADMIN_PASSWORD: bindings.secret() }),
    APP: bindings.durableObject({ worker: "inertia-rust", exportName: "App" }),
  },
})

export default defineConfig({ worker, containers: [app] })
