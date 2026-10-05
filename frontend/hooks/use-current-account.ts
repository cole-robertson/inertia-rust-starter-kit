import { usePage } from "@inertiajs/react"

import type { AccountSummary } from "@/types"

// The account in the URL (/:account_slug/...). The name comes from the switcher list,
// falling back to the slug if the list doesn't have it (yet).
export function useCurrentAccount(): AccountSummary {
  const { url, props } = usePage()
  const slug = url.split(/[/?#]/)[1]
  const account = props.accounts?.find((account) => account.slug === slug)
  return account ?? { slug, name: slug }
}
