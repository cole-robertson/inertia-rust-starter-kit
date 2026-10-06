import { Transition } from "@headlessui/react"
import { Form, Head } from "@inertiajs/react"

import Heading from "@/components/heading"
import HeadingSmall from "@/components/heading-small"
import { Button } from "@/components/ui/button"
import { Field, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import AppLayout from "@/layouts/app-layout"
import { accounts } from "@/routes"
import type { BreadcrumbItem } from "@/types"
import type { AccountProps } from "@/types/generated/AccountProps"

interface AccountSettingsProps {
  account: AccountProps
}

export default function AccountSettings({ account }: AccountSettingsProps) {
  const breadcrumbs: BreadcrumbItem[] = [
    { title: account.name, href: accounts.show(account.slug).url },
    { title: "Settings", href: accounts.edit(account.slug).url },
  ]

  return (
    <AppLayout breadcrumbs={breadcrumbs}>
      <Head title="Account settings" />

      <div className="px-4 py-6">
        <Heading
          title="Account settings"
          description="Manage the name of this account"
        />

        <section className="max-w-xl space-y-6">
          <HeadingSmall
            title="Account information"
            description="The URL of the account can't be changed"
          />

          <Form
            action={accounts.update(account.slug)}
            options={{ preserveScroll: true }}
            className="space-y-6"
          >
            {({ errors, processing, recentlySuccessful }) => (
              <>
                <Field>
                  <FieldLabel htmlFor="name">Name</FieldLabel>
                  <Input
                    id="name"
                    name="name"
                    defaultValue={account.name}
                    required
                    maxLength={60}
                  />
                  <FieldError
                    errors={errors.name?.map((message) => ({ message }))}
                  />
                </Field>

                <Field>
                  <FieldLabel htmlFor="slug">URL</FieldLabel>
                  <Input id="slug" value={`/${account.slug}`} disabled />
                </Field>

                <div className="flex items-center gap-4">
                  <Button disabled={processing}>Save</Button>

                  <Transition
                    show={recentlySuccessful}
                    enter="transition ease-in-out"
                    enterFrom="opacity-0"
                    leave="transition ease-in-out"
                    leaveTo="opacity-0"
                  >
                    <p className="text-sm text-neutral-600">Saved</p>
                  </Transition>
                </div>
              </>
            )}
          </Form>
        </section>
      </div>
    </AppLayout>
  )
}
