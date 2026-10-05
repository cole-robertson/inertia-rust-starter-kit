import { expectPartialReload } from "./budget"
import { expect, hydrated, test } from "./fixtures"

// Live updates (src/live/, frontend/lib/live.ts), with two people in two browser contexts on
// Acme's members page: each sees the other there (presence), and a role change made by one
// reaches the other's page as a partial reload of the members lists (AccountChannel), without
// a page load.

test("two people on the members page see each other and a role change arrives live", async ({
  page: owner,
  two: member,
}) => {
  await owner.goto("/acme/members")
  await hydrated(owner)
  await member.goto("/acme/members")
  await hydrated(member)

  // Presence: each sees the other.
  await expect(owner.locator('[data-test="viewing"]')).toHaveText(
    "Also here: Another User",
  )
  await expect(member.locator('[data-test="viewing"]')).toHaveText(
    "Also here: Test User",
  )

  // The member's page shows their role read-only; mark the document to prove no reload.
  const row = member.locator('[data-test="member-two@example.com"]')
  const role = owner.getByRole("combobox", { name: "Role of Another User" })
  await expect(row.getByText("member", { exact: true })).toBeVisible()
  await member.evaluate(() => {
    ;(window as unknown as { marker: number }).marker = 42
  })

  try {
    // The owner makes them an admin; it arrives on the member's page as a partial reload of
    // the two lists and nothing else.
    await expectPartialReload(
      member,
      async () => {
        await role.click()
        await owner.getByRole("option", { name: "admin" }).click()
      },
      { only: ["members", "invitations"] },
    )
    await expect(owner.getByText("Role updated")).toBeVisible()
    await expect(row.getByText("admin", { exact: true })).toBeVisible()
    expect(
      await member.evaluate(
        () => (window as unknown as { marker?: number }).marker,
      ),
    ).toBe(42)
  } finally {
    // Back to member, so the seeds stay as they were for the other specs and a retry.
    await role.click()
    await owner.getByRole("option", { name: "member" }).click()
  }
  await expect(row.getByText("member", { exact: true })).toBeVisible()

  // Leaving the page is announced too.
  await member.goto("/settings/profile")
  await expect(owner.locator('[data-test="viewing"]')).toHaveCount(0)
})
