import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parse } from "smol-toml";
import { defineConfig, type PluginOption } from "vite";

export function rapidVite(plugins: PluginOption[]) {
  return defineConfig(({ command }) => {
    const setup = {
      plugins,
      resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
    };
    if (command !== "serve") return setup;
    const target = process.env.RAPID_API_URL ?? apiOrigin();
    const proxy = Object.fromEntries(
      ["/api", "/openapi.json", "/healthz", "/readyz"].map((path) => [path, { target }]),
    );
    const port = Number(process.env.RAPID_WEB_PORT ?? 5173);
    if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error("Invalid RAPID_WEB_PORT");
    return { ...setup, server: { proxy, host: "127.0.0.1", port, strictPort: true } };
  });
}

function apiOrigin() {
  const root = fileURLToPath(new URL("..", import.meta.url));
  const path = resolve(root, process.env.RAPID_CONFIG ?? "Rapid.toml");
  const { http } = parse(readFileSync(path, "utf8"));
  const address = http && typeof http === "object" && !Array.isArray(http) && "address" in http
    ? http.address : null;
  if (typeof address !== "string") throw new Error("Rapid.toml must define http.address");
  return "http://" + (process.env.RAPID_BIND ?? address);
}
