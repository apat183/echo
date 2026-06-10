import { describe, expect, it } from "vitest";
import {
  type DragPayload,
  parseDragPayload,
  PROJECT_DRAG_MIME,
  isProjectDrag,
  parseProjectDragId,
  reorderIds,
} from "./drag";

function transfer(data: Record<string, string>): DataTransfer {
  return { getData: (type: string) => data[type] ?? "" } as unknown as DataTransfer;
}

describe("parseDragPayload", () => {
  it("decodes a valid payload from the echo mime type", () => {
    const payload: DragPayload = { appKey: "com.a", title: "doc", dates: ["2026-06-10"] };
    expect(parseDragPayload(transfer({ "application/x-echo-app": JSON.stringify(payload) }))).toEqual(
      payload
    );
  });

  it("falls back to the x-app mime type", () => {
    const payload: DragPayload = { appKey: "com.a", title: "", dates: ["2026-06-10"] };
    expect(parseDragPayload(transfer({ "application/x-app": JSON.stringify(payload) }))).toEqual(
      payload
    );
  });

  it("returns null when no payload is present", () => {
    expect(parseDragPayload(transfer({}))).toBeNull();
  });

  it("rejects a payload with no valid dates", () => {
    const t = transfer({
      "application/x-echo-app": JSON.stringify({ appKey: "com.a", title: "x", dates: [] }),
    });
    expect(parseDragPayload(t)).toBeNull();
  });

  it("filters out non-string dates and defaults a missing title to empty", () => {
    const t = transfer({
      "application/x-echo-app": JSON.stringify({ appKey: "com.a", dates: ["2026-06-10", 5, null] }),
    });
    expect(parseDragPayload(t)).toEqual({ appKey: "com.a", title: "", dates: ["2026-06-10"] });
  });
});

// ---- Project drag helpers --------------------------------------------------

describe("isProjectDrag", () => {
  it("returns true when the project mime type is present", () => {
    expect(isProjectDrag([PROJECT_DRAG_MIME])).toBe(true);
    expect(isProjectDrag(["text/plain", PROJECT_DRAG_MIME])).toBe(true);
  });

  it("returns false when the project mime type is absent", () => {
    expect(isProjectDrag([])).toBe(false);
    expect(isProjectDrag(["application/x-echo-app", "text/plain"])).toBe(false);
  });
});

describe("parseProjectDragId", () => {
  it("parses a valid integer project id", () => {
    expect(parseProjectDragId(transfer({ [PROJECT_DRAG_MIME]: "42" }))).toBe(42);
    expect(parseProjectDragId(transfer({ [PROJECT_DRAG_MIME]: "1" }))).toBe(1);
  });

  it("returns null when the mime type is missing", () => {
    expect(parseProjectDragId(transfer({}))).toBeNull();
  });

  it("returns null for garbage values", () => {
    expect(parseProjectDragId(transfer({ [PROJECT_DRAG_MIME]: "abc" }))).toBeNull();
    expect(parseProjectDragId(transfer({ [PROJECT_DRAG_MIME]: "" }))).toBeNull();
    expect(parseProjectDragId(transfer({ [PROJECT_DRAG_MIME]: "NaN" }))).toBeNull();
  });
});

describe("reorderIds", () => {
  it("moves an id forward in the list", () => {
    expect(reorderIds([1, 2, 3, 4], 1, 3)).toEqual([2, 3, 1, 4]);
  });

  it("moves an id backward in the list", () => {
    expect(reorderIds([1, 2, 3, 4], 3, 1)).toEqual([3, 1, 2, 4]);
  });

  it("moves an id upward (another case)", () => {
    expect(reorderIds([1, 2, 3, 4], 4, 2)).toEqual([1, 4, 2, 3]);
  });

  it("returns the original array unchanged when draggedId is absent", () => {
    const ids = [1, 2, 3];
    expect(reorderIds(ids, 99, 1)).toBe(ids);
  });

  it("returns the original array unchanged when targetId is absent", () => {
    const ids = [1, 2, 3];
    expect(reorderIds(ids, 1, 99)).toBe(ids);
  });

  it("returns the original array unchanged when dragged and target are the same", () => {
    const ids = [1, 2, 3];
    expect(reorderIds(ids, 2, 2)).toBe(ids);
  });
});
