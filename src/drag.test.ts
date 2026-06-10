import { describe, expect, it } from "vitest";
import { type DragPayload, parseDragPayload } from "./drag";

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
