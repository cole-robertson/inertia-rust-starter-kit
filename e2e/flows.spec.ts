import { type Browser, type Page, expect, test } from "@playwright/test"

import { hydrated, sessionFile, test as signedIn } from "./fixtures"
import { lastMailTo, linkIn } from "./mail"

// Full account lifecycle against the real server (CSRF + Origin checks on). Mail goes over
// SMTP to e2e/mail-sink.ts, so the invitation, verification and reset links are followed from
// the mail. These pass with either `settings.sign_up` (`SIGN_UP=open|invitation_only` for
// bin/e2e-server): `openSignUp` finds out which, and with sign-up closed, signs up through an
// invitation from two@ into Globex (not Acme: e2e/invitations.spec.ts counts Acme's members
// while these run in parallel).

const password = "Secret1*3*5*"

/** An address no other spec (or the other project's run) mails. */
function uniqueEmail(prefix: string) {
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}@example.com`
}

/** two@ (the stored session, in a second browser) invites `email` to Globex; returns the
 * invitation's path from the mail. */
async function invite(browser: Browser, baseURL: string, email: string) {
  const context = await browser.newContext({
    baseURL,
    storageState: sessionFile(test.info().project.name, "two@example.com"),
  })
  const owner = await context.newPage()
  await owner.goto("/globex/members")
  await hydrated(owner)
  await owner.getByLabel("Email address").fill(email)
  await owner.getByRole("button", { name: "Send invitation" }).click()
  await expect(owner.getByText(`Invitation sent to ${email}`)).toBeVisible()
  const mail = await lastMailTo(
    owner,
    email,
    "Another User invited you to Globex on Inertia Rust Starter Kit",
  )
  await context.close()
  const link = /https?:\/\/\S+?(\/invitations\/[0-9a-f]+)/.exec(mail)
  expect(link, "the mail has the invitation link").not.toBeNull()
  return link![1]
}

/** Open the sign-up form for `email`: `/sign_up` when sign-up is open; when it is invitation
 * only (`/sign_up` goes to sign in), through an invitation to Globex. */
async function openSignUp(page: Page, browser: Browser, email: string) {
  await page.goto("/sign_up")
  await hydrated(page)
  if (!new URL(page.url()).pathname.startsWith("/sign_in"))
    return { invited: false }
  await page.goto(await invite(browser, new URL(page.url()).origin, email))
  await hydrated(page)
  await page.getByRole("link", { name: "Sign up" }).click()
  await expect(page).toHaveURL(/\/sign_up\?invitation=/)
  return { invited: true }
}

/** Sign up `email`; returns where the new user landed (their personal account, or Globex
 * when invited) and whether the address is verified (an invitation proves it). */
async function signUp(
  page: Page,
  browser: Browser,
  email: string,
  pass: string,
) {
  const { invited } = await openSignUp(page, browser, email)
  await page.getByLabel("Name").fill("E2E Person")
  await page.getByLabel("Email address").fill(email)
  await page.getByLabel("Password", { exact: true }).fill(pass)
  await page.getByLabel("Confirm password").fill(pass)
  await page.getByRole("button", { name: "Create account" }).click()
  const home = invited ? /\/globex$/ : /\/e2e-person-s-account(-\d+)?$/
  await expect(page).toHaveURL(home)
  await expect(
    page.getByText(
      invited
        ? "Welcome to Globex"
        : "Welcome! You have signed up successfully",
    ),
  ).toBeVisible()
  return { home, verified: invited }
}

/** Open the verification link the sign-up mailed to `email`. */
async function verifyFromMail(page: Page, email: string) {
  const mail = await lastMailTo(page, email, "Verify your email")
  await page.goto(linkIn(mail, "/identity/email_verification"))
  await hydrated(page)
  await expect(
    page.getByText("Thank you for verifying your email address"),
  ).toBeVisible()
}

async function signIn(page: Page, email: string, pass = password) {
  await page.goto("/sign_in")
  await hydrated(page)
  await page.getByLabel("Email address").fill(email)
  await page.getByLabel("Password", { exact: true }).fill(pass)
  await page.getByRole("button", { name: "Log in" }).click()
}

test("guest is redirected from protected pages to sign in", async ({
  page,
}) => {
  await page.goto("/dashboard")
  await hydrated(page)
  await expect(page).toHaveURL(/\/sign_in$/)
})

test("wrong password shows an alert and stays on sign in", async ({ page }) => {
  await signIn(page, "two@example.com", "nope-nope-nope")
  await expect(page).toHaveURL(/\/sign_in$/)
  await expect(
    page.getByText("That email or password is incorrect"),
  ).toBeVisible()
})

test("sign up, update profile, see sessions, then delete the account", async ({
  page,
  browser,
}) => {
  const email = uniqueEmail("e2e")

  // a new user lands in their personal account (or, invited, in Globex).
  await signUp(page, browser, email, "a-long-password-1")

  await page.goto("/settings/profile")

  await hydrated(page)
  await page.getByLabel("Name").fill("Renamed Person")
  await page.getByRole("button", { name: "Save" }).click()
  await expect(page.getByText("Your profile has been updated")).toBeVisible()
  await expect(page.getByLabel("Name")).toHaveValue("Renamed Person")

  await page.goto("/settings/sessions")

  await hydrated(page)
  await expect(page.getByText("Current")).toBeVisible()

  await page.goto("/settings/profile")

  await hydrated(page)
  await page.getByRole("button", { name: "Delete account" }).click()
  await page.getByLabel("Password").fill("a-long-password-1")
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Delete account" })
    .click()
  await expect(page.getByText("Your account has been deleted")).toBeVisible()

  await signIn(page, email, "a-long-password-1")
  await expect(
    page.getByText("That email or password is incorrect"),
  ).toBeVisible()
})

test("validation errors come back from the server on sign up", async ({
  page,
  browser,
}) => {
  await openSignUp(page, browser, uniqueEmail("invalid"))
  await page.getByLabel("Name").fill("Dup")
  await page.getByLabel("Email address").fill("one@example.com")
  await page.getByLabel("Password", { exact: true }).fill("short")
  await page.getByLabel("Confirm password").fill("short")
  await page.getByRole("button", { name: "Create account" }).click()

  await expect(page.getByText("has already been taken")).toBeVisible()
  await expect(
    page.getByText("is too short (minimum is 12 characters)"),
  ).toBeVisible()
})

// Signed in from the stored session (e2e/fixtures.ts), not through the form: sign-ins are rate
// limited per IP (10 per 3 minutes) and the specs that test sign-in itself need them.
signedIn("appearance setting toggles dark mode", async ({ page }) => {
  await page.goto("/settings/appearance")
  await hydrated(page)
  await page.getByRole("button", { name: "Dark" }).click()
  await expect(page.locator("html")).toHaveClass(/dark/)
  await page.reload()
  await expect(page.locator("html")).toHaveClass(/dark/)
})

test("sign up, then verify the email address from the mailed link", async ({
  page,
  browser,
}) => {
  let email = uniqueEmail("verify")
  const { verified } = await signUp(page, browser, email, "a-long-password-1")
  if (verified) {
    // Invited: the invitation proved the address, so a new one is what needs verifying.
    email = uniqueEmail("verify-new")
    await page.goto("/settings/email")
    await hydrated(page)
    await page.getByLabel("Email address").fill(email)
    await page.getByLabel("Current password").fill("a-long-password-1")
    await page.getByRole("button", { name: "Save" }).click()
    await expect(page.getByText("Your email has been changed")).toBeVisible()
  }
  await page.goto("/settings/email")
  await hydrated(page)
  await expect(
    page.getByText("Your email address is unverified."),
  ).toBeVisible()

  await verifyFromMail(page, email)
  await page.goto("/settings/email")
  await hydrated(page)
  await expect(page.getByText("Your email address is unverified.")).toBeHidden()
})

test("forgot password: reset it from the mailed link and sign in with the new one", async ({
  page,
  browser,
}) => {
  const email = uniqueEmail("reset")
  const { home, verified } = await signUp(
    page,
    browser,
    email,
    "a-long-password-1",
  )
  // Reset links only go to verified addresses.
  if (!verified) await verifyFromMail(page, email)
  await page.context().clearCookies()

  await page.goto("/sign_in")

  await hydrated(page)
  await page.getByText("Forgot password?").click()
  // The reply is the same for any address, so a fill that lands on the sign-in form before
  // the visit finishes would pass silently: wait for the reset page.
  await expect(page).toHaveURL(/\/identity\/password_reset\/new$/)
  await page.getByLabel("Email address").fill(email)
  await page.getByRole("button", { name: "Email password reset link" }).click()
  await expect(
    page.getByText("we've sent reset instructions to it"),
  ).toBeVisible()

  const mail = await lastMailTo(page, email, "Reset your password")
  await page.goto(linkIn(mail, "/identity/password_reset/edit"))
  await hydrated(page)
  await page.getByLabel("Password", { exact: true }).fill("a-new-password-22")
  await page.getByLabel("Confirm password").fill("a-new-password-22")
  await page.getByRole("button", { name: "Reset password" }).click()
  await expect(
    page.getByText("Your password was reset successfully. Please sign in"),
  ).toBeVisible()

  await signIn(page, email, "a-long-password-1")
  await expect(
    page.getByText("That email or password is incorrect"),
  ).toBeVisible()
  await signIn(page, email, "a-new-password-22")
  await expect(page).toHaveURL(home)
})

test("the sign-in page offers sign-up only when it is open", async ({
  page,
}) => {
  await page.goto("/sign_up")
  await hydrated(page)
  const closed = new URL(page.url()).pathname.startsWith("/sign_in")
  if (closed) {
    await expect(page.getByText("is invitation only")).toBeVisible()
  }
  await page.goto("/sign_in")
  await hydrated(page)
  await expect(page.getByRole("link", { name: "Sign up" })).toHaveCount(
    closed ? 0 : 1,
  )
})
