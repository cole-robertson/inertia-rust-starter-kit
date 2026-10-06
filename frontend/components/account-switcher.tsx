import { Link, usePage } from "@inertiajs/react"
import { Check, ChevronsUpDown, Plus } from "lucide-react"

import AppLogoIcon from "@/components/app-logo-icon"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  useSidebar,
} from "@/components/ui/sidebar"
import { useCurrentAccount } from "@/hooks/use-current-account"
import { useIsMobile } from "@/hooks/use-mobile"
import { accounts as accountsRoutes } from "@/routes"

export function AccountSwitcher() {
  const { accounts = [] } = usePage().props
  const slug = useCurrentAccount().slug
  const current = accounts.find((account) => account.slug === slug)
  const { state } = useSidebar()
  const isMobile = useIsMobile()

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <SidebarMenuButton
              size="lg"
              className="data-[state=open]:bg-sidebar-accent"
              data-test="account-switcher"
            >
              <div className="bg-background flex aspect-square size-8 items-center justify-center rounded-md border">
                <AppLogoIcon className="size-5" />
              </div>
              <div className="ml-1 grid flex-1 text-left text-sm">
                <span className="truncate leading-tight font-semibold">
                  {current?.name ??
                    import.meta.env.VITE_APP_NAME ??
                    "Inertia Rust Starter Kit"}
                </span>
              </div>
              <ChevronsUpDown className="ml-auto size-4" />
            </SidebarMenuButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            className="w-(--radix-dropdown-menu-trigger-width) min-w-56 rounded-lg"
            align="start"
            side={
              isMobile ? "bottom" : state === "collapsed" ? "right" : "bottom"
            }
          >
            <DropdownMenuLabel className="text-muted-foreground text-xs">
              Accounts
            </DropdownMenuLabel>
            {accounts.map((account) => (
              <DropdownMenuItem key={account.slug} asChild>
                <Link href={accountsRoutes.show(account.slug)} prefetch>
                  <span className="flex-1 truncate">{account.name}</span>
                  {account.slug === current?.slug && <Check />}
                </Link>
              </DropdownMenuItem>
            ))}
            <DropdownMenuSeparator />
            <DropdownMenuItem asChild>
              <Link href={accountsRoutes.new()}>
                <Plus />
                New account
              </Link>
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarMenuItem>
    </SidebarMenu>
  )
}
