// Folds several single-day DayViews into one PeriodView for the week/month
// activity views: app + title time summed, hour buckets added, and the set of
// days each project link covers preserved (so a dot can unassign just its days).

import type { AppUsage, DayView } from "./api";
import {
  addHours,
  emptyHours,
  labelHour,
  periodFrameDates,
  periodLabel,
  prettyDate,
  type Granularity,
} from "./period";

export type RowProject = { id: number; dates: string[] };

export type PeriodChartBucket = {
  key: string;
  label: string;
  axisLabel: string;
  seconds: number;
};

export type PeriodTitleUsage = {
  title: string;
  seconds: number;
  dates: string[];
  projects: RowProject[]; // explicit title-level links across the period
};

export type PeriodAppUsage = Omit<AppUsage, "project_ids" | "titles"> & {
  timeline: PeriodChartBucket[];
  dates: string[];
  projects: RowProject[]; // app-level (title="") links across the period
  titles: PeriodTitleUsage[];
};

export type PeriodView = {
  label: string;
  total_seconds: number;
  apps: PeriodAppUsage[];
  hours: number[];
  timeline: PeriodChartBucket[];
};

export function mergeDayViews(
  days: DayView[],
  gran: Granularity,
  anchorDate: string
): PeriodView {
  type TitleAcc = { seconds: number; dates: string[]; datesByProject: Map<number, string[]> };
  type Accumulator = {
    app_key: string;
    app_name: string;
    bundle_id: string | null;
    seconds: number;
    hours: number[];
    dates: string[];
    datesByProject: Map<number, string[]>; // app-level links
    secondsByDate: Map<string, number>;
    titles: Map<string, TitleAcc>;
  };

  const byApp = new Map<string, Accumulator>();
  const hours = emptyHours();
  const secondsByDate = new Map<string, number>();
  let total = 0;

  for (const day of days) {
    total += day.total_seconds;
    secondsByDate.set(day.date, (secondsByDate.get(day.date) ?? 0) + day.total_seconds);
    addHours(hours, day.hours);

    for (const app of day.apps) {
      const entry =
        byApp.get(app.app_key) ??
        ({
          app_key: app.app_key,
          app_name: app.app_name,
          bundle_id: app.bundle_id,
          seconds: 0,
          hours: emptyHours(),
          dates: [],
          datesByProject: new Map<number, string[]>(),
          secondsByDate: new Map<string, number>(),
          titles: new Map<string, TitleAcc>(),
        } satisfies Accumulator);

      entry.seconds += app.seconds;
      addHours(entry.hours, app.hours);
      entry.dates.push(day.date);
      entry.secondsByDate.set(day.date, (entry.secondsByDate.get(day.date) ?? 0) + app.seconds);
      for (const pid of app.project_ids) pushDate(entry.datesByProject, pid, day.date);

      for (const t of app.titles) {
        const tacc =
          entry.titles.get(t.title) ??
          { seconds: 0, dates: [], datesByProject: new Map<number, string[]>() };
        tacc.seconds += t.seconds;
        tacc.dates.push(day.date);
        for (const pid of t.project_ids) pushDate(tacc.datesByProject, pid, day.date);
        entry.titles.set(t.title, tacc);
      }

      byApp.set(app.app_key, entry);
    }
  }

  const apps: PeriodAppUsage[] = [...byApp.values()]
    .map((app) => ({
      app_key: app.app_key,
      app_name: app.app_name,
      bundle_id: app.bundle_id,
      seconds: app.seconds,
      hours: app.hours,
      timeline: buildTimeline(gran, anchorDate, app.hours, app.secondsByDate),
      dates: [...new Set(app.dates)].sort(),
      projects: rowProjects(app.datesByProject),
      titles: [...app.titles.entries()]
        .map(([title, t]) => ({
          title,
          seconds: t.seconds,
          dates: [...new Set(t.dates)].sort(),
          projects: rowProjects(t.datesByProject),
        }))
        .sort((a, b) => b.seconds - a.seconds),
    }))
    .sort((a, b) => b.seconds - a.seconds);

  return {
    label: periodLabel(anchorDate, gran),
    total_seconds: total,
    apps,
    hours,
    timeline: buildTimeline(gran, anchorDate, hours, secondsByDate),
  };
}

function buildTimeline(
  gran: Granularity,
  anchorDate: string,
  hours: number[],
  secondsByDate: Map<string, number>,
): PeriodChartBucket[] {
  if (gran === "day") {
    return hours.map((seconds, hour) => ({
      key: String(hour),
      label: labelHour(hour),
      axisLabel: labelHour(hour),
      seconds,
    }));
  }

  return periodFrameDates(anchorDate, gran).map((date) => ({
    key: date,
    label: prettyDate(date),
    axisLabel: gran === "week" ? weekdayLabel(date) : String(Number(date.slice(8, 10))),
    seconds: secondsByDate.get(date) ?? 0,
  }));
}

function weekdayLabel(dateStr: string): string {
  const [y, m, d] = dateStr.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString([], { weekday: "short" });
}

function pushDate(map: Map<number, string[]>, id: number, date: string) {
  const list = map.get(id) ?? [];
  list.push(date);
  map.set(id, list);
}

function rowProjects(map: Map<number, string[]>): RowProject[] {
  return [...map.entries()]
    .map(([id, dates]) => ({ id, dates: [...new Set(dates)].sort() }))
    .sort((a, b) => a.id - b.id);
}

/**
 * Splits a list of PeriodTitleUsage rows into major (visible) and tiny
 * (sub-threshold) groups. The "" untitled row always stays in major.
 * Grouping only happens when there are at least 2 tiny rows; a single
 * tiny row is not worth collapsing. Input order is preserved in both outputs.
 */
export function partitionTitles(
  titles: PeriodTitleUsage[],
  thresholdSeconds = 60,
): { major: PeriodTitleUsage[]; tiny: PeriodTitleUsage[] } {
  const major: PeriodTitleUsage[] = [];
  const tiny: PeriodTitleUsage[] = [];

  for (const t of titles) {
    if (t.title.trim() === "" || t.seconds >= thresholdSeconds) {
      major.push(t);
    } else {
      tiny.push(t);
    }
  }

  // Only group when there are at least 2 tiny rows; otherwise keep input order
  if (tiny.length < 2) {
    return { major: [...titles], tiny: [] };
  }

  return { major, tiny };
}
