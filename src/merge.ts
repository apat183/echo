// Folds several single-day DayViews into one PeriodView for the week/month
// activity views: app + title time summed, hour buckets added, and each
// project link's days preserved — split by whether the project counted that
// day or was deliberately excluded, so one dot can act on just the right days.

import type { AppUsage, DayView, LinkState, RowProject as DayRowProject } from "./api";
import {
  addHours,
  emptyHours,
  labelHour,
  periodFrameDates,
  periodLabel,
  prettyDate,
  type Granularity,
} from "./period";

/**
 * One project's standing against a row across a whole period.
 *
 * `state` is what to draw. Folding many days into one dot needs a winner, and
 * it is the most direct thing present: a day you linked by hand outranks a day
 * a rule covered, which outranks an excluded day. That way clicking the dot
 * always offers to undo the strongest claim on the row.
 */
export type RowProject = {
  id: number;
  state: Extract<LinkState, "direct" | "rule" | "excluded">;
  /** The rule responsible, when one is involved on any day of the period. */
  ruleId: number | null;
  includedDates: string[];
  excludedDates: string[];
};

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

export type PeriodAppUsage = Omit<AppUsage, "projects" | "titles"> & {
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
  type ProjectAcc = {
    state: RowProject["state"];
    ruleId: number | null;
    includedDates: string[];
    excludedDates: string[];
  };
  type TitleAcc = { seconds: number; dates: string[]; byProject: Map<number, ProjectAcc> };
  type Accumulator = {
    app_key: string;
    app_name: string;
    bundle_id: string | null;
    seconds: number;
    hours: number[];
    dates: string[];
    byProject: Map<number, ProjectAcc>; // app-level standing
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
          byProject: new Map<number, ProjectAcc>(),
          secondsByDate: new Map<string, number>(),
          titles: new Map<string, TitleAcc>(),
        } satisfies Accumulator);

      entry.seconds += app.seconds;
      addHours(entry.hours, app.hours);
      entry.dates.push(day.date);
      entry.secondsByDate.set(day.date, (entry.secondsByDate.get(day.date) ?? 0) + app.seconds);
      for (const link of app.projects) foldLink(entry.byProject, link, day.date);

      for (const t of app.titles) {
        const tacc = entry.titles.get(t.title) ?? {
          seconds: 0,
          dates: [],
          byProject: new Map<number, ProjectAcc>(),
        };
        tacc.seconds += t.seconds;
        tacc.dates.push(day.date);
        for (const link of t.projects) foldLink(tacc.byProject, link, day.date);
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
      projects: rowProjects(app.byProject),
      titles: [...app.titles.entries()]
        .map(([title, t]) => ({
          title,
          seconds: t.seconds,
          dates: [...new Set(t.dates)].sort(),
          projects: rowProjects(t.byProject),
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

/** Strength order for folding a period's days into one dot: a link you made by
 *  hand outranks one a rule made, which outranks a day you excluded. */
const LINK_RANK: Record<RowProject["state"], number> = {
  direct: 3,
  rule: 2,
  excluded: 1,
};

type FoldedLink = {
  state: RowProject["state"];
  ruleId: number | null;
  includedDates: string[];
  excludedDates: string[];
};

function foldLink(map: Map<number, FoldedLink>, link: DayRowProject, date: string) {
  // "inherited" never reaches a day row; treat anything unexpected as a rule
  // rather than dropping the day silently.
  const state: RowProject["state"] =
    link.state === "direct" || link.state === "excluded" ? link.state : "rule";
  const acc = map.get(link.project_id) ?? {
    state,
    ruleId: null,
    includedDates: [],
    excludedDates: [],
  };
  if (state === "excluded") {
    acc.excludedDates.push(date);
  } else {
    acc.includedDates.push(date);
  }
  if (LINK_RANK[state] > LINK_RANK[acc.state]) acc.state = state;
  // Keep the rule even when a hand-made link wins the dot, so the interface can
  // still name what would take over if that link were removed.
  acc.ruleId ??= link.rule_id;
  map.set(link.project_id, acc);
}

function rowProjects(map: Map<number, FoldedLink>): RowProject[] {
  return [...map.entries()]
    .map(([id, acc]) => ({
      id,
      state: acc.state,
      ruleId: acc.ruleId,
      includedDates: [...new Set(acc.includedDates)].sort(),
      excludedDates: [...new Set(acc.excludedDates)].sort(),
    }))
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
