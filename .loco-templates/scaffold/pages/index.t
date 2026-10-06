to: frontend/pages/{{ snake_plural }}/index.tsx
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
{% if not scoped %}
const breadcrumbs: BreadcrumbItem[] = [
  { title: "{{ label_plural }}", href: routes.index().url },
]
{% endif %}
export default function {{ pascal_singular }}Index({
  {{ snake_plural }},
}: {
  {{ snake_plural }}: {{ pascal_singular }}Props[]
}) {
{%- if scoped %}
  const { slug: accountSlug } = useCurrentAccount()
  const breadcrumbs: BreadcrumbItem[] = [
    { title: "{{ label_plural }}", href: routes.index(accountSlug).url },
  ]

{%- endif %}
  return (
    <AppLayout breadcrumbs={breadcrumbs}>
      <Head title="{{ label_plural }}" />
      <div className="space-y-6 p-4">
        <div className="flex items-start justify-between gap-4">
          <Heading title="{{ label_plural }}" />
          <Button asChild>
            <Link href={routes.new({% if scoped %}accountSlug{% endif %})}>New {{ label_singular | lower }}</Link>
          </Button>
        </div>

        {% raw %}{{% endraw %}{{ snake_plural }}.length === 0 ? (
          <p className="text-muted-foreground text-sm">
            No {{ label_plural | lower }} yet.
          </p>
        ) : (
          <ul className="divide-y rounded-lg border">
            {% raw %}{{% endraw %}{{ snake_plural }}.map(({{ camel_singular }}) => (
              <li key={ {{- camel_singular }}.id} className="flex items-center justify-between gap-4 p-4">
                <Link
                  href={routes.show({% if scoped %}{ accountSlug, id: {{ camel_singular }}.id }{% else %}{{ camel_singular }}.id{% endif %})}
                  className="font-medium underline-offset-4 hover:underline"
                >
                  {String({{ camel_singular }}.{{ title_field }})}
                </Link>
                <Button variant="outline" size="sm" asChild>
                  <Link
                    href={routes.edit({% if scoped %}{ accountSlug, id: {{ camel_singular }}.id }{% else %}{{ camel_singular }}.id{% endif %})}
                  >
                    Edit
                  </Link>
                </Button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </AppLayout>
  )
}
