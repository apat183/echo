import { describe, expect, it } from "vitest";
import type { DayView, RowProject } from "./api";
import { mergeDayViews, partitionTitles, type PeriodTitleUsage } from "./merge";

function direct(id: number): RowProject {
  return { project_id: id, state: "direct", rule_id: null };
}

function hours(hourIdx: number, seconds: number): number[] {
  const h = Array.from({ length: 24 }, () => 0);
  h[hourIdx] = seconds;
  return h;
}

function day(
  date: string,
  seconds: number,
  projects: RowProject[],
  hourIdx: number
): DayView {
  return {
    date,
    total_seconds: seconds,
    hours: hours(hourIdx, seconds),
    apps: [
      {
        app_key: "com.a",
        app_name: "App A",
        bundle_id: "com.a",
        seconds,
        hours: hours(hourIdx, seconds),
        projects,
        titles: [{ title: "doc", seconds, projects }],
      },
    ],
  };
}

describe("mergeDayViews", () => {
  it("sums app and title time and hours across days, collecting project days", () => {
    const merged = mergeDayViews(
      [day("2026-06-09", 100, [direct(1)], 9), day("2026-06-10", 50, [direct(1)], 10)],
      "week",
      "2026-06-10"
    );

    expect(merged.total_seconds).toBe(150);
    expect(merged.apps).toHaveLength(1);

    const app = merged.apps[0];
    expect(app.seconds).toBe(150);
    expect(app.dates).toEqual(["2026-06-09", "2026-06-10"]);
    expect(app.projects).toEqual([
      {
        id: 1,
        state: "direct",
        ruleId: null,
        includedDates: ["2026-06-09", "2026-06-10"],
        excludedDates: [],
      },
    ]);
    expect(app.hours[9]).toBe(100);
    expect(app.hours[10]).toBe(50);
    expect(merged.timeline.map((b) => [b.key, b.seconds])).toEqual([
      ["2026-06-08", 0],
      ["2026-06-09", 100],
      ["2026-06-10", 50],
      ["2026-06-11", 0],
      ["2026-06-12", 0],
      ["2026-06-13", 0],
      ["2026-06-14", 0],
    ]);
    expect(app.timeline.map((b) => [b.key, b.seconds])).toEqual([
      ["2026-06-08", 0],
      ["2026-06-09", 100],
      ["2026-06-10", 50],
      ["2026-06-11", 0],
      ["2026-06-12", 0],
      ["2026-06-13", 0],
      ["2026-06-14", 0],
    ]);

    expect(app.titles).toHaveLength(1);
    expect(app.titles[0]).toMatchObject({ title: "doc", seconds: 150 });
    expect(app.titles[0].projects).toEqual([
      {
        id: 1,
        state: "direct",
        ruleId: null,
        includedDates: ["2026-06-09", "2026-06-10"],
        excludedDates: [],
      },
    ]);
  });

  it("leaves projects empty when nothing is assigned", () => {
    const merged = mergeDayViews([day("2026-06-10", 60, [], 8)], "day", "2026-06-10");
    expect(merged.apps[0].projects).toEqual([]);
    expect(merged.apps[0].titles[0].projects).toEqual([]);
  });

  it("accumulates several projects on one row into separate RowProjects", () => {
    const view: DayView = {
      date: "2026-06-10",
      total_seconds: 120,
      hours: hours(9, 120),
      apps: [
        {
          app_key: "com.b",
          app_name: "App B",
          bundle_id: "com.b",
          seconds: 120,
          hours: hours(9, 120),
          projects: [direct(1), direct(2)],
          titles: [{ title: "report", seconds: 120, projects: [direct(1), direct(2)] }],
        },
      ],
    };

    const merged = mergeDayViews([view], "day", "2026-06-10");
    const app = merged.apps[0];

    expect(app.projects.map((p) => p.id)).toEqual([1, 2]);
    expect(app.titles[0].projects.map((p) => p.id)).toEqual([1, 2]);
  });

  it("prefers a hand-made link over a rule when folding a period", () => {
    const merged = mergeDayViews(
      [
        day("2026-06-09", 100, [{ project_id: 1, state: "rule", rule_id: 7 }], 9),
        day("2026-06-10", 50, [direct(1)], 10),
      ],
      "week",
      "2026-06-10"
    );

    // One dot for the period. It offers to undo the thing you did by hand,
    // so it must read as direct even though most days came from the rule.
    expect(merged.apps[0].projects).toEqual([
      {
        id: 1,
        state: "direct",
        ruleId: 7,
        includedDates: ["2026-06-09", "2026-06-10"],
        excludedDates: [],
      },
    ]);
  });

  it("keeps excluded days separate so clearing acts only on them", () => {
    const merged = mergeDayViews(
      [
        day("2026-06-09", 100, [{ project_id: 1, state: "excluded", rule_id: null }], 9),
        day("2026-06-10", 50, [{ project_id: 1, state: "excluded", rule_id: null }], 10),
      ],
      "week",
      "2026-06-10"
    );

    expect(merged.apps[0].projects).toEqual([
      {
        id: 1,
        state: "excluded",
        ruleId: null,
        includedDates: [],
        excludedDates: ["2026-06-09", "2026-06-10"],
      },
    ]);
  });
});

// ---- partitionTitles -------------------------------------------------------

function pt(title: string, seconds: number): PeriodTitleUsage {
  return { title, seconds, dates: ["2026-06-10"], projects: [] };
}

describe("partitionTitles", () => {
  it("moves two tiny titled rows to tiny output", () => {
    const titles = [pt("a", 30), pt("b", 45), pt("c", 120)];
    const { major, tiny } = partitionTitles(titles);
    expect(major.map((t) => t.title)).toEqual(["c"]);
    expect(tiny.map((t) => t.title)).toEqual(["a", "b"]);
  });

  it("keeps a single tiny row in major (no grouping)", () => {
    const titles = [pt("a", 30), pt("b", 120)];
    const { major, tiny } = partitionTitles(titles);
    expect(major.map((t) => t.title)).toEqual(["a", "b"]);
    expect(tiny).toHaveLength(0);
  });

  it("never moves the '' untitled row even when it is under threshold", () => {
    const titles = [pt("", 10), pt("x", 20), pt("y", 25)];
    const { major, tiny } = partitionTitles(titles);
    expect(major.map((t) => t.title)).toContain("");
    expect(tiny.map((t) => t.title)).not.toContain("");
  });

  it("treats exactly threshold seconds as major (boundary)", () => {
    const titles = [pt("a", 60), pt("b", 59), pt("c", 30)];
    const { major, tiny } = partitionTitles(titles);
    expect(major.map((t) => t.title)).toContain("a");
    expect(tiny.map((t) => t.title)).toEqual(["b", "c"]);
  });

  it("preserves input order across both outputs", () => {
    const titles = [pt("first", 30), pt("second", 200), pt("third", 10)];
    const { major, tiny } = partitionTitles(titles);
    expect(major.map((t) => t.title)).toEqual(["second"]);
    expect(tiny.map((t) => t.title)).toEqual(["first", "third"]);
  });

  it("returns everything in major when there are zero tiny rows", () => {
    const titles = [pt("a", 100), pt("b", 200)];
    const { major, tiny } = partitionTitles(titles);
    expect(major).toHaveLength(2);
    expect(tiny).toHaveLength(0);
  });

  it("respects a custom threshold", () => {
    const titles = [pt("a", 120), pt("b", 119), pt("c", 50)];
    const { major, tiny } = partitionTitles(titles, 120);
    expect(major.map((t) => t.title)).toEqual(["a"]);
    expect(tiny.map((t) => t.title)).toEqual(["b", "c"]);
  });
});
