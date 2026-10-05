{#- One page per action of a `cargo loco generate controller` controller, rendered by
    `cargo loco task scaffold:pages controller:<name>` (src/tasks/scaffold_pages.rs).
    `member`: a `show:<param>` page, whose URL needs the param, so it isn't a breadcrumb itself;
    `has_index`: whether there is an index page to link to. -#}
to: frontend/pages/{{ file_name }}/{{ action }}.tsx
skip_exists: true
---
import { Head } from "@inertiajs/react"

import Heading from "@/components/heading"
{%- if scoped and has_index %}
import { useCurrentAccount } from "@/hooks/use-current-account"
{%- endif %}
import AppLayout from "@/layouts/app-layout"
{%- if has_index %}
import { {{ camel }} as routes } from "@/routes"
{%- endif %}
import type { BreadcrumbItem } from "@/types"
{% if not scoped or not has_index %}
const breadcrumbs: BreadcrumbItem[] = [
{%- if has_index %}
  { title: "{{ label }}", href: routes.index().url },
{%- if action != "index" and not member %}
  { title: "{{ action_label }}", href: routes.{{ action_camel }}().url },
{%- endif %}
{%- endif %}
]
{% endif %}
export default function {{ pascal }}{{ action_pascal }}() {
{%- if scoped and has_index %}
  const { slug: accountSlug } = useCurrentAccount()
  const breadcrumbs: BreadcrumbItem[] = [
    { title: "{{ label }}", href: routes.index(accountSlug).url },
{%- if action != "index" and not member %}
    { title: "{{ action_label }}", href: routes.{{ action_camel }}(accountSlug).url },
{%- endif %}
  ]

{%- endif %}
  return (
    <AppLayout breadcrumbs={breadcrumbs}>
      <Head title="{{ title }}" />
      <div className="space-y-6 p-4">
        <Heading
          title="{{ title }}"
          description="frontend/pages/{{ file_name }}/{{ action }}.tsx"
        />
      </div>
    </AppLayout>
  )
}
