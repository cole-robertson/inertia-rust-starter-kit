to: frontend/pages/{{ snake_plural }}/edit.tsx
skip_exists: true
---
import { Head } from "@inertiajs/react"

import Heading from "@/components/heading"
{%- if scoped %}
import { useCurrentAccount } from "@/hooks/use-current-account"
{%- endif %}
import AppLayout from "@/layouts/app-layout"
import { {{ camel_plural }} as routes } from "@/routes"
import type { BreadcrumbItem } from "@/types"
import type { {{ pascal_singular }}Props } from "@/types/generated/{{ pascal_singular }}Props"
{% if selects | length > 0 -%}
import type { SelectOption } from "@/types/generated/SelectOption"
{% endif %}
import {{ pascal_singular }}Form from "./form"

export default function {{ pascal_singular }}Edit({
  {{ snake_singular }},
{%- for f in selects %}
  {{ f.options_prop }},
{%- endfor %}
}: {
  {{ snake_singular }}: {{ pascal_singular }}Props
{%- for f in selects %}
  {{ f.options_prop }}: SelectOption[]
{%- endfor %}
}) {
{%- if scoped %}
  const { slug: accountSlug } = useCurrentAccount()
  const at = { accountSlug, id: {{ snake_singular }}.id }
{%- else %}
  const at = {{ snake_singular }}.id
{%- endif %}
  const breadcrumbs: BreadcrumbItem[] = [
    { title: "{{ label_plural }}", href: routes.index({% if scoped %}accountSlug{% endif %}).url },
    { title: `Edit {{ label_singular | lower }} #${ {{- snake_singular }}.id}`, href: routes.edit(at).url },
  ]

  return (
    <AppLayout breadcrumbs={breadcrumbs}>
      <Head title={breadcrumbs[1].title} />
      <div className="max-w-2xl p-4">
        <Heading title={`Edit {{ label_singular | lower }}`} />
        <{{ pascal_singular }}Form
          action={routes.update(at)}
          {{ camel_singular }}={ {{- snake_singular }}}
{%- for f in selects %}
          {{ f.options_camel }}={ {{- f.options_prop }}}
{%- endfor %}
          submitLabel="Update {{ label_singular | lower }}"
        />
      </div>
    </AppLayout>
  )
}
