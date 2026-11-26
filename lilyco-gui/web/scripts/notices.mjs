import { execFileSync } from "node:child_process";
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const web = new URL("../", import.meta.url);
const tree = JSON.parse(execFileSync("pnpm", ["list", "--prod", "--depth", "Infinity", "--json"],
  { cwd: web, encoding: "utf8" }))[0];
const packages = new Map();

function record(entries) {
  for (const { path, dependencies } of Object.values(entries ?? {})) {
    if (!path || packages.has(path)) continue;
    packages.set(path, JSON.parse(readFileSync(join(path, "package.json"), "utf8")));
    record(dependencies);
  }
}

record(tree.dependencies);
const css = fileURLToPath(new URL("../node_modules/tailwindcss/", import.meta.url));
packages.set(css, JSON.parse(readFileSync(join(css, "package.json"), "utf8")));
const licenses = new Map();
for (const [path, { name, version, license }] of packages) {
  const key = `${name} ${version}`;
  if (licenses.has(key)) continue;
  const legal = readdirSync(path).filter(file => /^(license|copying|notice)([.-].*)?$/i.test(file)).sort();
  const content = legal.length
    ? legal.map(file => readFileSync(join(path, file), "utf8").trim()).join("\n\n")
    : readFileSync(new URL(`../licenses/${name.replaceAll("/", "-")}-${version}.txt`, import.meta.url), "utf8").trim();
  licenses.set(key, `${key} (${license})\n\n${content}`);
}

const shadcn = readFileSync(new URL("../src/ui/LICENSE.md", import.meta.url), "utf8").trim();
const text = ["shadcn/ui registry components\n\n" + shadcn,
  ...[...licenses].sort(([a], [b]) => a.localeCompare(b)).map(([, notice]) => notice)].join("\n\n---\n\n");
writeFileSync(new URL("../../assets/shadcn/THIRD-PARTY-NOTICES.txt", import.meta.url), text.replaceAll("\r\n", "\n") + "\n");
