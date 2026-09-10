import { invoke } from "@tauri-apps/api/core";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";

export type Project = { id: number; name: string; color: string };

// Mirrors `LinkState` in src-tauri/src/db.rs; keep the two in sync.
// "inherited" belongs to the project view (folded rows and per-day receipts);
// activity rows render inheritance by absence and never carry it.
export type LinkState = "direct" | "rule" | "inherited" | "excluded";

// Mirrors `RowProject` in src-tauri/src/db.rs; keep the two in sync.
export type RowProject = {
  project_id: number;
  state: LinkState;
  rule_id: number | null;
};

export type TitleUsage = {
  title: string; // "" = untitled
  seconds: number;
  projects: RowProject[]; // the title's OWN standing; empty = inherits app-level
};

export type AppUsage = {
  app_key: string;
  app_name: string;
  bundle_id: string | null;
  seconds: number;
  hours: number[]; // length 24
  projects: RowProject[]; // what the app-level row itself says
  titles: TitleUsage[];
};

export type DayView = {
  date: string; // YYYY-MM-DD
  total_seconds: number;
  apps: AppUsage[];
  hours: number[]; // length 24
};

export type DayTotal = { date: string; seconds: number };

export type IgnoredEntry = {
  app_key: string;
  app_name: string | null;
  title: string;
  created_at: number;
};

// Mirrors `TrackedApp` in src-tauri/src/db.rs; keep the two in sync.
export type TrackedApp = {
  app_key: string;
  app_name: string;
  bundle_id: string | null;
  seconds: number;
};

// Mirrors `TrackedTitle` in src-tauri/src/db.rs; keep the two in sync.
export type TrackedTitle = {
  title: string;
  seconds: number;
};

// Mirrors `AssignmentRule` in src-tauri/src/db.rs; keep the two in sync.
export type AssignmentRule = {
  id: number;
  project_id: number;
  app_key: string;
  app_name: string | null;
  title: string; // "" = the whole app; else a case-insensitive substring
  effective_from: string | null; // null = reaches all history
};

/** Does a rule's title pattern cover this title? The one definition of the
 *  match, mirroring `Resolver::title_rule_links` in src-tauri/src/db.rs —
 *  case-insensitive and partial, because titles are transient (ADR 0003). */
export function titleMatches(pattern: string, title: string): boolean {
  const needle = pattern.trim().toLowerCase();
  return needle !== "" && title.toLowerCase().includes(needle);
}

// Mirrors `ReceiptEntry` in src-tauri/src/db.rs; keep the two in sync.
export type ReceiptEntry = {
  app_key: string;
  app_name: string;
  bundle_id: string | null;
  title: string; // "" = untitled
  seconds: number;
  state: LinkState;
  rule_id: number | null;
};

export type ProjectTitle = {
  title: string; // "" = untitled
  seconds: number;
  state: LinkState;
  rule_id: number | null;
};

export type ProjectApp = {
  app_key: string;
  app_name: string;
  bundle_id: string | null;
  seconds: number;
  state: LinkState;
  rule_id: number | null;
  titles: ProjectTitle[];
};

export type ProjectPeriodNote = {
  granularity: "day" | "week" | "month";
  period_key: string;
  note: string;
};

// Mirrors `AutodeleteConfig` in src-tauri/src/db.rs; keep the two in sync.
export type AutodeleteConfig = { enabled: boolean; days: number };

// Mirrors `UpdateStatus` in src-tauri/src/updater.rs; keep the two in sync.
export type UpdateStatus =
  | { state: "idle" }
  | { state: "available"; version: string }
  | { state: "downloading"; version: string; downloaded: number; total: number | null }
  | { state: "installing"; version: string }
  | { state: "error"; message: string };

export const api = {
  getDayView: (date: string) => invoke<DayView>("get_day_view", { date }),
  listProjects: () => invoke<Project[]>("list_projects"),
  createProject: (name: string, color: string) =>
    invoke<Project>("create_project", { name, color }),
  deleteProject: (id: number) => invoke<void>("delete_project", { id }),
  addAssignment: (date: string, appKey: string, title: string, projectId: number) =>
    invoke<void>("add_assignment", { date, appKey, title, projectId }),
  removeAssignment: (date: string, appKey: string, title: string, projectId: number) =>
    invoke<void>("remove_assignment", { date, appKey, title, projectId }),
  /** Remove an app (or one of its titles) from a project across all days.
   *  Records exceptions for anything a rule or app-level assignment would
   *  still bill; resolves to how many days had to be excepted. */
  removeFromProject: (projectId: number, appKey: string, title: string | null) =>
    invoke<number>("remove_from_project", { projectId, appKey, title }),
  trackedApps: () => invoke<TrackedApp[]>("tracked_apps"),
  /** Every title one app has shown, busiest first — seeds a rule's pattern. */
  trackedTitles: (appKey: string) => invoke<TrackedTitle[]>("tracked_titles", { appKey }),
  listRules: () => invoke<AssignmentRule[]>("list_rules"),
  createRule: (
    projectId: number,
    appKey: string,
    appName: string | null,
    title: string,
    effectiveFrom: string | null,
  ) => invoke<AssignmentRule>("create_rule", { projectId, appKey, appName, title, effectiveFrom }),
  deleteRule: (id: number) => invoke<void>("delete_rule", { id }),
  /** Take a row out of a project for one day; records an Exception only if a
   *  rule or app-level assignment would otherwise put it straight back. */
  excludeForDay: (date: string, appKey: string, title: string, projectId: number) =>
    invoke<void>("exclude_for_day", { date, appKey, title, projectId }),
  /** Clear a recorded "no", returning the row to Undecided. */
  removeException: (date: string, appKey: string, title: string, projectId: number) =>
    invoke<void>("remove_exception", { date, appKey, title, projectId }),
  projectDayEntries: (projectId: number, date: string) =>
    invoke<ReceiptEntry[]>("project_day_entries", { projectId, date }),
  addIgnoredEntry: (appKey: string, appName: string | null, title: string) =>
    invoke<void>("add_ignored_entry", { appKey, appName, title }),
  listIgnoredEntries: () => invoke<IgnoredEntry[]>("list_ignored_entries"),
  removeIgnoredEntry: (appKey: string, title: string) =>
    invoke<void>("remove_ignored_entry", { appKey, title }),
  ignoredBreakdown: () => invoke<DayTotal[]>("ignored_breakdown"),
  projectBreakdown: (projectId: number) =>
    invoke<DayTotal[]>("project_breakdown", { projectId }),
  projectApps: (projectId: number) =>
    invoke<ProjectApp[]>("project_apps", { projectId }),
  listProjectPeriodNotes: (projectId: number) =>
    invoke<ProjectPeriodNote[]>("list_project_period_notes", { projectId }),
  setProjectPeriodNote: (
    projectId: number,
    granularity: ProjectPeriodNote["granularity"],
    periodKey: string,
    note: string,
  ) =>
    invoke<void>("set_project_period_note", { projectId, granularity, periodKey, note }),
  setProjectOrder: (ids: number[]) => invoke<void>("set_project_order", { ids }),
  appIcon: (bundleId: string | null) =>
    bundleId
      ? invoke<string | null>("app_icon_data_url", { bundleId })
      : Promise.resolve(null),
  axStatus: () => invoke<boolean>("ax_status"),
  axRequest: () => invoke<boolean>("ax_request"),
  axOpenSettings: () => invoke<void>("ax_open_settings"),
  appVersion: () => invoke<string>("app_version"),
  openExternal: (url: string) => invoke<void>("open_external", { url }),
  storageSize: () => invoke<number>("storage_size"),
  clearTrackingData: () => invoke<void>("clear_tracking_data"),
  clearUntagged: () => invoke<number>("clear_untagged"),
  resetEverything: () => invoke<void>("reset_everything"),
  getAutodeleteConfig: () => invoke<AutodeleteConfig>("get_autodelete_config"),
  setAutodeleteConfig: (enabled: boolean, days: number) =>
    invoke<void>("set_autodelete_config", { enabled, days }),
  // Launch-at-login, via tauri-plugin-autostart (its JS API wraps invoke itself).
  autostartIsEnabled: () => isEnabled(),
  autostartEnable: () => enable(),
  autostartDisable: () => disable(),
  updateStatus: () => invoke<UpdateStatus>("update_status"),
  installUpdate: () => invoke<void>("install_update"),
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

/** Human-readable byte size, e.g. "0 B", "12.0 KB", "1.2 MB". */
export function fmtBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let n = bytes / 1024;
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n.toFixed(1)} ${units[i]}`;
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
