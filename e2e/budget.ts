import { type Page, type Request, expect } from "@playwright/test"

// Inertia budgets in the browser: the counterpart of tests/requests/budget.rs.

/**
 * Run `action` (a click, a live message, …) and check that it caused an Inertia partial reload
 * asking for exactly `only`, and that the response carried those props and nothing else of the
 * page's own (shared props aside). Times out if no such reload comes. Returns the response's
 * page object.
 *
 *   await expectPartialReload(page, () => page.getByRole("button", { name: "Refresh" }).click(),
 *     { only: ["members", "invitations"] })
 */
export async function expectPartialReload(
  page: Page,
  action: () => Promise<unknown>,
  {
    only,
    shared = ["auth", "accounts", "errors"],
  }: { only: string[]; shared?: string[] },
): Promise<Record<string, unknown>> {
  // The reload asking for exactly `only` (a page may make other partial reloads meanwhile).
  const wanted = [...only].sort().join(",")
  const asks = (request: Request) =>
    (request.headers()["x-inertia-partial-data"] ?? "")
      .split(",")
      .sort()
      .join(",") === wanted
  const response = page.waitForResponse((res) => asks(res.request()))
  await action()
  const res = await response
  const body = (await res.json()) as { props: Record<string, unknown> }
  const sent = Object.keys(body.props).filter((key) => !shared.includes(key))
  expect(sent.sort(), "the page props the reload sent").toEqual(
    [...only].sort(),
  )
  return body
}
