// Drag-and-drop payload for assigning an app (or a single window title) to a
// project. Encoded onto the native dataTransfer so a drop survives the round
// trip through WKWebView; parseDragPayload is the defensive decoder.

import type { DragEvent as ReactDragEvent } from "react";

// title "" = whole-app (app-level) drag; otherwise a single title row.
export type DragPayload = { appKey: string; title: string; dates: string[] };

export function startDrag(e: ReactDragEvent, payload: DragPayload) {
  const encoded = JSON.stringify(payload);
  e.dataTransfer.setData("application/x-echo-app", encoded);
  e.dataTransfer.setData("application/x-app", encoded);
  e.dataTransfer.setData("text/plain", payload.appKey);
  e.dataTransfer.effectAllowed = "link";
  e.stopPropagation();
}

export function parseDragPayload(dataTransfer: DataTransfer): DragPayload | null {
  for (const type of ["application/x-echo-app", "application/x-app"]) {
    const raw = dataTransfer.getData(type);
    if (!raw) continue;
    try {
      const parsed = JSON.parse(raw) as Partial<DragPayload>;
      if (typeof parsed.appKey === "string" && Array.isArray(parsed.dates)) {
        const dates = parsed.dates.filter((d): d is string => typeof d === "string");
        const title = typeof parsed.title === "string" ? parsed.title : "";
        if (dates.length > 0) return { appKey: parsed.appKey, title, dates };
      }
    } catch {
      return null;
    }
  }
  return null;
}
