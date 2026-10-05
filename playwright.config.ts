import { defineConfig, devices } from "@playwright/test"

// The whole suite runs twice: against a client-rendered server and against one that
// renders through the real ssr/ssr.js bundle (e2e/rendering.spec.ts asserts which).
const baseURL = process.env.E2E_BASE_URL ?? "http://127.0.0.1:5190"
const ssrBaseURL = process.env.E2E_SSR_BASE_URL ?? "http://127.0.0.1:5191"
// Both servers run with server-managed head tags on (the kit default is off), and
// the first one builds the client and SSR bundle with the same setting, so
// e2e/head.spec.ts sees the tags in the initial HTML and after navigation.
const serverHead = { VITE_INERTIA_SERVER_HEAD: "true" }

export default defineConfig({
  testDir: "e2e",
  outputDir: "tmp/playwright",
  retries: process.env.CI ? 1 : 0,
  // PW_WORKERS caps parallelism on a shared build host; Playwright's default otherwise.
  workers: process.env.PW_WORKERS ? Number(process.env.PW_WORKERS) : undefined,
  reporter: "list",
  use: {
    trace: "retain-on-failure",
  },
  projects: [
    // one@ and two@ sign in once per server (e2e/auth.setup.ts, used through e2e/fixtures.ts):
    // sign-in is rate limited per IP, 10 per 3 minutes, and every worker is 127.0.0.1.
    {
      name: "csr-auth",
      testMatch: /auth\.setup\.ts/,
      use: { ...devices["Desktop Chrome"], baseURL },
    },
    {
      name: "ssr-auth",
      testMatch: /auth\.setup\.ts/,
      use: { ...devices["Desktop Chrome"], baseURL: ssrBaseURL },
    },
    {
      name: "csr",
      metadata: { ssr: false },
      dependencies: ["csr-auth"],
      use: { ...devices["Desktop Chrome"], baseURL },
    },
    {
      name: "ssr",
      metadata: { ssr: true },
      dependencies: ["ssr-auth"],
      use: { ...devices["Desktop Chrome"], baseURL: ssrBaseURL },
    },
  ],
  // Started one after the other: the first builds assets + the release binary,
  // the second reuses them. Each boots with its own freshly seeded DB.
  // Run: npm run test:e2e (or npx playwright test)
  webServer: [
    // An SMTP sink the app servers send mail to; specs read it back (e2e/mail-sink.ts).
    {
      command: "node --experimental-strip-types e2e/mail-sink.ts",
      url: "http://127.0.0.1:2526/up",
      reuseExistingServer: !process.env.CI,
      timeout: 30_000,
    },
    {
      command: "bin/e2e-server",
      url: `${baseURL}/up`,
      reuseExistingServer: !process.env.CI,
      // A cold `cargo build --release` is slow.
      timeout: 900_000,
      env: {
        PORT: "5190",
        SSR_ENABLED: "false",
        SSR_SPAWN: "false",
        ...serverHead,
      },
    },
    {
      command: "bin/e2e-server",
      url: `${ssrBaseURL}/up`,
      reuseExistingServer: !process.env.CI,
      timeout: 120_000,
      env: {
        PORT: "5191",
        E2E_SKIP_BUILD: "1",
        SSR_ENABLED: "true",
        SSR_SPAWN: "true",
        // Its own SSR port, so it never talks to another app's `node ssr/ssr.js`
        // on the default 13714; the app passes the port to the spawned server.
        SSR_URL: "http://127.0.0.1:13791/render",
        ...serverHead,
      },
    },
  ],
})
