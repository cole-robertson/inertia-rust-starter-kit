import { createInertiaApp } from "@inertiajs/react"

import { defaults, layout, serverHead, title } from "@/entrypoints/app"
import { initializeTheme } from "@/hooks/use-appearance"

void createInertiaApp({
  title,
  strictMode: true,
  pages: "../pages",
  layout,
  serverHead,
  defaults,
  progress: {
    color: "#4B5563",
  },
}).catch((error) => {
  // This ensures this entrypoint is only loaded on Inertia pages
  // by checking for the presence of the root element (#app by default).
  // Feel free to remove this `catch` if you don't need it.
  if (document.getElementById("app")) {
    throw error
  } else {
    console.error(
      "Missing root element.\n\n" +
        "If you see this error, it probably means you loaded Inertia.js on non-Inertia pages.\n" +
        "The Inertia entrypoint should only be loaded by the Rust root document (src/inertia/document.rs) on Inertia pages.",
    )
  }
})

// This will set light / dark mode on load...
initializeTheme()
