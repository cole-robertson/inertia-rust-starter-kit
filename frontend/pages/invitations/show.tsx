import { Head, Link, router, usePage } from "@inertiajs/react"

import TextLink from "@/components/text-link"
import { Button } from "@/components/ui/button"
import AuthLayout from "@/layouts/auth-layout"
import { invitations, sessions, users } from "@/routes"

interface InvitationProps {
  account_name: string
  inviter_name: string
  email: string
  signed_in_as: string | null
  expired: boolean
}

export default function Invitation({
  account_name,
  inviter_name,
  email,
  signed_in_as,
  expired,
}: InvitationProps) {
  const { auth } = usePage().props
  const token = usePage().url.split(/[/?#]/)[2]
  const carryToken = { query: { invitation: token } }

  if (expired) {
    return (
      <AuthLayout
        title="This invitation has expired"
        description={`Ask ${inviter_name} to send you a new one.`}
      >
        <Head title="Invitation expired" />
      </AuthLayout>
    )
  }

  return (
    <AuthLayout
      title={`Join ${account_name}`}
      description={`${inviter_name} invited ${email} to ${account_name} on ${import.meta.env.VITE_APP_NAME ?? "Inertia Rust Starter Kit"}.`}
    >
      <Head title={`Join ${account_name}`} />

      {signed_in_as === null ? (
        <div className="flex flex-col gap-4">
          <Button asChild className="w-full">
            <Link href={users.new(carryToken)}>Sign up</Link>
          </Button>
          <div className="text-muted-foreground text-center text-sm">
            Already have an account?{" "}
            <TextLink href={sessions.new(carryToken)}>Log in</TextLink>
          </div>
        </div>
      ) : signed_in_as === email ? (
        <Button asChild className="w-full">
          <Link href={invitations.accept(token)} as="button">
            Accept invitation
          </Link>
        </Button>
      ) : (
        <div className="flex flex-col gap-4 text-center text-sm">
          <p>
            This invitation is for <strong>{email}</strong>. You&apos;re signed
            in as {signed_in_as}.
          </p>
          <TextLink
            href={sessions.destroy(auth.session.id)}
            as="button"
            onClick={() => router.flushAll()}
          >
            Log out
          </TextLink>
        </div>
      )}
    </AuthLayout>
  )
}
