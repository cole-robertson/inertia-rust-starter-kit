import { usePage } from "@inertiajs/react"

// The invitation token carried through sign-in / sign-up as ?invitation=<token>.
export function useInvitationToken(): string | null {
  const { url } = usePage()
  return new URLSearchParams(url.split("?")[1] ?? "").get("invitation")
}
