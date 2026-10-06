import { Deferred, Head } from "@inertiajs/react"

import Heading from "@/components/heading"
import { Badge } from "@/components/ui/badge"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import AppLayout from "@/layouts/app-layout"
import { accounts } from "@/routes"
import type { BreadcrumbItem } from "@/types"
import type { AccountProps } from "@/types/generated/AccountProps"
import type { Role } from "@/types/generated/Role"

interface ShowAccountProps {
  account: AccountProps
  membership: { role: Role }
  members_count?: number
}

export default function ShowAccount({
  account,
  membership,
  members_count,
}: ShowAccountProps) {
  const breadcrumbs: BreadcrumbItem[] = [
    { title: account.name, href: accounts.show(account.slug).url },
  ]

  return (
    <AppLayout breadcrumbs={breadcrumbs}>
      <Head title={account.name} />

      <div className="flex h-full flex-1 flex-col gap-4 p-4">
        <div className="flex items-start justify-between">
          <Heading title={account.name} description={`/${account.slug}`} />
          <Badge variant="secondary" className="capitalize">
            {membership.role}
          </Badge>
        </div>

        <div className="grid auto-rows-min gap-4 md:grid-cols-3">
          <Card>
            <CardHeader>
              <CardDescription>Members</CardDescription>
              <CardTitle className="text-3xl" data-test="members-count">
                <Deferred
                  data="members_count"
                  fallback={<Skeleton className="h-9 w-12" />}
                >
                  {members_count}
                </Deferred>
              </CardTitle>
            </CardHeader>
          </Card>
        </div>

        <Card>
          <CardHeader>
            <CardTitle>This account is empty</CardTitle>
            <CardDescription>
              Everything you add here belongs to {account.name}.
            </CardDescription>
          </CardHeader>
          <CardContent className="text-muted-foreground text-sm">
            Generate your first resource with{" "}
            <code>cargo loco generate scaffold</code>; its pages live under{" "}
            <code>/{account.slug}</code> and link from the sidebar.
          </CardContent>
        </Card>
      </div>
    </AppLayout>
  )
}
