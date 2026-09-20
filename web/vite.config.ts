import { defineConfig } from "vite";

export default defineConfig({
  base: "/mmorpg/",
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
