import { useEffect, useLayoutEffect, useState } from "react";
import { MonitorIcon, MoonIcon, SunIcon } from "lucide-react";
import { Button } from "@/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from "@/ui/dropdown-menu";
import { preference, setPreference, useText } from "./i18n";

type Theme = "system" | "light" | "dark";
const key = "lilyco-theme";
const query = "(prefers-color-scheme: dark)";
const modes = {system: MonitorIcon, light: SunIcon, dark: MoonIcon} as const;

function themePreference(value: string | null): Theme {
  return value === "light" || value === "dark" ? value : "system";
}

export function ThemeMenu() {
  const [mode, setMode] = useState(() => themePreference(preference(key)));
  const t = useText();
  useLayoutEffect(() => {
    const media = matchMedia(query);
    const apply = () => document.documentElement.classList.toggle("dark", mode === "system" ? media.matches : mode === "dark");
    apply();
    if (mode !== "system") return;
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [mode]);

  useEffect(() => {
    const sync = (event: StorageEvent) => {
      if (event.key === key || event.key === null) setMode(themePreference(preference(key)));
    };
    window.addEventListener("storage", sync);
    return () => window.removeEventListener("storage", sync);
  }, []);

  const Icon = modes[mode];
  return <DropdownMenu>
    <DropdownMenuTrigger render={<Button variant="ghost" size="icon" aria-label={t("theme")}
      title={t("theme_" + mode)} />}><Icon /></DropdownMenuTrigger>
    <DropdownMenuContent align="end">
      <DropdownMenuRadioGroup value={mode} onValueChange={value => {
        const choice = themePreference(value);
        setMode(choice);
        setPreference(key, choice);
      }}>
        {(Object.keys(modes) as Theme[]).map(choice =>
          <DropdownMenuRadioItem key={choice} value={choice} closeOnClick className="min-h-11">{t("theme_" + choice)}</DropdownMenuRadioItem>)}
      </DropdownMenuRadioGroup>
    </DropdownMenuContent>
  </DropdownMenu>;
}
