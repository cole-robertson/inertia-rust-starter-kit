import { http, router } from "@inertiajs/react"
import { useEffect, useRef, useState } from "react"

// Live updates: the client half of src/live/ (the kit's Action Cable).
//
//   subscribe("AccountChannel", { account: "acme" }, { received(message) {} })
//   useChannel("AccountChannel", { account }, (message) => …)
//   useLiveReload("AccountChannel", { account }, { only: ["members"] })
//   usePresence("AccountChannel", { account })   // who else has it open
//
// One Server-Sent Events stream per tab carries every subscription (GET /live?s=[…]); it
// reopens with the new set when a subscription comes or goes, and on its own after a drop
// (`retry: 1000`). The server confirms or rejects each subscription, the way Action Cable does,
// and a rejected one never receives anything.

/** This browser tab. Every Inertia request sends it as X-Tab-Id; a change it causes comes back
 * stamped with it, and is skipped here (the tab already shows it). */
export const tabId =
  typeof crypto === "undefined" || !("randomUUID" in crypto)
    ? ""
    : crypto.randomUUID()

if (tabId) {
  http.onRequest((config) => ({
    ...config,
    headers: { ...config.headers, "X-Tab-Id": tabId },
  }))
}

export type Params = Record<string, string | number | boolean | null>

export interface Present {
  user_id: number
  name: string
  state: unknown
}

export interface Callbacks<M = unknown> {
  received?: (message: M) => void
  connected?: () => void
  rejected?: () => void
  presence?: (present: Present[]) => void
  /** Also deliver messages this tab caused (off by default). */
  echo?: boolean
}

export interface Subscription {
  identifier: string
  /** Action Cable's `perform(action, data)`: POST /live/perform. */
  perform: (action: string, data?: unknown) => Promise<Response>
  unsubscribe: () => void
}

interface Frame {
  identifier: string
  type?: "confirm_subscription" | "reject_subscription" | "presence"
  message?: unknown
  present?: Present[]
  tab_id?: string | null
}

/** `{"channel": name, …params}` as JSON with sorted keys: what the server's
 * `live::identifier` builds. */
export function identifier(channel: string, params: Params = {}): string {
  const fields: Record<string, unknown> = { ...params, channel }
  const sorted = Object.keys(fields)
    .sort()
    .reduce<Record<string, unknown>>((out, key) => {
      out[key] = fields[key]
      return out
    }, {})
  return JSON.stringify(sorted)
}

const subscriptions = new Map<string, Set<Callbacks<never>>>()
let source: EventSource | null = null
let reopen: number | undefined

function xsrfToken(): string {
  const match = /(?:^|;\s*)XSRF-TOKEN=([^;]*)/.exec(document.cookie)
  return match ? decodeURIComponent(match[1]) : ""
}

/** A JSON request the way Inertia's would be: same-origin cookies, the CSRF token, the tab. */
export function send(
  url: string,
  method: "POST" | "DELETE",
  body: unknown,
  keepalive = false,
): Promise<Response> {
  return fetch(url, {
    method,
    credentials: "same-origin",
    keepalive,
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
      "X-XSRF-TOKEN": xsrfToken(),
      "X-Tab-Id": tabId,
    },
    body: JSON.stringify(body),
  })
}

function dispatch(frame: Frame) {
  const listeners = subscriptions.get(frame.identifier)
  if (!listeners) return
  for (const listener of listeners) {
    const callbacks = listener as Callbacks
    switch (frame.type) {
      case "confirm_subscription":
        callbacks.connected?.()
        break
      case "reject_subscription":
        callbacks.rejected?.()
        break
      case "presence":
        callbacks.presence?.(frame.present ?? [])
        break
      default:
        if (callbacks.echo || !tabId || frame.tab_id !== tabId) {
          callbacks.received?.(frame.message)
        }
    }
  }
}

// (Re)open the stream with the current subscriptions, once per tick however many changed. A
// message broadcast while it reopens is missed, as with Action Cable; `connected` runs on each
// (re)connection so a page can reload what it shows, and a presence channel's stream starts
// with who is there now.
function schedule() {
  if (typeof EventSource === "undefined") return
  window.clearTimeout(reopen)
  reopen = window.setTimeout(() => {
    source?.close()
    source = null
    const identifiers = [...subscriptions.keys()]
    if (identifiers.length === 0) return
    const query = new URLSearchParams({ s: JSON.stringify(identifiers) })
    source = new EventSource(`/live?${query.toString()}`)
    source.onmessage = (event: MessageEvent<string>) =>
      dispatch(JSON.parse(event.data) as Frame)
  }, 0)
}

/** Subscribe to `channel` with `params` (Action Cable's
 * `consumer.subscriptions.create({channel, …params}, callbacks)`). */
export function subscribe<M = unknown>(
  channel: string,
  params: Params,
  callbacks: Callbacks<M>,
): Subscription {
  const id = identifier(channel, params)
  let listeners = subscriptions.get(id)
  const isNew = !listeners
  if (!listeners) {
    listeners = new Set()
    subscriptions.set(id, listeners)
  }
  listeners.add(callbacks)
  if (isNew) schedule()

  return {
    identifier: id,
    perform: (action, data) =>
      send("/live/perform", "POST", { identifier: id, action, data }),
    unsubscribe: () => {
      const set = subscriptions.get(id)
      if (!set) return
      set.delete(callbacks)
      if (set.size === 0) {
        subscriptions.delete(id)
        schedule()
      }
    },
  }
}

// `params` as a stable dependency for effects (a new object each render is the same channel).
function useIdentifier(channel: string, params: Params) {
  return identifier(channel, params)
}

/** Call `onMessage` for each message on `channel` (not this tab's own echoes). Returns the
 * subscription's `perform`. */
export function useChannel<M = unknown>(
  channel: string,
  params: Params,
  onMessage: (message: M) => void,
): Subscription["perform"] {
  const id = useIdentifier(channel, params)
  const callback = useRef(onMessage)
  useEffect(() => {
    callback.current = onMessage
  })
  const subscription = useRef<Subscription | null>(null)
  useEffect(() => {
    const sub = subscribe<M>(channel, JSON.parse(id) as Params, {
      received: (message) => callback.current(message),
    })
    subscription.current = sub
    return () => sub.unsubscribe()
  }, [channel, id])
  return (action, data) =>
    subscription.current?.perform(action, data) ??
    Promise.reject(new Error("not subscribed"))
}

/** Notify, then reload: on each message on `channel`, partially reload the page's `only`
 * props (the Inertia-shaped way to go live: the server keeps rendering, no second format).
 * `when` filters the messages that count. */
export function useLiveReload<M = unknown>(
  channel: string,
  params: Params,
  { only, when }: { only: string[]; when?: (message: M) => boolean },
) {
  const props = only.join(",")
  const filter = useRef(when)
  useEffect(() => {
    filter.current = when
  })
  useChannel<M>(channel, params, (message) => {
    if (filter.current && !filter.current(message)) return
    router.reload({ only: props.split(",") })
  })
}

const HEARTBEAT_MS = 15_000

/** Who else has `channel` open (a channel with `tracks_presence`). Sends a heartbeat every 15 s
 * with `state` (e.g. `{ editing: 7 }`, at once when it changes) and a goodbye when the page
 * closes; the server expires a tab after 30 s without one. */
export function usePresence(
  channel: string,
  params: Params,
  state: unknown = null,
): Present[] {
  const id = useIdentifier(channel, params)
  const [present, setPresent] = useState<Present[]>([])
  const stateJson = JSON.stringify(state)

  useEffect(() => {
    const sub = subscribe(channel, JSON.parse(id) as Params, {
      presence: setPresent,
    })
    return () => sub.unsubscribe()
  }, [channel, id])

  // Heartbeats with the latest state, and the goodbye. A new state goes out at once (below),
  // not as a goodbye and a hello.
  const stateRef = useRef(stateJson)
  const beat = useRef<() => void>(() => undefined)
  useEffect(() => {
    if (!tabId) return
    const body = (extra: object = {}) => ({
      identifier: id,
      tab_id: tabId,
      ...extra,
    })
    beat.current = () =>
      void send(
        "/live/presence",
        "POST",
        body({ state: JSON.parse(stateRef.current) as unknown }),
      )
        .then(async (res) =>
          res.ok
            ? setPresent(((await res.json()) as { present: Present[] }).present)
            : undefined,
        )
        .catch(() => undefined)
    beat.current()
    const timer = window.setInterval(() => beat.current(), HEARTBEAT_MS)
    const bye = () =>
      void send("/live/presence", "DELETE", body(), true).catch(() => undefined)
    window.addEventListener("pagehide", bye)
    return () => {
      window.clearInterval(timer)
      window.removeEventListener("pagehide", bye)
      bye()
    }
  }, [id])

  useEffect(() => {
    if (stateRef.current === stateJson) return
    stateRef.current = stateJson
    beat.current()
  }, [stateJson])

  return present
}
