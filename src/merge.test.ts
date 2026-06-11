import { describe, expect, it } from "vitest";
import type { DayView } from "./api";
import { mergeDayViews, partitionTitles, type PeriodTitleUsage } from "./merge";

function hours(hourIdx: number, seconds: number): number[] {
  const h = Array.from({ length: 24 }, () => 0);
  h[hourIdx] = seconds;
  return h;
}

function day(
  date: string,
  seconds: number,
  projectIds: number[],
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
        project_ids: projectIds,
        titles: [{ title: "doc", seconds, project_ids: projectIds }],
      },
    ],
  };
}

describe("mergeDayViews", () => {
  it("sums app and title time and hours across days, collecting project days", () => {
    const merged = mergeDayViews(
      [day("2026-06-09", 100, [1], 9), day("2026-06-10", 50, [1], 10)],
      "week",
      "2026-06-10"
    );

    expect(merged.total_seconds).toBe(150);
    expect(merged.apps).toHaveLength(1);

    const app = merged.apps[0];
    expect(app.seconds).toBe(150);
    expect(app.dates).toEqual(["2026-06-09", "2026-06-10"]);
    expect(app.projects).toEqual([{ id: 1, dates: ["2026-06-09", "2026-06-10"] }]);
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
    expect(app.titles[0].projects).toEqual([{ id: 1, dates: ["2026-06-09", "2026-06-10"] }]);
  });

  it("leaves projects empty when nothing is assigned", () => {
    const merged = mergeDayViews([day("2026-06-10", 60, [], 8)], "day", "2026-06-10");
    expect(merged.apps[0].projects).toEqual([]);
    expect(merged.apps[0].titles[0].projects).toEqual([]);
  });

  it("accumulates multiple project_ids on app and title into separate RowProjects", () => {
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
          project_ids: [1, 2],
          titles: [{ title: "report", seconds: 120, project_ids: [1, 2] }],
        },
      ],
    };

    const merged = mergeDayViews([view], "day", "2026-06-10");
    const app = merged.apps[0];

    expect(app.projects).toEqual([
      { id: 1, dates: ["2026-06-10"] },
      { id: 2, dates: ["2026-06-10"] },
    ]);
    expect(app.titles[0].projects).toEqual([
      { id: 1, dates: ["2026-06-10"] },
      { id: 2, dates: ["2026-06-10"] },
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
