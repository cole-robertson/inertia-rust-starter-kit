import { Form, Head } from "@inertiajs/react"

import { Button } from "@/components/ui/button"
import {
  Field,
  FieldError,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Spinner } from "@/components/ui/spinner"
import AuthLayout from "@/layouts/auth-layout"
import { accounts } from "@/routes"

export default function NewAccount() {
  return (
    <AuthLayout
      title="Create an account"
      description="An account holds your team's projects and members"
    >
      <Head title="New account" />
      <Form
        action={accounts.create()}
        disableWhileProcessing
        className="flex flex-col gap-6"
      >
        {({ processing, errors }) => (
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="name">Account name</FieldLabel>
              <Input
                id="name"
                name="name"
                required
                autoFocus
                maxLength={60}
                placeholder="Acme"
              />
              <FieldError
                errors={errors.name?.map((message) => ({ message }))}
              />
            </Field>

            <Button type="submit" className="mt-2 w-full">
              {processing && <Spinner />}
              Create account
            </Button>
          </FieldGroup>
        )}
      </Form>
    </AuthLayout>
  )
}
