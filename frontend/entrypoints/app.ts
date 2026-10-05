// Options shared by the browser entry (inertia.tsx) and the SSR entry (ssr.tsx),
// so both render the same app and manage <head> the same way.
import PersistentLayout from "@/layouts/persistent-layout"

const env = import.meta.env as Record<string, string | undefined>
const appName = env.VITE_APP_NAME ?? "Inertia Rust Starter Kit"

/**
 * Inertia's `serverHead` option, matching the server's `settings.server_head`
 * (both read VITE_INERTIA_SERVER_HEAD): unset/"false" leaves server-managed
 * tags to the Rust document head; "true" reads the HTML tags from the `head`
 * prop; any other value names the prop. With it on, SSR renders the tags and
 * every client navigation replaces them. Set it for the build and the server
 * alike: a mismatch only loses the per-navigation updates, since the server
 * still writes any tag the SSR head lacks.
 */
function serverHeadOption(
  value = env.VITE_INERTIA_SERVER_HEAD,
): boolean | string {
  const setting = value?.trim() ?? ""
  if (setting === "" || setting === "false") return false
  if (setting === "true") return true
  return setting
}

export const serverHead = serverHeadOption()

export const title = (title: string) =>
  title ? `${title} - ${appName}` : appName

export const layout = () => PersistentLayout

export const defaults = {
  form: {
    forceIndicesArrayFormatInFormData: false,
    withAllErrors: true,
  },
  visitOptions: () => ({
    queryStringArrayFormat: "brackets" as const,
  }),
}
