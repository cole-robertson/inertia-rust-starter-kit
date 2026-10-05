import { expect, test } from "@playwright/test"

// The home page's "the server spent X on this page" line shows the app's own measurement:
// the `Server-Timing: app;dur=` header (src/inertia/timing.rs) of the response the browser got.

test("the home page shows the server time from the Server-Timing header", async ({
  page,
}) => {
  const response = await page.goto("/")
  const header = response?.headers()["server-timing"] ?? ""
  const ms = Number(/(?:^|, )app;dur=([\d.]+)(?:,|$)/.exec(header)?.[1])
  expect(ms, `Server-Timing was "${header}"`).toBeGreaterThan(0)

  const shown = ms < 1 ? `${Math.max(1, Math.round(ms * 1000))} µs` : null
  const line = page.getByTestId("server-timing")
  await expect(line).toContainText("The server spent")
  if (shown) {
    await expect(line).toContainText(shown)
  } else {
    await expect(line).toContainText(`${ms.toFixed(ms < 10 ? 2 : 1)} ms`)
  }
  await expect(page.getByRole("list", { name: "Stack" })).toContainText("Loco")
})

// Screenshots for docs/screenshots/, on the client-rendered project only (the pixels match).
test("home page screenshots: desktop and mobile, light and dark", async ({
  page,
}, testInfo) => {
  test.skip(
    testInfo.project.metadata.ssr === true || !process.env.SCREENSHOTS,
    "set SCREENSHOTS=1 to refresh docs/screenshots/",
  )
  const shots: [string, { width: number; height: number }][] = [
    ["desktop", { width: 1280, height: 800 }],
    ["mobile", { width: 390, height: 844 }],
  ]
  for (const scheme of ["light", "dark"] as const) {
    await page.emulateMedia({ colorScheme: scheme })
    for (const [name, viewport] of shots) {
      await page.setViewportSize(viewport)
      await page.goto("/")
      await expect(page.getByTestId("server-timing")).toContainText(
        "The server spent",
      )
      // The card fades in (`starting:opacity-0`, 750 ms); shoot the settled page.
      await page.evaluate(() =>
        Promise.all(document.getAnimations().map((a) => a.finished)),
      )
      await page.screenshot({
        animations: "disabled",
        path: `docs/screenshots/home-${name}-${scheme}.png`,
        fullPage: name === "mobile",
      })
    }
  }
})
