import { expect, test } from "@playwright/test"

import { hydrated } from "./fixtures"

// The <head> as the Rails kit has it: one server `<title data-inertia>` with the app name
// in the initial HTML (on SSR, React's own title), and each page's `<Head title>` applied
// with the "%s - Inertia Rust Starter Kit" template, on first load and after navigation.
// playwright.config.ts turns settings.server_head on for both servers, so this also
// checks that pages with no server tags leave the head alone in that mode.

const password = "Secret1*3*5*"

test("the initial HTML has one title and no server meta tags", async ({
  request,
}, testInfo) => {
  const fetchHome = async () => (await request.get("/")).text()
  if (testInfo.project.metadata.ssr === true) {
    // The spawned SSR server may not listen yet on the first requests.
    await expect
      .poll(fetchHome, { timeout: 15_000 })
      .toContain('data-server-rendered="true"')
  }
  const html = await fetchHome()
  const head = html.slice(0, html.indexOf("</head>"))
  expect(head.match(/<title/g)).toHaveLength(1)
  expect(head).not.toContain('<meta name="description"')
  expect(head).toContain(
    '<meta name="application-name" content="Inertia Rust Starter Kit">',
  )
})

test("page titles follow navigation", async ({ page }) => {
  await page.goto("/")
  await hydrated(page)
  await expect(page).toHaveTitle("Welcome - Inertia Rust Starter Kit")

  await page.getByRole("link", { name: "Log in" }).click()
  await expect(page).toHaveURL(/\/sign_in$/)
  await expect(page).toHaveTitle("Log in - Inertia Rust Starter Kit")

  await page.getByLabel("Email address").fill("one@example.com")
  await page.getByLabel("Password", { exact: true }).fill(password)
  await page.getByRole("button", { name: "Log in" }).click()
  await expect(page).toHaveURL(/\/acme$/)
  await expect(page).toHaveTitle("Acme - Inertia Rust Starter Kit")
  await expect(page.locator('meta[name="description"]')).toHaveCount(0)
})
