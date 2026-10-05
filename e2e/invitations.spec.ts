import { expect, test } from "@playwright/test"

import { hydrated } from "./fixtures"
import { lastMailTo } from "./mail"

// Organizations end to end: the owner invites, the invitee opens the mailed link, signs up,
// lands in Acme, and the account switcher shows their accounts. The app sends the mail over
// SMTP to e2e/mail-sink.ts, which this reads it back from. The pages mark elements with
// `data-test` (Capybara's convention, from the Rails twin they were written for), hence the CSS
// locators.

const password = "Secret1*3*5*"

test("owner invites, the invitee signs up from the mail link, lands in Acme and switches accounts", async ({
  page,
}) => {
  // Unique per project run. CSR and SSR share the mail sink and may start this in the same
  // millisecond, so the project name is part of it (else one reads the other's link).
  const newbie = `newbie-${test.info().project.name}-${Date.now()}@example.com`

  await page.goto("/sign_in")

  await hydrated(page)
  await page.getByLabel("Email address").fill("one@example.com")
  await page.getByLabel("Password", { exact: true }).fill(password)
  await page.getByRole("button", { name: "Log in" }).click()
  await expect(page).toHaveURL(/\/acme$/)

  await page.getByRole("link", { name: "Members" }).click()
  await expect(page).toHaveURL(/\/acme\/members$/)
  const email = page.getByLabel("Email address")
  await email.fill("two@example.com")
  await email.blur() // precognition validates on blur
  await expect(page.getByText("is already a member")).toBeVisible()

  await email.fill(newbie)
  await page.getByRole("button", { name: "Send invitation" }).click()
  await expect(page.getByText(`Invitation sent to ${newbie}`)).toBeVisible()
  await expect(page.locator(`[data-test="invitation-${newbie}"]`)).toBeVisible()

  const mail = await lastMailTo(
    page,
    newbie,
    "Test User invited you to Acme on Inertia Rust Starter Kit",
  )
  const link = /https?:\/\/\S+?(\/invitations\/[0-9a-f]+)/.exec(mail)
  expect(link, "the mail has the accept link").not.toBeNull()

  // A flash on a full page load (sign-in -> / -> /acme, all server redirects) toasts once.
  await page.goto("/sign_in")
  await hydrated(page)
  await expect(page).toHaveURL(/\/acme$/)
  await expect(page.getByText("You are already signed in")).toHaveCount(1)

  await page.getByRole("button", { name: "Test User" }).click()
  await page.getByRole("menuitem", { name: "Log out" }).click()
  await expect(page).toHaveURL(/\/sign_in$/)

  await page.goto(link![1])

  await hydrated(page)
  await expect(page.getByRole("heading", { name: "Join Acme" })).toBeVisible()
  await page.getByRole("link", { name: "Sign up" }).click()
  await page.getByLabel("Name").fill("New Bie")
  await page.getByLabel("Email address").fill(newbie)
  await page.getByLabel("Password", { exact: true }).fill(password)
  await page.getByLabel("Confirm password").fill(password)
  await page.getByRole("button", { name: "Create account" }).click()

  await expect(page).toHaveURL(/\/acme$/)
  await expect(page.locator('[data-test="members-count"]')).toHaveText("3")
  // The deferred members_count reload does not toast the flash a second time.
  await expect(page.getByText("Welcome to Acme")).toHaveCount(1)

  await page.locator('[data-test="account-switcher"]').click()
  await expect(page.getByRole("menuitem", { name: "Acme" })).toBeVisible()
  await page.getByRole("menuitem", { name: "New account" }).click()
  await page.getByLabel("Account name").fill("Newbie Labs")
  await page.getByRole("button", { name: "Create account" }).click()
  await expect(page.getByText("Account created")).toBeVisible()
  await expect(page).toHaveURL(/\/newbie-labs(-\d+)?$/)

  await page.locator('[data-test="account-switcher"]').click()
  await expect(page.getByRole("menuitem", { name: "Acme" })).toBeVisible()
  await expect(
    page.getByRole("menuitem", { name: "Newbie Labs" }).first(),
  ).toBeVisible()
})
