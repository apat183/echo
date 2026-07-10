// Light / Dark / System theme, persisted in localStorage (key `echo.theme`,
// same convention as `echo.sidebar-collapsed`). "System" clears the
// `data-theme` attribute so the OS `prefers-color-scheme` media query governs
// (App.css); Light/Dark set an explicit attribute that overrides it.

export type Theme = "light" | "dark" | "system";

const STORAGE_KEY = "echo.theme";

/** Set (or, for "system", clear) the `data-theme` attribute on <html>. */
export function applyTheme(theme: Theme): void {
  const root = document.documentElement;
  if (theme === "system") {
    root.removeAttribute("data-theme");
  } else {
    root.setAttribute("data-theme", theme);
  }
}

/** The saved theme, defaulting to "system" when unset or unrecognized. */
export function loadTheme(): Theme {
  const saved = localStorage.getItem(STORAGE_KEY);
  return saved === "light" || saved === "dark" || saved === "system" ? saved : "system";
}

export function saveTheme(theme: Theme): void {
  localStorage.setItem(STORAGE_KEY, theme);
}
