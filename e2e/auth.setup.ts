import { test as setup } from "@playwright/test"

import { sessionFile, signIn } from "./fixtures"

// Sign one@ and two@ in once per server and keep their cookies for the signed-in specs
// (e2e/fixtures.ts). Runs as the csr-auth / ssr-auth project before csr / ssr.

for (const email of ["one@example.com", "two@example.com"]) {
  setup(`sign in ${email}`, async ({ page }, setupInfo) => {
    await signIn(page, email)
    await page
      .context()
      .storageState({ path: sessionFile(setupInfo.project.name, email) })
  })
}
