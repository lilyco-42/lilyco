import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { parse } from "smol-toml";
import { defineConfig } from "vite";

export default defineConfig(({ command }) => {
  const setup = {
    plugins: [react(), tailwindcss()],
    resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  };
  if (command !== "serve") return setup;
  let target = process.env.RAPID_API_URL;
  if (!target) {
    const root = fileURLToPath(new URL("..", import.meta.url));
    const path = resolve(root, process.env.RAPID_CONFIG ?? "Rapid.toml");
    const config = parse(readFileSync(path, "utf8"));
    const http = config.http;
    const address =
      http && typeof http === "object" && !Array.isArray(http) && "address" in http
        ? http.address
        : null;
    if (typeof address !== "string") throw new Error("Rapid.toml must define http.address");
    target = "http://" + (process.env.RAPID_BIND ?? address);
  }
  const proxy = Object.fromEntries(
    ["/api", "/openapi.json", "/healthz", "/readyz"].map((path) => [path, { target }]),
  );
  const port = Number(process.env.RAPID_WEB_PORT ?? 5173);
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error("Invalid RAPID_WEB_PORT");
  return { ...setup, server: { proxy, host: "127.0.0.1", port, strictPort: true } };
});
