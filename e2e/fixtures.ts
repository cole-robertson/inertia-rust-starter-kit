// Signed-in specs: one@ (the default page) and two@ (`two`), signed in once per server by
// e2e/auth.setup.ts and reused from their saved cookies. Sign-in is rate limited per IP (10 per
// 3 minutes) and every Playwright worker is the same 127.0.0.1, so specs that sign in for
// themselves would start failing each other as the suite grows.
//
//   import { expect, test } from "./fixtures"
//   test("…", async ({ page, two }) => { … })   // page is one@, two is two@ in another browser
import { type Page, expect, test as base } from "@playwright/test"

export { expect }

export const password = "Secret1*3*5*"

/** Where e2e/auth.setup.ts keeps a user's cookies for one server (`csr` or `ssr`). */
export function sessionFile(project: string, email: string) {
  return `tmp/e2e-auth/${project.replace(/-auth$/, "")}-${email.split("@")[0]}.json`
}

/** Wait until React has hydrated the server-rendered page. Before that, a button has no
 * handler (a click does nothing) and a submit button posts its form natively, without the CSRF
 * header (a 422): the SSR project hits both on a busy machine. Call it after `goto` when the
 * next step clicks. */
export async function hydrated(page: Page) {
  await page.waitForFunction(() => {
    const app = document.getElementById("app")
    return (
      app !== null && Object.keys(app).some((key) => key.startsWith("__react"))
    )
  })
}

export async function signIn(page: Page, email: string) {
  await page.goto("/sign_in")
  await hydrated(page)
  await page.getByLabel("Email address").fill(email)
  await page.getByLabel("Password", { exact: true }).fill(password)
  await page.getByRole("button", { name: "Log in" }).click()
  await expect(page).toHaveURL(/\/acme$/)
}

export const test = base.extend<{ two: Page }>({
  // one@ in the default page.
  storageState: async ({ browserName }, provide, testInfo) => {
    void browserName // a fixture needs at least one dependency here
    await provide(sessionFile(testInfo.project.name, "one@example.com"))
  },
  // two@ in a second browser.
  two: async ({ browser, baseURL }, provide, testInfo) => {
    const context = await browser.newContext({
      baseURL,
      storageState: sessionFile(testInfo.project.name, "two@example.com"),
    })
    await provide(await context.newPage())
    await context.close()
  },
})
