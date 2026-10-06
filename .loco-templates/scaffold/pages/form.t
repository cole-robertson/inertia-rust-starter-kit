to: frontend/pages/{{ snake_plural }}/form.tsx
skip_exists: true
---
import type { UrlMethodPair } from "@inertiajs/core"
import { Form } from "@inertiajs/react"

import { Button } from "@/components/ui/button"
{%- for f in fields %}{% if f.input == "checkbox" %}
import { Checkbox } from "@/components/ui/checkbox"
{%- break %}{% endif %}{% endfor %}
import {
  Field,
  FieldError,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
{%- if selects | length > 0 %}
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
{%- endif %}
import { Spinner } from "@/components/ui/spinner"
import type { {{ pascal_singular }}Props } from "@/types/generated/{{ pascal_singular }}Props"
{%- if selects | length > 0 %}
import type { SelectOption } from "@/types/generated/SelectOption"
{%- endif %}
{%- set_global nullable_selects = [] %}
{%- for f in selects %}{% if f.nullable %}{% set_global nullable_selects = nullable_selects | concat(with=f.name) %}{% endif %}{% endfor %}
{%- if nullable_selects | length > 0 %}

// A select item can't have the value "", so "None" submits NONE and `transform` blanks it.
const NONE = "none"
{%- endif %}

// The create and edit form. Inertia's <Form> submits every named field (as strings) to
// `action`; the server answers with a redirect, or back here with `errors` (string[] per field).
export default function {{ pascal_singular }}Form({
  action,
  {{ camel_singular }},
{%- for f in selects %}
  {{ f.options_camel }},
{%- endfor %}
  submitLabel,
}: {
  action: UrlMethodPair
  {{ camel_singular }}?: {{ pascal_singular }}Props
{%- for f in selects %}
  {{ f.options_camel }}: SelectOption[]
{%- endfor %}
  submitLabel: string
}) {
  return (
    <Form
      action={action}
{%- if nullable_selects | length > 0 %}
      transform={(data) => ({
        ...data,
{%- for name in nullable_selects %}
        {{ name }}: data.{{ name }} === NONE ? "" : String(data.{{ name }}),
{%- endfor %}
      })}
{%- endif %}
      disableWhileProcessing
      className="flex flex-col gap-6"
    >
      {({ processing, errors }) => (
        <>
          <FieldGroup>
{%- for f in fields %}
{%- if f.input == "checkbox" %}
            <Field orientation="horizontal">
              <Checkbox
                id="{{ f.name }}"
                name="{{ f.name }}"
                value="1"
                defaultChecked={Boolean({{ camel_singular }}?.{{ f.name }})}
              />
              <FieldLabel htmlFor="{{ f.name }}">{{ f.label }}</FieldLabel>
            </Field>
{%- else %}
            <Field>
              <FieldLabel htmlFor="{{ f.name }}">{{ f.label }}</FieldLabel>
{%- if f.input == "select" %}
              <Select
                name="{{ f.name }}"
{%- if f.nullable %}
                defaultValue={String({{ camel_singular }}?.{{ f.name }} ?? NONE)}
{%- else %}
                defaultValue={
                  {{ camel_singular }} ? String({{ camel_singular }}.{{ f.name }}) : undefined
                }
                required
{%- endif %}
              >
                <SelectTrigger
                  id="{{ f.name }}"
                  className="w-full"
                  aria-invalid={Boolean(errors.{{ f.error_key }})}
                >
                  <SelectValue placeholder="Select a {{ f.label | lower }}" />
                </SelectTrigger>
                <SelectContent>
{%- if f.nullable %}
                  <SelectItem value={NONE}>None</SelectItem>
{%- endif %}
                  {% raw %}{{% endraw %}{{ f.options_camel }}.map((option) => (
                    <SelectItem key={option.id} value={String(option.id)}>
                      {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
{%- elif f.input == "textarea" %}
              <textarea
                id="{{ f.name }}"
                name="{{ f.name }}"
                rows={5}
                defaultValue={ {{- camel_singular }}?.{{ f.name }} ?? ""}
                className="border-input placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-ring/50 dark:bg-input/30 w-full rounded-md border bg-transparent px-3 py-2 text-base shadow-xs outline-none focus-visible:ring-[3px] md:text-sm"
              />
{%- else %}
              <Input
                id="{{ f.name }}"
                name="{{ f.name }}"
                type="{{ f.input }}"
{%- if f.input == "number" %}
                step="{{ f.step }}"
{%- endif %}
                defaultValue={ {{- camel_singular }}?.{{ f.name }} ?? ""}
{%- if not f.nullable %}
                required
{%- endif %}
              />
{%- endif %}
              <FieldError
                errors={errors.{{ f.error_key }}?.map((message) => ({ message }))}
              />
            </Field>
{%- endif %}
{%- endfor %}
          </FieldGroup>

          <div>
            <Button type="submit" disabled={processing}>
              {processing && <Spinner />}
              {submitLabel}
            </Button>
          </div>
        </>
      )}
    </Form>
  )
}
