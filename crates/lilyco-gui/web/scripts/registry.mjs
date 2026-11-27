import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";

const lock = JSON.parse(readFileSync(new URL("../registry-lock.json", import.meta.url), "utf8"));
const files = readdirSync(new URL("../src/ui/", import.meta.url)).filter(path => path.endsWith(".tsx"));
if (files.length !== Object.keys(lock).length) throw new Error("Registry lock and component inventory disagree");
for (const file of files) {
  const entry = lock[file.slice(0, -4)];
  const bytes = readFileSync(new URL(`../src/ui/${file}`, import.meta.url));
  if (entry?.sha256 !== createHash("sha256").update(bytes).digest("hex")) {
    throw new Error(`shadcn/ui component hash differs: ${file}`);
  }
}
