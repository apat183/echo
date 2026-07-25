// Project view: a project's time auto-grouped by day/week/month, with period
// notes and an app/title breakdown. Read-only for time; assignment happens by
// dragging in the day pane.

import { useCallback, useEffect, useMemo, useState } from "react";
import {
  api,
  appColor,
  addDays,
  type DayTotal,
  fmtDur,
  initials,
  monthKey,
  type LinkState,
  type Project,
  type ProjectApp,
  type ProjectPeriodNote,
  type ReceiptEntry,
  weekKey,
} from "../api";
import { bucketLabel, rollup, type Bucket, type Granularity } from "../period";
import { loadAppIcon } from "../appIcon";
import { Calendar, CalendarDays, CalendarRange, X } from "lucide-react";
import { Segmented } from "./Segmented";

export function ProjectPane(props: { project?: Project; onAssignmentChange: () => void }) {
  const { project, onAssignmentChange } = props;
  const [rows, setRows] = useState<DayTotal[]>([]);
  const [apps, setApps] = useState<ProjectApp[]>([]);
  const [notes, setNotes] = useState<ProjectPeriodNote[]>([]);
  const [gran, setGran] = useState<Granularity>("week");

  const projectId = project?.id;
  const load = useCallback(async () => {
    if (projectId == null) return;
    const [nextRows, nextApps, nextNotes] = await Promise.all([
      api.projectBreakdown(projectId),
      api.projectApps(projectId),
      api.listProjectPeriodNotes(projectId),
    ]);
    setRows(nextRows);
    setApps(nextApps);
    setNotes(nextNotes);
  }, [projectId]);

  useEffect(() => {
    load();
  }, [load]);

  const buckets = useMemo(() => rollup(rows, gran), [rows, gran]);
  const total = useMemo(() => rows.reduce((s, r) => s + r.seconds, 0), [rows]);
  const appsMax = apps.length > 0 ? apps[0].seconds : 1;
  const noteByKey = useMemo(() => {
    const map = new Map<string, string>();
    for (const note of notes) {
      map.set(noteKey(note.granularity, note.period_key), note.note);
    }
    return map;
  }, [notes]);

  async function savePeriodNote(noteGran: Granularity, key: string, text: string) {
    if (projectId == null) return;
    await api.setProjectPeriodNote(projectId, noteGran, key, text);
    await api.listProjectPeriodNotes(projectId).then(setNotes);
  }

  async function removeProjectApp(app: ProjectApp) {
    if (projectId == null) return;
    if (!window.confirm(removalPrompt(app.app_name, app.state))) return;
    await api.removeFromProject(projectId, app.app_key, null);
    await load();
    onAssignmentChange();
  }

  async function removeProjectTitle(app: ProjectApp, title: string, state: LinkState) {
    if (projectId == null) return;
    if (!window.confirm(removalPrompt(title, state))) return;
    await api.removeFromProject(projectId, app.app_key, title);
    await load();
    onAssignmentChange();
  }

  if (!project) return <div className="pane-body" />;

  return (
    <>
      <header className="pane-header" data-tauri-drag-region="">
        <div className="project-title">
          <span className="nav-dot" style={{ background: project.color }} />
          {project.name}
        </div>
        <div className="spacer" />
        <Segmented
          value={gran}
          options={[
            { value: "day" as Granularity, label: "Day", icon: <Calendar size={13} /> },
            { value: "week" as Granularity, label: "Week", icon: <CalendarRange size={13} /> },
            { value: "month" as Granularity, label: "Month", icon: <CalendarDays size={13} /> },
          ]}
          onChange={setGran}
        />
      </header>

      <div className="pane-body">
        <h1 className="usage-total">{fmtDur(total)}</h1>
        <p className="usage-sub">
          {buckets.length} {gran}
          {buckets.length === 1 ? "" : "s"} · {apps.length} app{apps.length === 1 ? "" : "s"}
        </p>

        {apps.length > 0 && (
          <div className="project-apps">
            <div className="section-label">Apps</div>
            {apps.map((a) => (
              <ProjectAppRow
                key={a.app_key}
                app={a}
                color={project.color}
                max={appsMax}
                onRemoveApp={removeProjectApp}
                onRemoveTitle={(title, state) => removeProjectTitle(a, title, state)}
              />
            ))}
          </div>
        )}

        {buckets.length > 0 && <div className="section-label">By {gran}</div>}
        <div className="bucket-list">
          {buckets.length === 0 && (
            <p className="empty">
              Nothing assigned yet. Open Activities and drag this project's apps onto it.
            </p>
          )}
          {buckets.map((b) => (
            <PeriodBucketRow
              key={b.key}
              bucket={b}
              gran={gran}
              rows={rows}
              maxSeconds={buckets[0].seconds}
              color={project.color}
              projectId={project.id}
              onAssignmentChange={async () => {
                await load();
                onAssignmentChange();
              }}
              noteByKey={noteByKey}
              onSaveNote={savePeriodNote}
            />
          ))}
        </div>
      </div>
    </>
  );
}

function noteKey(gran: Granularity, key: string): string {
  return `${gran}:${key}`;
}

function noteFor(map: Map<string, string>, gran: Granularity, key: string): string | null {
  return map.get(noteKey(gran, key)) ?? null;
}

/** One editable note attached to a day/week/month bucket. Empty saves clear it. */
function PeriodNote(props: {
  note: string | null;
  placeholder: string;
  onSave: (text: string) => void;
}) {
  const { note, onSave } = props;
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(note ?? "");

  useEffect(() => {
    setDraft(note ?? "");
  }, [note]);

  if (editing) {
    return (
      <textarea
        className="period-note-input"
        autoFocus
        rows={2}
        value={draft}
        placeholder={props.placeholder}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => {
          setEditing(false);
          if (draft.trim() !== (note ?? "")) onSave(draft.trim());
        }}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            setDraft(note ?? "");
            setEditing(false);
          } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            (e.target as HTMLTextAreaElement).blur();
          }
        }}
      />
    );
  }
  if (note) {
    return (
      <div className="period-note" title="Click to edit" onClick={() => setEditing(true)}>
        {note}
      </div>
    );
  }
  return (
    <button type="button" className="period-note-add" onClick={() => setEditing(true)}>
      + note
    </button>
  );
}

function PeriodBucketRow(props: {
  bucket: Bucket;
  gran: Granularity;
  rows: DayTotal[];
  maxSeconds: number;
  color: string;
  projectId: number;
  noteByKey: Map<string, string>;
  onSaveNote: (gran: Granularity, key: string, text: string) => void;
  onAssignmentChange: () => void;
}) {
  const { bucket, gran, rows, maxSeconds, color, noteByKey, onSaveNote } = props;
  const [expanded, setExpanded] = useState(false);
  const children = useMemo(
    () => childNoteRows(bucket.key, gran, rows, noteByKey),
    [bucket.key, gran, rows, noteByKey]
  );
  // A day bucket has no child periods, but it can still be opened — that is
  // where its receipt lives.
  const isDay = gran === "day";
  const canExpand = children.length > 0 || isDay;

  return (
    <div className="period-bucket">
      <div className="bucket-row">
        <button
          type="button"
          className={`disclosure ${canExpand ? "" : "empty"}`}
          tabIndex={canExpand ? 0 : -1}
          aria-label={expanded ? `Collapse ${bucket.label}` : `Expand ${bucket.label}`}
          onClick={() => canExpand && setExpanded((v) => !v)}
        >
          {canExpand ? (expanded ? "▾" : "▸") : ""}
        </button>
        <span className="bucket-label">{bucket.label}</span>
        <span className="bucket-bar-wrap">
          <span
            className="bucket-bar"
            style={{
              width: `${(bucket.seconds / Math.max(1, maxSeconds)) * 100}%`,
              background: color,
            }}
          />
        </span>
        <span className="bucket-time">{fmtDur(bucket.seconds)}</span>
      </div>

      <div className="period-note-wrap">
        <PeriodNote
          note={noteFor(noteByKey, gran, bucket.key)}
          placeholder={`Add ${gran} note...`}
          onSave={(text) => onSaveNote(gran, bucket.key, text)}
        />
      </div>

      {expanded && isDay && (
        <DayReceipt
          projectId={props.projectId}
          date={bucket.key}
          onChanged={props.onAssignmentChange}
        />
      )}

      {expanded && children.length > 0 && (
        <div className="period-child-list">
          {children.map((child) => (
            <ChildPeriodRow
              key={`${child.gran}:${child.key}`}
              child={child}
              projectId={props.projectId}
              noteByKey={noteByKey}
              onSaveNote={onSaveNote}
              onAssignmentChange={props.onAssignmentChange}
            />
          ))}
        </div>
      )}
    </div>
  );
}

/** A day (or week) inside an expanded bucket: its note, plus its own receipt. */
function ChildPeriodRow(props: {
  child: { gran: Granularity; key: string; label: string; seconds: number };
  projectId: number;
  noteByKey: Map<string, string>;
  onSaveNote: (gran: Granularity, key: string, text: string) => void;
  onAssignmentChange: () => void;
}) {
  const { child, projectId, noteByKey, onSaveNote, onAssignmentChange } = props;
  const [open, setOpen] = useState(false);
  const isDay = child.gran === "day";

  return (
    <div className="period-child-block">
      <div className="period-child-row">
        <button
          type="button"
          className={`disclosure ${isDay ? "" : "empty"}`}
          tabIndex={isDay ? 0 : -1}
          aria-label={open ? `Collapse ${child.label}` : `Expand ${child.label}`}
          onClick={() => isDay && setOpen((v) => !v)}
        >
          {isDay ? (open ? "▾" : "▸") : ""}
        </button>
        <span className="period-child-label">{child.label}</span>
        <span className="period-child-time">
          {child.seconds > 0 ? fmtDur(child.seconds) : ""}
        </span>
        <div className="period-child-note">
          <PeriodNote
            note={noteFor(noteByKey, child.gran, child.key)}
            placeholder={`Add ${child.gran} note...`}
            onSave={(text) => onSaveNote(child.gran, child.key, text)}
          />
        </div>
      </div>
      {open && isDay && (
        <DayReceipt projectId={projectId} date={child.key} onChanged={onAssignmentChange} />
      )}
    </div>
  );
}

function childNoteRows(
  parentKey: string,
  parentGran: Granularity,
  rows: DayTotal[],
  noteByKey: Map<string, string>,
): Array<{ gran: Granularity; key: string; label: string; seconds: number }> {
  if (parentGran === "day") return [];

  const childGran: Granularity = parentGran === "week" ? "day" : "week";
  const secondsByKey = new Map<string, number>();
  for (const row of rows) {
    if (parentGran === "week" && weekKey(row.date) === parentKey) {
      secondsByKey.set(row.date, (secondsByKey.get(row.date) ?? 0) + row.seconds);
    }
    if (parentGran === "month" && monthKey(row.date) === parentKey) {
      const key = weekKey(row.date);
      secondsByKey.set(key, (secondsByKey.get(key) ?? 0) + row.seconds);
    }
  }

  for (const key of noteByKey.keys()) {
    const [gran, periodKey] = key.split(":", 2) as [Granularity, string];
    if (gran !== childGran) continue;
    if (parentGran === "week" && weekKey(periodKey) === parentKey) {
      secondsByKey.set(periodKey, secondsByKey.get(periodKey) ?? 0);
    }
    if (parentGran === "month" && weekOverlapsMonth(periodKey, parentKey)) {
      secondsByKey.set(periodKey, secondsByKey.get(periodKey) ?? 0);
    }
  }

  return [...secondsByKey.entries()]
    .sort((a, b) => (a[0] < b[0] ? 1 : -1))
    .map(([key, seconds]) => ({
      gran: childGran,
      key,
      label: bucketLabel(key, childGran),
      seconds,
    }));
}

function weekOverlapsMonth(weekStart: string, month: string): boolean {
  for (let i = 0; i < 7; i++) {
    if (monthKey(addDays(weekStart, i)) === month) return true;
  }
  return false;
}

function ProjectAppRow(props: {
  app: ProjectApp;
  color: string;
  max: number;
  onRemoveApp: (app: ProjectApp) => void;
  onRemoveTitle: (title: string, state: LinkState) => void;
}) {
  const { app, color, max, onRemoveApp, onRemoveTitle } = props;
  const [icon, setIcon] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    let alive = true;
    loadAppIcon(app.bundle_id)
      .then((src) => alive && setIcon(src))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [app.bundle_id]);

  const canExpand = app.titles.some((t) => t.title.trim() !== "");

  return (
    <div className="project-app-group">
      <div className="project-app-row">
        <button
          type="button"
          className={`disclosure ${canExpand ? "" : "empty"}`}
          tabIndex={canExpand ? 0 : -1}
          aria-label={expanded ? "Collapse" : "Expand"}
          onClick={() => canExpand && setExpanded((v) => !v)}
        >
          {canExpand ? (expanded ? "▾" : "▸") : ""}
        </button>
        <span
          className={`app-badge ${icon ? "with-icon" : ""}`}
          style={{ background: appColor(app.app_key) }}
        >
          {icon ? (
            <img src={icon} alt="" draggable={false} onError={() => setIcon(null)} />
          ) : (
            initials(app.app_name)
          )}
        </span>
        <span className="project-app-name">{app.app_name}</span>
        <span className="bucket-bar-wrap">
          <span
            className="bucket-bar"
            style={{ width: `${(app.seconds / Math.max(1, max)) * 100}%`, background: color }}
          />
        </span>
        <span className="bucket-time">{fmtDur(app.seconds)}</span>
        {app.state === "rule" && (
          <span className="project-row-rule" title="A rule puts this app in the project">
            rule
          </span>
        )}
        <button
          type="button"
          className="icon-btn project-row-remove"
          title={`Remove ${app.app_name} from project`}
          onClick={() => onRemoveApp(app)}
        >
          <X size={14} />
        </button>
      </div>

      {expanded && (
        <div className="project-title-list">
          {app.titles.map((t) => {
            const isUntitled = t.title.trim() === "";
            return (
              <div className="project-title-block" key={t.title || "(untitled)"}>
                <div className="project-title-row">
                  <span className="title-name">{isUntitled ? "Untitled" : t.title}</span>
                  <span className="title-time">{fmtDur(t.seconds)}</span>
                  {t.state === "rule" && (
                    <span className="project-row-rule" title="A rule puts this in the project">
                      rule
                    </span>
                  )}
                  {/* The untitled row *is* the app-level row, so removing it is
                      the app's own control above rather than a second one here.
                      Everything else is removable whatever put it there. */}
                  {!isUntitled && (
                    <button
                      type="button"
                      className="icon-btn project-row-remove"
                      title="Remove title from project"
                      onClick={() => onRemoveTitle(t.title, t.state)}
                    >
                      <X size={13} />
                    </button>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

/**
 * The receipt behind one day's total: what made it up, and why each line is
 * here. After rules, a project can hold time nobody linked by hand, so a day
 * total needs somewhere to be interrogated. Removing a line takes it out of the
 * project for that day only — recording an exception if a rule or an app-level
 * assignment would otherwise put it straight back.
 */
function DayReceipt(props: { projectId: number; date: string; onChanged: () => void }) {
  const { projectId, date, onChanged } = props;
  const [entries, setEntries] = useState<ReceiptEntry[] | null>(null);

  const load = useCallback(() => {
    api
      .projectDayEntries(projectId, date)
      .then(setEntries)
      .catch(() => setEntries([]));
  }, [projectId, date]);

  useEffect(load, [load]);

  async function exclude(entry: ReceiptEntry) {
    const what = entry.title || entry.app_name;
    if (!window.confirm(`Remove ${what} from this project on ${date}?`)) return;
    await api.excludeForDay(date, entry.app_key, entry.title, projectId);
    load();
    onChanged();
  }

  if (entries == null) return <div className="receipt-list empty">Loading…</div>;
  if (entries.length === 0) {
    return <div className="receipt-list empty">Nothing in this project on {date}.</div>;
  }

  return (
    <div className="receipt-list">
      {entries.map((e) => (
        <div className="receipt-row" key={`${e.app_key}\u0000${e.title}`}>
          <span className="receipt-app">{e.app_name}</span>
          <span className="receipt-title">{e.title || "Untitled"}</span>
          <span className="receipt-why" title={receiptReason(e)}>
            {e.state === "direct" ? "assigned" : e.state === "rule" ? "rule" : "inherited"}
          </span>
          <span className="receipt-time">{fmtDur(e.seconds)}</span>
          <button
            type="button"
            className="icon-btn project-row-remove"
            title={`Remove from this project on ${date}`}
            onClick={() => exclude(e)}
          >
            <X size={13} />
          </button>
        </div>
      ))}
    </div>
  );
}

function receiptReason(entry: ReceiptEntry): string {
  switch (entry.state) {
    case "direct":
      return "You assigned this on this day.";
    case "rule":
      return "A standing rule puts this in the project.";
    default:
      return "Covered by an app-level assignment on this day.";
  }
}

/** Removal is permanent and has no undo, so the prompt says what will actually
 *  happen — a rule-covered row is excluded day by day and the rule itself stays
 *  put, which is a different thing from deleting the rule. */
function removalPrompt(what: string, state: LinkState): string {
  if (state === "rule") {
    return (
      `Remove ${what} from this project?\n\n` +
      `A rule puts it here, so each affected day is excluded individually and ` +
      `the rule itself stays. To stop it for good, delete the rule under Rules.`
    );
  }
  return `Remove ${what} from this project?`;
}
