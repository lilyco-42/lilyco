const THEME_KEY = "lilyco-site-theme";
const SCALE_KEY = "lilyco-site-scale";

function applyTheme(mode) {
  if (mode === "light" || mode === "dark") {
    document.documentElement.dataset.theme = mode;
    localStorage.setItem(THEME_KEY, mode);
  } else {
    document.documentElement.removeAttribute("data-theme");
    localStorage.removeItem(THEME_KEY);
  }
}

function currentTheme() {
  return document.documentElement.dataset.theme || "auto";
}

function cycleTheme() {
  const next = currentTheme() === "auto" ? "dark" : currentTheme() === "dark" ? "light" : "auto";
  applyTheme(next);
  const btn = document.getElementById("theme-toggle");
  if (btn) btn.textContent = next === "dark" ? "☾" : next === "light" ? "☀" : "◐";
}

function applyScale(pct) {
  const clamped = Math.max(80, Math.min(160, Math.round(pct / 10) * 10));
  document.documentElement.style.fontSize = `${16 * clamped / 100}px`;
  localStorage.setItem(SCALE_KEY, String(clamped));
  const out = document.getElementById("zoom-value");
  if (out) out.textContent = `${clamped}%`;
}

function zoom(delta) {
  const cur = Number(localStorage.getItem(SCALE_KEY) || 100);
  applyScale(cur + delta);
}

document.addEventListener("DOMContentLoaded", () => {
  const savedTheme = localStorage.getItem(THEME_KEY);
  if (savedTheme) applyTheme(savedTheme);
  applyScale(Number(localStorage.getItem(SCALE_KEY) || 100));

  const btn = document.getElementById("theme-toggle");
  if (btn) {
    btn.textContent = currentTheme() === "dark" ? "☾" : currentTheme() === "light" ? "☀" : "◐";
    btn.addEventListener("click", cycleTheme);
  }
  for (const b of document.querySelectorAll("[data-zoom]")) {
    b.addEventListener("click", () => zoom(Number(b.dataset.zoom)));
  }
});