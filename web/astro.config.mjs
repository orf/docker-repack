// @ts-check
import { defineConfig } from "astro/config";

import tailwind from "@astrojs/tailwind";
import ViteYaml from "@modyfi/vite-plugin-yaml";
import react from "@astrojs/react";

// https://astro.build/config
export default defineConfig({
  integrations: [tailwind(), react()],
  vite: {
    plugins: [ViteYaml()],
  },
});
