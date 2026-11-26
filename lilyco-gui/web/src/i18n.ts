import { createContext, useContext } from "react";

export type Locale = "zh-CN" | "en";
export type Translate = (key: string, values?: Record<string, string | number>) => string;

const messages: Partial<Record<Locale, Record<string, string>>> = JSON.parse(
  document.getElementById("lilyco-messages")?.textContent ?? "{}",
);

export function translate(locale: Locale): Translate {
  return (key, values = {}) => (messages[locale]?.[key] ?? messages["zh-CN"]?.[key] ?? key)
    .replace(/\{([a-z_]+)\}/g, (_, name: string) => String(values[name] ?? ""));
}

export const Language = createContext(translate("zh-CN"));
export const useText = () => useContext(Language);

export function preference(key: string): string | null {
  try { return localStorage.getItem(key); } catch { return null; }
}

export function setPreference(key: string, value: string) {
  try { localStorage.setItem(key, value); } catch { /* Storage can be disabled. */ }
}
