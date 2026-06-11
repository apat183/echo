// Pure period/calendar math for the day/week/month views. No React, no IPC —
// every function here is deterministic given its inputs (plus the current date
// for the "today" cap and the *_label helpers), which is what makes it testable.

import { addDays, monthKey, toDateStr, weekKey, type DayTotal } from "./api";

export type Granularity = "day" | "week" | "month";

export function emptyHours(): number[] {
  return Array.from({ length: 24 }, () => 0);
}

export function addHours(target: number[], source: number[]) {
  for (let i = 0; i < 24; i++) {
    target[i] += source[i] ?? 0;
  }
}

/** Local dates covered by the period containing `dateStr`, never past today. */
export function periodDates(dateStr: string, gran: Granularity): string[] {
  const today = toDateStr(new Date());
  const dates = periodFrameDates(dateStr, gran).filter((d) => d <= today);
  return dates.length > 0 ? dates : [periodStart(dateStr, gran)];
}

/** Local dates in the full visual frame for a period, including future empty days. */
export function periodFrameDates(dateStr: string, gran: Granularity): string[] {
  const start = periodStart(dateStr, gran);
  const end = periodEnd(start, gran);
  const dates: string[] = [];
  for (let d = start; d <= end; d = addDays(d, 1)) {
    dates.push(d);
  }
  return dates;
}

export function periodStart(dateStr: string, gran: Granularity): string {
  if (gran === "day") return dateStr;
  if (gran === "week") return weekKey(dateStr);
  return `${monthKey(dateStr)}-01`;
}

export function periodEnd(start: string, gran: Granularity): string {
  if (gran === "day") return start;
  if (gran === "week") return addDays(start, 6);
  return addDays(addMonths(start, 1), -1);
}

export function shiftPeriod(dateStr: string, gran: Granularity, amount: number): string {
  if (gran === "day") return addDays(dateStr, amount);
  if (gran === "week") return addDays(dateStr, amount * 7);
  return addMonths(periodStart(dateStr, "month"), amount);
}

export function samePeriod(a: string, b: string, gran: Granularity): boolean {
  return periodStart(a, gran) === periodStart(b, gran);
}

export function periodLabel(dateStr: string, gran: Granularity): string {
  const today = toDateStr(new Date());
  if (gran === "day") return dateStr === today ? "Today" : prettyDate(dateStr);
  if (gran === "week") {
    return samePeriod(dateStr, today, "week")
      ? "This Week"
      : `Week of ${prettyDate(weekKey(dateStr))}`;
  }
  return samePeriod(dateStr, today, "month") ? "This Month" : monthLabel(monthKey(dateStr));
}

export function addMonths(dateStr: string, amount: number): string {
  const [y, m] = dateStr.split("-").map(Number);
  return toDateStr(new Date(y, m - 1 + amount, 1));
}

export type Bucket = { key: string; seconds: number; label: string };

/** Group per-day totals into day/week/month buckets, newest bucket first. */
export function rollup(rows: DayTotal[], gran: Granularity): Bucket[] {
  const map = new Map<string, number>();
  for (const r of rows) {
    const key = gran === "day" ? r.date : gran === "week" ? weekKey(r.date) : monthKey(r.date);
    map.set(key, (map.get(key) ?? 0) + r.seconds);
  }
  return [...map.entries()]
    .sort((a, b) => (a[0] < b[0] ? 1 : -1))
    .map(([key, seconds]) => ({ key, seconds, label: bucketLabel(key, gran) }));
}

export function bucketLabel(key: string, gran: Granularity): string {
  if (gran === "month") return monthLabel(key);
  if (gran === "week") return `Week of ${prettyDate(key)}`;
  return prettyDate(key);
}

export function monthLabel(key: string): string {
  const [y, m] = key.split("-").map(Number);
  return new Date(y, m - 1, 1).toLocaleDateString([], { month: "long", year: "numeric" });
}

export function prettyDate(dateStr: string): string {
  const [y, m, d] = dateStr.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString([], {
    weekday: "short",
    day: "numeric",
    month: "short",
  });
}

export function labelHour(h: number): string {
  if (h === 0) return "12 AM";
  if (h < 12) return `${h} AM`;
  if (h === 12) return "12 PM";
  return `${h - 12} PM`;
}
