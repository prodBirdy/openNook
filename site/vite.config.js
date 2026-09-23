import { defineConfig } from "vite";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  root,
  base: "./",
  publicDir: "public",
  build: {
    outDir: resolve(root, "../docs"),
    emptyOutDir: true,
    assetsDir: "assets",
    cssCodeSplit: false,
  },
});
