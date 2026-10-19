/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import preact from "@preact/preset-vite";

// La consola se sirve bajo /console desde el binario (rust-embed).
export default defineConfig({
  plugins: [preact()],
  base: "/console/",
  build: { outDir: "dist", target: "es2022" },
  test: { environment: "node" },
});
