import { h } from "vue"
import DefaultTheme from "vitepress/theme"
import type { Theme } from "vitepress"

// The landing page's code example, shown right under the hero (like inertia-rails.dev),
// before the feature cards that VitePress renders from index.md's frontmatter.
import HomeCode from "../../partials/home-code.md"

import "./custom.css"

export default {
  extends: DefaultTheme,
  Layout: () =>
    h(DefaultTheme.Layout, null, {
      "home-features-before": () => h("div", { class: "home-code" }, [h("div", { class: "vp-doc" }, [h(HomeCode)])]),
    }),
} satisfies Theme
