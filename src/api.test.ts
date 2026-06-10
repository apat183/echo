import { describe, expect, it } from "vitest";
import { addDays, appColor, fmtDur, initials, monthKey, toDateStr, weekKey } from "./api";

describe("fmtDur", () => {
  it("renders sub-minute, minutes, and hours", () => {
    expect(fmtDur(0)).toBe("<1m");
    expect(fmtDur(29)).toBe("<1m");
    expect(fmtDur(30)).toBe("1m");
    expect(fmtDur(60)).toBe("1m");
    expect(fmtDur(3540)).toBe("59m");
    expect(fmtDur(3600)).toBe("1h");
    expect(fmtDur(3660)).toBe("1h 1m");
    expect(fmtDur(5400)).toBe("1h 30m");
  });
});

describe("addDays", () => {
  it("moves forward and backward across month and year boundaries", () => {
    expect(addDays("2026-06-10", 1)).toBe("2026-06-11");
    expect(addDays("2026-06-30", 1)).toBe("2026-07-01");
    expect(addDays("2026-01-01", -1)).toBe("2025-12-31");
  });
});

describe("toDateStr", () => {
  it("formats a local date as zero-padded YYYY-MM-DD", () => {
    expect(toDateStr(new Date(2026, 5, 9))).toBe("2026-06-09");
    expect(toDateStr(new Date(2026, 0, 1))).toBe("2026-01-01");
  });
});

describe("weekKey", () => {
  it("returns the Monday of the week", () => {
    expect(weekKey("2026-06-10")).toBe("2026-06-08"); // Wed -> Mon
    expect(weekKey("2026-06-08")).toBe("2026-06-08"); // Mon -> itself
    expect(weekKey("2026-06-14")).toBe("2026-06-08"); // Sun -> Mon
  });
});

describe("monthKey", () => {
  it("returns YYYY-MM", () => {
    expect(monthKey("2026-06-10")).toBe("2026-06");
  });
});

describe("initials", () => {
  it("derives one or two uppercase letters", () => {
    expect(initials("Visual Studio Code")).toBe("VS");
    expect(initials("Finder")).toBe("FI");
    expect(initials("a")).toBe("A");
    expect(initials("")).toBe("?");
  });
});

describe("appColor", () => {
  it("is deterministic and returns an hsl string", () => {
    expect(appColor("com.apple.Safari")).toMatch(/^hsl\(/);
    expect(appColor("com.apple.Safari")).toBe(appColor("com.apple.Safari"));
  });
});
