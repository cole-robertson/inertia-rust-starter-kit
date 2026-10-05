import { expect, test } from "@playwright/test"

// Proves which rendering path the server took: the "ssr" project boots the app with
// SSR_ENABLED=true SSR_SPAWN=true (the real ssr/ssr.js bundle), "csr" with SSR off.

test("initial HTML is server-rendered only when SSR is on", async ({
  request,
}, testInfo) => {
  const fetchSignIn = async () => {
    const response = await request.get("/sign_in")
    expect(response.status()).toBe(200)
    return response.text()
  }

  if (testInfo.project.metadata.ssr === true) {
    // The app spawns `node ssr/ssr.js` at boot, so /up can answer before the SSR
    // server listens (those first pages fall back to client rendering).
    await expect
      .poll(fetchSignIn, { timeout: 15_000 })
      .toContain('data-server-rendered="true"')
    // The form markup itself came from the server, not an empty mount point.
    expect(await fetchSignIn()).toMatch(/<input[^>]*type="email"/)
  } else {
    const html = await fetchSignIn()
    expect(html).toContain('<div id="app"></div>')
    expect(html).not.toContain("data-server-rendered")
    expect(html).not.toMatch(/<input[^>]*type="email"/)
  }
})

test("the page hydrates without console errors", async ({ page }) => {
  const errors: string[] = []
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text())
  })
  page.on("pageerror", (error) => errors.push(error.message))

  await page.goto("/sign_in")
  await page.getByLabel("Email address").fill("one@example.com")
  await expect(page.getByLabel("Email address")).toHaveValue("one@example.com")
  expect(errors).toEqual([])
})
