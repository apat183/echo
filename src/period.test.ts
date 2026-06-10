import { describe, expect, it } from "vitest";
import {
  addMonths,
  bucketLabel,
  labelHour,
  periodDates,
  periodEnd,
  periodStart,
  rollup,
  samePeriod,
  shiftPeriod,
} from "./period";

describe("periodStart / periodEnd", () => {
  it("day is a single date", () => {
    expect(periodStart("2026-06-15", "day")).toBe("2026-06-15");
    expect(periodEnd("2026-06-15", "day")).toBe("2026-06-15");
  });

  it("month spans the first to the last day, leap-aware", () => {
    expect(periodStart("2026-06-15", "month")).toBe("2026-06-01");
    expect(periodEnd("2026-06-01", "month")).toBe("2026-06-30");
    expect(periodEnd("2026-02-01", "month")).toBe("2026-02-28"); // non-leap
    expect(periodEnd("2024-02-01", "month")).toBe("2024-02-29"); // leap
  });
});

describe("shiftPeriod", () => {
  it("steps by day, week, and month", () => {
    expect(shiftPeriod("2026-06-15", "day", -1)).toBe("2026-06-14");
    expect(shiftPeriod("2026-06-15", "week", 1)).toBe("2026-06-22");
    expect(shiftPeriod("2026-06-15", "month", 1)).toBe("2026-07-01");
  });
});

describe("samePeriod", () => {
  it("compares within the same granularity", () => {
    expect(samePeriod("2026-06-01", "2026-06-30", "month")).toBe(true);
    expect(samePeriod("2026-06-01", "2026-07-01", "month")).toBe(false);
  });
});

describe("addMonths", () => {
  it("rolls over the year in both directions", () => {
    expect(addMonths("2026-12-15", 1)).toBe("2027-01-01");
    expect(addMonths("2026-01-15", -1)).toBe("2025-12-01");
  });
});

describe("periodDates", () => {
  it("enumerates every day of a past month, capped at the month end", () => {
    const dates = periodDates("2020-02-15", "month"); // leap Feb, fully in the past
    expect(dates).toHaveLength(29);
    expect(dates[0]).toBe("2020-02-01");
    expect(dates[28]).toBe("2020-02-29");
  });
});

describe("rollup", () => {
  it("sums per-day totals into buckets, newest first", () => {
    const buckets = rollup(
      [
        { date: "2026-06-10", seconds: 100 },
        { date: "2026-06-10", seconds: 50 },
        { date: "2026-06-09", seconds: 30 },
      ],
      "day"
    );
    expect(buckets).toHaveLength(2);
    expect(buckets[0]).toMatchObject({ key: "2026-06-10", seconds: 150 });
    expect(buckets[1]).toMatchObject({ key: "2026-06-09", seconds: 30 });
  });
});

describe("bucketLabel / labelHour", () => {
  it("labels weeks and 12-hour clock hours", () => {
    expect(bucketLabel("2026-06-08", "week")).toMatch(/^Week of /);
    expect(labelHour(0)).toBe("12 AM");
    expect(labelHour(9)).toBe("9 AM");
    expect(labelHour(12)).toBe("12 PM");
    expect(labelHour(15)).toBe("3 PM");
    expect(labelHour(23)).toBe("11 PM");
  });
});
