to: frontend/pages/{{ snake_plural }}/show.tsx
skip_exists: true
---
import { Head, Link } from "@inertiajs/react"

import Heading from "@/components/heading"
import { Button } from "@/components/ui/button"
{%- if scoped %}
import { useCurrentAccount } from "@/hooks/use-current-account"
{%- endif %}
import AppLayout from "@/layouts/app-layout"
import { {{ camel_plural }} as routes } from "@/routes"
import type { BreadcrumbItem } from "@/types"
import type { {{ pascal_singular }}Props } from "@/types/generated/{{ pascal_singular }}Props"

export default function {{ pascal_singular }}Show({
  {{ snake_singular }},
}: {
  {{ snake_singular }}: {{ pascal_singular }}Props
}) {
{%- if scoped %}
  const { slug: accountSlug } = useCurrentAccount()
  const at = { accountSlug, id: {{ snake_singular }}.id }
{%- else %}
  const at = {{ snake_singular }}.id
{%- endif %}
  const breadcrumbs: BreadcrumbItem[] = [
    { title: "{{ label_plural }}", href: routes.index({% if scoped %}accountSlug{% endif %}).url },
    { title: `{{ label_singular }} #${ {{- snake_singular }}.id}`, href: routes.show(at).url },
  ]

  return (
    <AppLayout breadcrumbs={breadcrumbs}>
      <Head title={breadcrumbs[1].title} />
      <div className="max-w-2xl space-y-6 p-4">
        <Heading title={String({{ snake_singular }}.{{ title_field }})} />

        <dl className="divide-y rounded-lg border text-sm">
{%- for f in fields %}
          <div className="grid grid-cols-3 gap-4 p-4">
            <dt className="text-muted-foreground font-medium">{{ f.label }}</dt>
            <dd className="col-span-2 whitespace-pre-wrap">
{%- if f.input == "checkbox" %}
              {% raw %}{{% endraw %}{{ snake_singular }}.{{ f.name }} ? "Yes" : "No"}
{%- else %}
              {% raw %}{{% endraw %}{{ snake_singular }}.{{ f.name }}{% if f.nullable %} ?? "—"{% endif %}}
{%- endif %}
            </dd>
          </div>
{%- endfor %}
        </dl>

        <div className="flex gap-2">
          <Button variant="outline" asChild>
            <Link href={routes.edit(at)}>Edit</Link>
          </Button>
          <Button variant="destructive" asChild>
            <Link
              href={routes.destroy(at)}
              as="button"
              onBefore={() => confirm("Are you sure?")}
            >
              Delete
            </Link>
          </Button>
          <Button variant="ghost" asChild>
            <Link href={routes.index({% if scoped %}accountSlug{% endif %})}>Back to {{ label_plural | lower }}</Link>
          </Button>
        </div>
      </div>
    </AppLayout>
  )
}
