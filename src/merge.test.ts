import { describe, expect, it } from "vitest";
import type { DayView } from "./api";
import { mergeDayViews } from "./merge";

function hours(hourIdx: number, seconds: number): number[] {
  const h = Array.from({ length: 24 }, () => 0);
  h[hourIdx] = seconds;
  return h;
}

function day(
  date: string,
  seconds: number,
  projectId: number | null,
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
        project_id: projectId,
        titles: [{ title: "doc", seconds, project_id: projectId }],
      },
    ],
  };
}

describe("mergeDayViews", () => {
  it("sums app and title time and hours across days, collecting project days", () => {
    const merged = mergeDayViews(
      [day("2026-06-09", 100, 1, 9), day("2026-06-10", 50, 1, 10)],
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

    expect(app.titles).toHaveLength(1);
    expect(app.titles[0]).toMatchObject({ title: "doc", seconds: 150 });
    expect(app.titles[0].projects).toEqual([{ id: 1, dates: ["2026-06-09", "2026-06-10"] }]);
  });

  it("leaves projects empty when nothing is assigned", () => {
    const merged = mergeDayViews([day("2026-06-10", 60, null, 8)], "day", "2026-06-10");
    expect(merged.apps[0].projects).toEqual([]);
    expect(merged.apps[0].titles[0].projects).toEqual([]);
  });
});
