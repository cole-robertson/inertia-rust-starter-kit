import { Head, Link, usePage } from "@inertiajs/react"
import { Gauge } from "lucide-react"

import AppLogoIcon from "@/components/app-logo-icon"
import { Badge } from "@/components/ui/badge"
import { formatDuration, useServerTiming } from "@/hooks/use-server-timing"
import { home, sessions } from "@/routes"

const STACK = ["Rust", "Loco", "Inertia.js", "React", "shadcn/ui"]

export default function Welcome() {
  const page = usePage()
  const { auth } = page.props
  const serverMs = useServerTiming()

  return (
    <>
      <Head title="Welcome">
        <link rel="preconnect" href="https://fonts.bunny.net" />
        <link
          href="https://fonts.bunny.net/css?family=instrument-sans:400,500,600"
          rel="stylesheet"
        />
      </Head>

      <div className="flex min-h-screen flex-col items-center bg-[#FDFDFC] p-6 text-[#1b1b18] lg:justify-center lg:p-8 dark:bg-[#0a0a0a]">
        <header className="mb-6 w-full max-w-[335px] text-sm not-has-[nav]:hidden lg:max-w-4xl">
          <nav className="flex items-center justify-end gap-4">
            <Link
              href={home.index()}
              className="mr-auto flex items-center gap-2 font-medium text-[#1b1b18] dark:text-[#EDEDEC]"
            >
              <AppLogoIcon className="size-6" />
              {import.meta.env.VITE_APP_NAME ?? "Inertia Rust Starter Kit"}
            </Link>
            {auth.user ? (
              <Link
                href={home.index()}
                className="inline-block rounded-sm border border-[#19140035] px-5 py-1.5 text-sm leading-normal text-[#1b1b18] hover:border-[#1915014a] dark:border-[#3E3E3A] dark:text-[#EDEDEC] dark:hover:border-[#62605b]"
              >
                Dashboard
              </Link>
            ) : (
              <>
                <Link
                  href={sessions.new()}
                  className="inline-block rounded-sm border border-transparent px-5 py-1.5 text-sm leading-normal text-[#1b1b18] hover:border-[#19140035] dark:text-[#EDEDEC] dark:hover:border-[#3E3E3A]"
                >
                  Log in
                </Link>
              </>
            )}
          </nav>
        </header>

        <div className="flex w-full items-center justify-center opacity-100 transition-opacity duration-750 lg:grow starting:opacity-0">
          <main className="flex w-full max-w-[335px] flex-col-reverse lg:max-w-4xl lg:flex-row">
            <div className="flex-1 rounded-br-lg rounded-bl-lg bg-white p-6 pb-12 text-[13px] leading-[20px] shadow-[inset_0px_0px_0px_1px_rgba(26,26,0,0.16)] lg:rounded-tl-lg lg:rounded-br-none lg:p-20 dark:bg-[#161615] dark:text-[#EDEDEC] dark:shadow-[inset_0px_0px_0px_1px_#fffaed2d]">
              <h1 className="mb-1 font-medium">
                {import.meta.env.VITE_APP_NAME ?? "Inertia Rust Starter Kit"}
              </h1>
              <p className="mb-3 text-[#706f6c] dark:text-[#A1A09A]">
                A full-stack starter: a Rust server on Loco renders React pages
                through Inertia.js. Sign-up, sessions, settings and mail work
                out of the box.
              </p>

              <ul aria-label="Stack" className="mb-3 flex flex-wrap gap-1.5">
                {STACK.map((item) => (
                  <li key={item}>
                    <Badge variant="outline" className="font-normal">
                      {item}
                    </Badge>
                  </li>
                ))}
              </ul>

              <p
                className="mb-4 flex min-h-5 items-center gap-1.5 text-[#706f6c] dark:text-[#A1A09A]"
                data-test="server-timing"
              >
                {serverMs !== null && (
                  <>
                    <Gauge className="size-3.5 shrink-0" aria-hidden />
                    <span>
                      The server spent{" "}
                      <span className="font-medium text-[#1b1b18] tabular-nums dark:text-[#EDEDEC]">
                        {formatDuration(serverMs)}
                      </span>{" "}
                      on this page
                    </span>
                  </>
                )}
              </p>

              <p className="mb-2 text-[#706f6c] dark:text-[#A1A09A]">
                Here are some resources to begin:
              </p>

              <ul className="mb-4 flex flex-col lg:mb-6">
                {[
                  {
                    text: "Inertia.js Docs",
                    href: "https://inertiajs.com",
                  },
                  {
                    text: "shadcn/ui Components",
                    href: "https://ui.shadcn.com",
                  },
                  {
                    text: "React Docs",
                    href: "https://react.dev",
                  },
                  {
                    text: "Loco Guides",
                    href: "https://loco.rs/docs/",
                  },
                ].map((resource, index) => (
                  <ResourceItem key={index} {...resource} />
                ))}
              </ul>

              <ul className="flex gap-3 text-sm leading-normal">
                <li>
                  <a
                    href="https://loco.rs"
                    target="_blank"
                    className="inline-block rounded-sm border border-black bg-[#1b1b18] px-5 py-1.5 text-sm leading-normal text-white hover:border-black hover:bg-black dark:border-[#eeeeec] dark:bg-[#eeeeec] dark:text-[#1C1C1A] dark:hover:border-white dark:hover:bg-white"
                    rel="noreferrer"
                  >
                    Learn More
                  </a>
                </li>
              </ul>
            </div>

            <div className="bg-muted text-foreground relative -mb-px flex aspect-[335/376] w-full shrink-0 items-center justify-center overflow-hidden rounded-t-lg shadow-[inset_0px_0px_0px_1px_rgba(26,26,0,0.16)] lg:mb-0 lg:-ml-px lg:aspect-auto lg:w-[438px] lg:rounded-t-none lg:rounded-r-lg dark:shadow-[inset_0px_0px_0px_1px_#fffaed2d]">
              <div
                aria-hidden
                className="absolute inset-0 bg-[radial-gradient(var(--border)_1px,transparent_1px)] [mask-image:radial-gradient(ellipse_at_center,black_30%,transparent_75%)] bg-[size:16px_16px]"
              />
              <div className="bg-background relative flex size-32 items-center justify-center rounded-3xl border shadow-sm lg:size-40">
                <AppLogoIcon className="size-14 lg:size-[4.5rem]" />
              </div>
            </div>
          </main>
        </div>
      </div>
    </>
  )
}

function ResourceItem({ text, href }: { text: string; href: string }) {
  return (
    <li className="relative flex items-center gap-4 py-2">
      <span className="flex h-3.5 w-3.5 items-center justify-center rounded-full border border-[#e3e3e0] bg-[#FDFDFC] shadow-[0px_0px_1px_0px_rgba(0,0,0,0.03),0px_1px_2px_0px_rgba(0,0,0,0.06)] dark:border-[#3E3E3A] dark:bg-[#161615]">
        <span className="h-1.5 w-1.5 rounded-full bg-[#dbdbd7] dark:bg-[#3E3E3A]" />
      </span>
      <a
        href={href}
        target="_blank"
        className="inline-flex items-center space-x-1 font-medium text-[#f53003] underline underline-offset-4 dark:text-[#FF4433]"
        rel="noreferrer"
      >
        <span>{text}</span>
        <svg width={10} height={11} viewBox="0 0 10 11" className="h-2.5 w-2.5">
          <path
            d="M7.70833 6.95834V2.79167H3.54167M2.5 8L7.5 3.00001"
            stroke="currentColor"
            strokeLinecap="square"
          />
        </svg>
      </a>
    </li>
  )
}
