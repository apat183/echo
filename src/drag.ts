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

// ---- Project reorder drag helpers -----------------------------------------

export const PROJECT_DRAG_MIME = "application/x-echo-project";

/** Starts a project reorder drag; encodes the project id onto the transfer. */
export function startProjectDrag(e: ReactDragEvent, projectId: number): void {
  e.dataTransfer.setData(PROJECT_DRAG_MIME, String(projectId));
  e.dataTransfer.effectAllowed = "move";
}

/**
 * Returns true when the drag transfer carries a project drag payload.
 * Use this in dragover handlers where getData() is unavailable in WKWebView.
 */
export function isProjectDrag(types: readonly string[]): boolean {
  return types.includes(PROJECT_DRAG_MIME);
}

/**
 * Parses the project id from a drop's DataTransfer.
 * Returns null when the mime type is absent or the value is not a valid integer.
 */
export function parseProjectDragId(dt: DataTransfer): number | null {
  const raw = dt.getData(PROJECT_DRAG_MIME);
  if (!raw) return null;
  const id = parseInt(raw, 10);
  return isNaN(id) ? null : id;
}

/**
 * Pure reorder: removes draggedId from ids, then inserts it directly after
 * targetId. Returns ids unchanged (same reference) if either id is absent or
 * both ids are the same.
 */
export function reorderIds(ids: number[], draggedId: number, targetId: number): number[] {
  if (draggedId === targetId) return ids;
  const fromIdx = ids.indexOf(draggedId);
  if (fromIdx === -1 || ids.indexOf(targetId) === -1) return ids;
  const result = [...ids];
  result.splice(fromIdx, 1);
  const newTargetIdx = result.indexOf(targetId);
  result.splice(newTargetIdx + 1, 0, draggedId);
  return result;
}
