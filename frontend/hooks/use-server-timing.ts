import { useSyncExternalStore } from "react"

import { isBrowser } from "@/lib/browser"

// How long the server spent on the latest request for this page, in milliseconds: the
// `app;dur=` entry of the `Server-Timing` header (src/inertia/timing.rs), as the browser
// recorded it for the document and for each Inertia visit since. Only entries for the current
// URL count: assets and prefetches of other pages carry the header too. `null` until there is
// one, during SSR, and in browsers without `serverTiming` (Safari before 16.4).

function latestAppDuration(): number | null {
  if (!isBrowser || typeof performance.getEntriesByType !== "function") {
    return null
  }
  const entries = [
    ...performance.getEntriesByType("navigation"),
    ...performance.getEntriesByType("resource"),
  ] as PerformanceResourceTiming[]
  const here = window.location.pathname + window.location.search
  let latest: PerformanceResourceTiming | null = null
  for (const entry of entries) {
    if (!entry.serverTiming?.some((t) => t.name === "app")) continue
    const url = new URL(entry.name, window.location.href)
    if (url.origin !== window.location.origin) continue
    if (url.pathname + url.search !== here) continue
    if (!latest || entry.responseEnd > latest.responseEnd) latest = entry
  }
  return latest?.serverTiming.find((t) => t.name === "app")?.duration ?? null
}

function subscribe(onChange: () => void) {
  if (!isBrowser || typeof PerformanceObserver === "undefined") {
    return () => undefined
  }
  const observer = new PerformanceObserver(onChange)
  try {
    observer.observe({ type: "resource", buffered: false })
  } catch {
    return () => undefined
  }
  return () => observer.disconnect()
}

const serverSnapshot = () => null

export function useServerTiming(): number | null {
  return useSyncExternalStore(subscribe, latestAppDuration, serverSnapshot)
}

/** `0.412` -> "412 µs", `12.3` -> "12.3 ms". */
export function formatDuration(ms: number): string {
  if (ms < 1) return `${Math.max(1, Math.round(ms * 1000))} µs`
  if (ms < 10) return `${ms.toFixed(2)} ms`
  return `${ms.toFixed(1)} ms`
}
