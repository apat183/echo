// Project view: a project's time auto-grouped by day/week/month, its app/title
// breakdown, and per-entry notes. Read-only for time; assignment happens by
// dragging in the day pane.

import { useCallback, useEffect, useMemo, useState } from "react";
import {
  api,
  appColor,
  type DayTotal,
  fmtDur,
  initials,
  type Project,
  type ProjectApp,
} from "../api";
import { rollup, type Granularity } from "../period";
import { loadAppIcon } from "../appIcon";
import { Calendar, CalendarDays, CalendarRange } from "lucide-react";
import { Segmented } from "./Segmented";

export function ProjectPane(props: { project?: Project }) {
  const { project } = props;
  const [rows, setRows] = useState<DayTotal[]>([]);
  const [apps, setApps] = useState<ProjectApp[]>([]);
  const [gran, setGran] = useState<Granularity>("week");

  const projectId = project?.id;
  const load = useCallback(() => {
    if (projectId == null) return;
    api.projectBreakdown(projectId).then(setRows);
    api.projectApps(projectId).then(setApps);
  }, [projectId]);

  useEffect(() => {
    load();
  }, [load]);

  const buckets = useMemo(() => rollup(rows, gran), [rows, gran]);
  const total = useMemo(() => rows.reduce((s, r) => s + r.seconds, 0), [rows]);
  const appsMax = apps.length > 0 ? apps[0].seconds : 1;

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
                projectId={project.id}
                onChanged={load}
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
            <div className="bucket-row" key={b.key}>
              <span className="bucket-label">{b.label}</span>
              <span className="bucket-bar-wrap">
                <span
                  className="bucket-bar"
                  style={{
                    width: `${(b.seconds / Math.max(1, buckets[0].seconds)) * 100}%`,
                    background: project.color,
                  }}
                />
              </span>
              <span className="bucket-time">{fmtDur(b.seconds)}</span>
            </div>
          ))}
        </div>
      </div>
    </>
  );
}

/** One editable note on a project entry (app or title). Empty saves clear it. */
function EntryNote(props: { note: string | null; onSave: (text: string) => void }) {
  const { note, onSave } = props;
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(note ?? "");

  useEffect(() => {
    setDraft(note ?? "");
  }, [note]);

  if (editing) {
    return (
      <textarea
        className="entry-note-input"
        autoFocus
        rows={2}
        value={draft}
        placeholder="Add a note…"
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
      <div className="entry-note" title="Click to edit" onClick={() => setEditing(true)}>
        {note}
      </div>
    );
  }
  return (
    <button type="button" className="entry-note-add" onClick={() => setEditing(true)}>
      + note
    </button>
  );
}

function ProjectAppRow(props: {
  app: ProjectApp;
  color: string;
  max: number;
  projectId: number;
  onChanged: () => void;
}) {
  const { app, color, max, projectId, onChanged } = props;
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

  async function saveNote(title: string, text: string) {
    await api.setNote(projectId, app.app_key, title, text);
    onChanged();
  }

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
      </div>

      <div className="entry-note-wrap">
        <EntryNote note={app.note} onSave={(text) => saveNote("", text)} />
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
                </div>
                {!isUntitled && (
                  <div className="entry-note-wrap title">
                    <EntryNote note={t.note} onSave={(text) => saveNote(t.title, text)} />
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
