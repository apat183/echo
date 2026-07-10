import { afterEach, describe, expect, it } from "vitest";
import { applyTheme, loadTheme, saveTheme, type Theme } from "./theme";

afterEach(() => {
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
});

describe("applyTheme", () => {
  it("sets data-theme for an explicit light/dark choice", () => {
    applyTheme("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    applyTheme("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("clears data-theme for system so the OS media query governs", () => {
    applyTheme("dark");
    applyTheme("system");
    expect(document.documentElement.hasAttribute("data-theme")).toBe(false);
  });
});

describe("loadTheme / saveTheme", () => {
  it("defaults to system when nothing is stored", () => {
    expect(loadTheme()).toBe("system");
  });

  it("round-trips every theme through localStorage", () => {
    for (const t of ["light", "dark", "system"] as Theme[]) {
      saveTheme(t);
      expect(loadTheme()).toBe(t);
    }
  });

  it("falls back to system for an unrecognized stored value", () => {
    localStorage.setItem("echo.theme", "neon");
    expect(loadTheme()).toBe("system");
  });
});
