// `cf deploy` delegates the bundling to Wrangler, configured here.
import { defineWranglerConfig } from "wrangler/experimental-config"

export default defineWranglerConfig({ types: { generate: false } })
