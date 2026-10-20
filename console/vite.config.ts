/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import preact from "@preact/preset-vite";
import tailwindcss from "@tailwindcss/vite";

// La consola se sirve bajo /console desde el binario (rust-embed).
export default defineConfig({
  plugins: [preact(), tailwindcss()],
  base: "/console/",
  build: { outDir: "dist", target: "es2022" },
  test: { environment: "node" },
});
