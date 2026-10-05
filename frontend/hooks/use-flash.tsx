import { router } from "@inertiajs/react"
import { useEffect } from "react"
import { toast } from "sonner"

import type { FlashData } from "@/types"

function showFlash(flash: FlashData) {
  if (flash.alert) toast.error(flash.alert)
  if (flash.notice) toast(flash.notice)
}

// Inertia's `flash` event fires once per visit that carries a flash, and on the first page
// load. Unlike watching `usePage().flash`, it does not fire again when a deferred-props
// reload hands the page a copy of the same flash, which showed every toast twice on pages
// with deferred props.
export function useFlash() {
  useEffect(
    () => router.on("flash", (event) => showFlash(event.detail.flash)),
    [],
  )
}
