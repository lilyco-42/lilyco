import { fileURLToPath, URL } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [tailwindcss(), {
    name: "lilyco-trim-generated-blank-lines",
    generateBundle(_options, bundle) {
      for (const asset of Object.values(bundle)) {
        if (asset.type === "chunk") asset.code = asset.code.replace(/^[ \t]+$/gm, "");
      }
    },
  }],
  define: { "process.env.NODE_ENV": '"production"' },
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  build: {
    outDir: "../assets/shadcn",
    emptyOutDir: false,
    rollupOptions: {
      onwarn(warning, warn) {
        if (warning.code !== "MODULE_LEVEL_DIRECTIVE") warn(warning);
      },
    },
    lib: { entry: "src/main.tsx", name: "LilycoGui", formats: ["iife"], fileName: () => "gui.js", cssFileName: "gui" },
  },
});
