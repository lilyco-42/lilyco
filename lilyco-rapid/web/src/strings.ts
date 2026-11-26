import { parse } from "smol-toml";
import english from "./strings.toml?raw";
import chinese from "./strings.zh-CN.toml?raw";

const language = navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
const fallback = parse(english);
const messages = language === "zh-CN" ? parse(chinese) : fallback;
document.documentElement.lang = language;

export function text(key: string): string {
  const value = messages[key] ?? fallback[key];
  return typeof value === "string" ? value : key;
}
