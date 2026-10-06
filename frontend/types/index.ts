import type { LucideIcon } from "lucide-react"

import type { AccountSummary } from "./generated/AccountSummary"
import type { AuthSession } from "./generated/AuthSession"
import type { AuthUser } from "./generated/AuthUser"

// The props structs' types are generated from Rust into ./generated (src/page_types.rs,
// `cargo loco task types:generate`); import them from there.

// `auth` as the signed-in app's pages see it. The server sends `Auth` (generated), whose user
// and session are null for a guest: pages a guest can open check `auth.user` first.
export interface SignedInAuth {
  user: User
  session: AuthSession
}

export interface BreadcrumbItem {
  title: string
  href: string
}

export interface NavItem {
  title: string
  href: string
  icon?: LucideIcon | null
  isActive?: boolean
}

export interface FlashData {
  alert?: string
  notice?: string
}

export interface SharedProps {
  auth: SignedInAuth
  accounts?: AccountSummary[]
}

// The signed-in user, plus the avatar URL the layout shows when there is one.
export type User = AuthUser & { avatar?: string }
