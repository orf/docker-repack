// @ts-check
import { defineConfig } from "astro/config";

import tailwind from "@astrojs/tailwind";
import ViteYaml from "@modyfi/vite-plugin-yaml";
import react from "@astrojs/react";

export default defineConfig({
  site: process.env.SITE_ORIGIN,
  base: process.env.SITE_PREFIX,
  integrations: [tailwind(), react()],
  vite: {
    plugins: [ViteYaml()],
  },
});
