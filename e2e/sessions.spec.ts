import { expect, test } from "@playwright/test"

import { hydrated } from "./fixtures"

test("signs in and lands in the last used account", async ({ page }) => {
  await page.goto("/sign_in")
  await hydrated(page)

  await page.getByLabel("Email address").fill("one@example.com")
  await page.getByLabel("Password", { exact: true }).fill("Secret1*3*5*")
  await page.getByRole("button", { name: "Log in" }).click()

  await expect(page).toHaveURL(/\/acme$/)
  await expect(page.getByText("Acme").first()).toBeVisible()
})
