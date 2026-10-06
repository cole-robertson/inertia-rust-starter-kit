import { Form, Head, Link, router, usePage } from "@inertiajs/react"

import Heading from "@/components/heading"
import HeadingSmall from "@/components/heading-small"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Field, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Spinner } from "@/components/ui/spinner"
import { useCurrentAccount } from "@/hooks/use-current-account"
import AppLayout from "@/layouts/app-layout"
import { useLiveReload, usePresence } from "@/lib/live"
import { accounts, accountsInvitations, members } from "@/routes"
import type { BreadcrumbItem } from "@/types"
import type { MemberProps } from "@/types/generated/MemberProps"
import type { PendingInvitationProps } from "@/types/generated/PendingInvitationProps"
import type { Role } from "@/types/generated/Role"

interface MembersProps {
  members: MemberProps[]
  invitations: PendingInvitationProps[]
  can_manage: boolean
}

const ROLES: Role[] = ["owner", "admin", "member"]

export default function Members({
  members: memberList,
  invitations,
  can_manage,
}: MembersProps) {
  const { auth } = usePage().props
  const { slug, name } = useCurrentAccount()
  // Live (src/channels/account.rs): someone joins, leaves or changes role -> reload the lists;
  // and who else has this page open.
  useLiveReload(
    "AccountChannel",
    { account: slug },
    {
      only: ["members", "invitations"],
    },
  )
  const here = usePresence("AccountChannel", { account: slug })
  const others = here.filter((person) => person.user_id !== auth.user.id)

  const breadcrumbs: BreadcrumbItem[] = [
    { title: name, href: accounts.show(slug).url },
    { title: "Members", href: members.index(slug).url },
  ]

  return (
    <AppLayout breadcrumbs={breadcrumbs}>
      <Head title="Members" />

      <div className="max-w-3xl space-y-10 px-4 py-6">
        <Heading
          title="Members"
          description="People who can see this account"
        />
        {others.length > 0 && (
          <p className="text-muted-foreground text-sm" data-test="viewing">
            Also here: {others.map((person) => person.name).join(", ")}
          </p>
        )}

        {can_manage && <InviteForm slug={slug} />}

        <section className="space-y-4">
          <HeadingSmall title={`Members (${memberList.length})`} />
          <ul className="divide-y rounded-lg border">
            {memberList.map((member) => {
              const isSelf = member.user.email === auth.user.email
              return (
                <li
                  key={member.id}
                  className="flex items-center justify-between gap-4 p-4"
                  data-test={`member-${member.user.email}`}
                >
                  <div className="min-w-0 space-y-1">
                    <p className="truncate font-medium">
                      {member.user.name}
                      {isSelf && (
                        <Badge variant="secondary" className="ml-2">
                          You
                        </Badge>
                      )}
                    </p>
                    <p className="text-muted-foreground truncate text-sm">
                      {member.user.email} · joined{" "}
                      {new Date(member.joined_at).toLocaleDateString()}
                    </p>
                  </div>
                  <div className="flex items-center gap-2">
                    {can_manage ? (
                      <Select
                        value={member.role}
                        onValueChange={(role) =>
                          router.patch(
                            members.update({ accountSlug: slug, id: member.id })
                              .url,
                            { role },
                            { preserveScroll: true },
                          )
                        }
                      >
                        <SelectTrigger
                          className="w-28 capitalize"
                          aria-label={`Role of ${member.user.name}`}
                        >
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {ROLES.map((role) => (
                            <SelectItem
                              key={role}
                              value={role}
                              className="capitalize"
                            >
                              {role}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    ) : (
                      <Badge variant="outline" className="capitalize">
                        {member.role}
                      </Badge>
                    )}
                    {(can_manage || isSelf) && (
                      <Button variant="outline" size="sm" asChild>
                        <Link
                          href={members.destroy({
                            accountSlug: slug,
                            id: member.id,
                          })}
                          as="button"
                          preserveScroll
                        >
                          {isSelf ? "Leave" : "Remove"}
                        </Link>
                      </Button>
                    )}
                  </div>
                </li>
              )
            })}
          </ul>
        </section>

        {can_manage && (
          <section className="space-y-4">
            <HeadingSmall
              title={`Pending invitations (${invitations.length})`}
            />
            {invitations.length === 0 ? (
              <p className="text-muted-foreground text-sm">
                No pending invitations.
              </p>
            ) : (
              <ul className="divide-y rounded-lg border">
                {invitations.map((invitation) => (
                  <li
                    key={invitation.id}
                    className="flex items-center justify-between gap-4 p-4"
                    data-test={`invitation-${invitation.email}`}
                  >
                    <div className="min-w-0 space-y-1">
                      <p className="truncate font-medium">
                        {invitation.email}
                        <Badge variant="outline" className="ml-2 capitalize">
                          {invitation.role}
                        </Badge>
                      </p>
                      <p className="text-muted-foreground truncate text-sm">
                        Invited by {invitation.inviter_name} · expires{" "}
                        {new Date(invitation.expires_at).toLocaleDateString()}
                      </p>
                    </div>
                    <Button variant="outline" size="sm" asChild>
                      <Link
                        href={accountsInvitations.destroy({
                          accountSlug: slug,
                          id: invitation.id,
                        })}
                        as="button"
                        preserveScroll
                      >
                        Revoke
                      </Link>
                    </Button>
                  </li>
                ))}
              </ul>
            )}
          </section>
        )}
      </div>
    </AppLayout>
  )
}

function InviteForm({ slug }: { slug: string }) {
  return (
    <section className="space-y-4">
      <HeadingSmall
        title="Invite someone"
        description="They get an email with a link to join"
      />
      <Form
        action={accountsInvitations.create(slug)}
        options={{ preserveScroll: true }}
        resetOnSuccess={["email"]}
        className="flex flex-col gap-4 sm:flex-row sm:items-start"
      >
        {({ errors, processing, validate, invalid, validating }) => (
          <>
            <Field className="flex-1">
              <FieldLabel htmlFor="invite-email" className="sr-only">
                Email address
              </FieldLabel>
              <Input
                id="invite-email"
                type="email"
                name="email"
                required
                placeholder="email@example.com"
                aria-invalid={invalid("email")}
                onBlur={() => validate("email")}
              />
              <FieldError
                errors={errors.email?.map((message) => ({ message }))}
              />
            </Field>
            <Field className="sm:w-32">
              <FieldLabel htmlFor="invite-role" className="sr-only">
                Role
              </FieldLabel>
              <Select name="role" defaultValue="member">
                <SelectTrigger id="invite-role" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="member">Member</SelectItem>
                  <SelectItem value="admin">Admin</SelectItem>
                </SelectContent>
              </Select>
              <FieldError
                errors={errors.role?.map((message) => ({ message }))}
              />
            </Field>
            <Button type="submit" disabled={processing}>
              {(processing || validating) && <Spinner />}
              Send invitation
            </Button>
          </>
        )}
      </Form>
    </section>
  )
}
