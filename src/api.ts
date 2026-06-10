import { invoke } from "@tauri-apps/api/core";

export type Project = { id: number; name: string; color: string };

export type TitleUsage = {
  title: string; // "" = untitled
  seconds: number;
  project_id: number | null; // explicit (app,title) link; null = inherits app-level
};

export type AppUsage = {
  app_key: string;
  app_name: string;
  bundle_id: string | null;
  seconds: number;
  hours: number[]; // length 24
  project_id: number | null; // app-level (title="") link
  titles: TitleUsage[];
};

export type DayView = {
  date: string; // YYYY-MM-DD
  total_seconds: number;
  apps: AppUsage[];
  hours: number[]; // length 24
};

export type DayTotal = { date: string; seconds: number };

export type ProjectTitle = {
  title: string; // "" = untitled
  seconds: number;
  note: string | null;
};

export type ProjectApp = {
  app_key: string;
  app_name: string;
  bundle_id: string | null;
  seconds: number;
  note: string | null; // app-level note
  titles: ProjectTitle[];
};

export const api = {
  getDayView: (date: string) => invoke<DayView>("get_day_view", { date }),
  listProjects: () => invoke<Project[]>("list_projects"),
  createProject: (name: string, color: string) =>
    invoke<Project>("create_project", { name, color }),
  deleteProject: (id: number) => invoke<void>("delete_project", { id }),
  setAssignment: (date: string, appKey: string, title: string, projectId: number | null) =>
    invoke<void>("set_assignment", { date, appKey, title, projectId }),
  projectBreakdown: (projectId: number) =>
    invoke<DayTotal[]>("project_breakdown", { projectId }),
  projectApps: (projectId: number) =>
    invoke<ProjectApp[]>("project_apps", { projectId }),
  setNote: (projectId: number, appKey: string, title: string, note: string) =>
    invoke<void>("set_note", { projectId, appKey, title, note }),
  appIcon: (bundleId: string | null) =>
    bundleId
      ? invoke<string | null>("app_icon_data_url", { bundleId })
      : Promise.resolve(null),
  axStatus: () => invoke<boolean>("ax_status"),
  axRequest: () => invoke<boolean>("ax_request"),
};

// ---- helpers --------------------------------------------------------------

export const PROJECT_COLORS = [
  "#34c759", "#ff9500", "#ffcc00", "#5ac8fa", "#007aff",
  "#af52de", "#ff2d55", "#ff3b30", "#8e8e93", "#30b0c7",
];

/** Local YYYY-MM-DD for a Date. */
export function toDateStr(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

export function addDays(dateStr: string, n: number): string {
  const [y, m, d] = dateStr.split("-").map(Number);
  const dt = new Date(y, m - 1, d + n);
  return toDateStr(dt);
}

export function fmtDur(seconds: number): string {
  const m = Math.round(seconds / 60);
  if (m < 1) return "<1m";
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  const rem = m % 60;
  return rem ? `${h}h ${rem}m` : `${h}h`;
}

/** Monday-of-week ISO label, e.g. "Week of Mon 9 Jun". */
export function weekKey(dateStr: string): string {
  const [y, m, d] = dateStr.split("-").map(Number);
  const dt = new Date(y, m - 1, d);
  const dow = (dt.getDay() + 6) % 7; // Mon=0
  dt.setDate(dt.getDate() - dow);
  return toDateStr(dt);
}

export function monthKey(dateStr: string): string {
  return dateStr.slice(0, 7); // YYYY-MM
}

/** A short, stable color for an app with no project, from its key. */
export function appColor(key: string): string {
  let h = 0;
  for (let i = 0; i < key.length; i++) h = (h * 31 + key.charCodeAt(i)) >>> 0;
  return `hsl(${h % 360} 45% 60%)`;
}

export function initials(name: string): string {
  const clean = name.replace(/[^A-Za-z0-9 ]/g, "").trim();
  const parts = clean.split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "?";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[1][0]).toUpperCase();
}
