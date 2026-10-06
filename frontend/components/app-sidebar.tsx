import { usePage } from "@inertiajs/react"
import { BookOpen, Folder, LayoutGrid, Settings, Users } from "lucide-react"

import { AccountSwitcher } from "@/components/account-switcher"
import { NavFooter } from "@/components/nav-footer"
import { NavMain } from "@/components/nav-main"
import { NavUser } from "@/components/nav-user"
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarHeader,
} from "@/components/ui/sidebar"
import { useCurrentAccount } from "@/hooks/use-current-account"
import { accounts, members } from "@/routes"
import type { NavItem } from "@/types"

const footerNavItems: NavItem[] = [
  {
    title: "Repository",
    href: "https://github.com/cole-robertson/inertia-rust-starter-kit",
    icon: Folder,
  },
  {
    title: "Documentation",
    href: "https://inertia-rust.dev/guide/",
    icon: BookOpen,
  },
]

// Pages outside accounts (`cargo loco generate scaffold … --global`).
const globalNavItems: NavItem[] = [
  // scaffold:nav-global
]

export function AppSidebar() {
  const { accounts: accountList = [] } = usePage().props
  const account = useCurrentAccount()
  const inAccount = accountList.some(({ slug }) => slug === account.slug)
  const mainNavItems: NavItem[] = inAccount
    ? [
        {
          title: "Overview",
          href: accounts.show(account.slug).url,
          icon: LayoutGrid,
        },
        {
          title: "Members",
          href: members.index(account.slug).url,
          icon: Users,
        },
        {
          title: "Settings",
          href: accounts.edit(account.slug).url,
          icon: Settings,
        },
        // scaffold:nav
      ]
    : []

  return (
    <Sidebar collapsible="icon" variant="inset">
      <SidebarHeader>
        <AccountSwitcher />
      </SidebarHeader>

      <SidebarContent>
        <NavMain items={[...mainNavItems, ...globalNavItems]} />
      </SidebarContent>

      <SidebarFooter>
        <NavFooter items={footerNavItems} className="mt-auto" />
        <NavUser />
      </SidebarFooter>
    </Sidebar>
  )
}
