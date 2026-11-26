import type { ArgKind, ArgSchema } from "./contract";

export type Draft = string | boolean | Draft[];
export type Argument = null | string | boolean | number | Argument[];
export type Drafts = Record<string, Draft>;

export function initialValue(kind: ArgKind, value: unknown): Draft {
  if (kind.type === "list") return Array.isArray(value) ? value.map(item => initialValue(kind.item, item)) : [];
  if (kind.type === "flag") return value === true || value === "true" || value === "1";
  return typeof value === "string" || typeof value === "number" ? String(value) : "";
}

export function createDrafts(args: ArgSchema[], search: string): Drafts {
  const params = new URLSearchParams(search);
  return Object.fromEntries(args.map(arg => {
    const given = params.get(arg.name) ?? params.get(arg.name.replaceAll("-", "_"));
    const value = arg.kind.type === "flag" && given === "" ? true : given ?? arg.default;
    return [arg.name, initialValue(arg.kind, value)];
  }));
}

export function argumentValue(kind: ArgKind, value: Draft): Argument {
  if (kind.type === "list") {
    const converted = Array.isArray(value) ? value.map(row => argumentValue(kind.item, row)) : [];
    return converted.filter(item => item !== null);
  }
  if (kind.type === "flag") return value === true;
  if (typeof value !== "string" || value === "") return null;
  if (kind.type === "number" && Number.isFinite(Number(value))) return Number(value);
  return value; // Rust reports invalid types and ranges for the original submission.
}

function quote(value: string): string {
  return /^[\w./:@%+=,-]+$/.test(value) ? value : "'" + value.replaceAll("'", "'\\''") + "'";
}

export function cliPreview(command: string, args: Record<string, Argument>): string {
  const parts = [quote(command)];
  for (const [key, value] of Object.entries(args)) {
    if (value === true) parts.push("--" + key);
    else if (Array.isArray(value)) {
      for (const item of value) parts.push("--" + key, quote(typeof item === "string" ? item : JSON.stringify(item)));
    } else if (value !== null && value !== false) parts.push("--" + key, quote(String(value)));
  }
  return parts.join(" ");
}
