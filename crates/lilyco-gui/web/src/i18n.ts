import { createContext, useContext, useEffect, useState } from "react";
import { resolveLocale, type Locale } from "./locale";

type LanguageMode = "system" | Locale;
export type Translate = (key: string, values?: Record<string, string | number>) => string;
const languageKey = "lilyco-language";

const messages: Partial<Record<Locale, Record<string, string>>> = JSON.parse(
  document.getElementById("lilyco-messages")?.textContent ?? "{}",
);

export function translate(locale: Locale): Translate {
  return (key, values = {}) => {
    const text = messages[locale]?.[key] ?? messages["zh-CN"]?.[key] ?? key;
    if (key.startsWith("app.")) return text;
    return text.replace(/\{([a-z_]+)\}/g, (_, name: string) => String(values[name] ?? ""));
  };
}

export const Language = createContext(translate("zh-CN"));
export const useText = () => useContext(Language);

export function preference(key: string): string | null {
  try { return localStorage.getItem(key); } catch { return null; }
}

export function setPreference(key: string, value: string) {
  try { localStorage.setItem(key, value); } catch { /* Storage can be disabled. */ }
}

function languageMode(value: string | null): LanguageMode {
  return value === "zh-CN" || value === "en" ? value : "system";
}

function browserLocale(): Locale {
  return resolveLocale([...navigator.languages, navigator.language]);
}

export function useLanguage() {
  const [mode, setMode] = useState(() => languageMode(preference(languageKey)));
  const [inherited, setInherited] = useState(browserLocale);
  useEffect(() => {
    const onLanguageChange = () => setInherited(browserLocale());
    const onStorage = (event: StorageEvent) => {
      if (event.key === languageKey || event.key === null) setMode(languageMode(preference(languageKey)));
    };
    window.addEventListener("languagechange", onLanguageChange);
    window.addEventListener("storage", onStorage);
    return () => {
      window.removeEventListener("languagechange", onLanguageChange);
      window.removeEventListener("storage", onStorage);
    };
  }, []);
  function choose(value: string) {
    const next = languageMode(value);
    if (next === "system") setInherited(browserLocale());
    setMode(next);
    setPreference(languageKey, next);
  }
  return {mode, locale: mode === "system" ? inherited : mode, choose};
}
