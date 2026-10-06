to: frontend/pages/{{ snake_plural }}/new.tsx
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
{% if selects | length > 0 -%}
import type { SelectOption } from "@/types/generated/SelectOption"
{% endif %}
import {{ pascal_singular }}Form from "./form"

{% if not scoped -%}
const breadcrumbs: BreadcrumbItem[] = [
  { title: "{{ label_plural }}", href: routes.index().url },
  { title: "New {{ label_singular | lower }}", href: routes.new().url },
]

{% endif -%}
{% if selects | length > 0 -%}
export default function {{ pascal_singular }}New({
{%- for f in selects %}
  {{ f.options_prop }},
{%- endfor %}
}: {
{%- for f in selects %}
  {{ f.options_prop }}: SelectOption[]
{%- endfor %}
}) {
{%- else -%}
export default function {{ pascal_singular }}New() {
{%- endif %}
{%- if scoped %}
  const { slug: accountSlug } = useCurrentAccount()
  const breadcrumbs: BreadcrumbItem[] = [
    { title: "{{ label_plural }}", href: routes.index(accountSlug).url },
    { title: "New {{ label_singular | lower }}", href: routes.new(accountSlug).url },
  ]

{%- endif %}
  return (
    <AppLayout breadcrumbs={breadcrumbs}>
      <Head title="New {{ label_singular | lower }}" />
      <div className="max-w-2xl p-4">
        <Heading title="New {{ label_singular | lower }}" />
        <{{ pascal_singular }}Form
          action={routes.create({% if scoped %}accountSlug{% endif %})}
{%- for f in selects %}
          {{ f.options_camel }}={ {{- f.options_prop }}}
{%- endfor %}
          submitLabel="Create {{ label_singular | lower }}"
        />
      </div>
    </AppLayout>
  )
}
