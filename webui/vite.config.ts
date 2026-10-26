import { readFileSync } from "node:fs";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { viteSingleFile } from "vite-plugin-singlefile";

// The Rust server embeds a single self-contained dist/index.html, so every
// asset has to be inlined and no extra files may be emitted.
export default defineConfig({
  plugins: [react(), viteSingleFile(), {
    name: "embedded-legal-notices",
    transformIndexHtml: {
      order: "post",
      handler: (html) => {
        const legal = ["LICENSE.md", "THIRD_PARTY_NOTICES.md"].map(name => readFileSync(new URL(`../${name}`, import.meta.url), "utf8")).join("\n\n");
        const escaped = legal.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
        return html.replace("</body>", `<template id="db3-third-party-notices"><pre>${escaped}</pre></template></body>`);
      },
    },
  }],
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2020",
    cssCodeSplit: false,
    assetsInlineLimit: 100_000_000,
    chunkSizeWarningLimit: 100_000,
    rollupOptions: {
      output: { inlineDynamicImports: true },
    },
  },
});
