import {
  type DragEvent as ReactDragEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  api,
  addDays,
  appColor,
  type AppUsage,
  type DayTotal,
  type DayView,
  fmtDur,
  initials,
  monthKey,
  type Project,
  type ProjectApp,
  PROJECT_COLORS,
  toDateStr,
  weekKey,
} from "./api";
import "./App.css";

type Selection = { kind: "day" } | { kind: "project"; id: number };
// title "" = whole-app (app-level) drag; otherwise a single title row.
type DragPayload = { appKey: string; title: string; dates: string[] };
type Granularity = "day" | "week" | "month";

export default function App() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [selection, setSelection] = useState<Selection>({ kind: "day" });
  const [dragPayload, setDragPayload] = useState<DragPayload | null>(null);
  const [assignmentVersion, setAssignmentVersion] = useState(0);
  const dragPayloadRef = useRef<DragPayload | null>(null);

  const refreshProjects = useCallback(async () => {
    setProjects(await api.listProjects());
  }, []);

  const handleDragApp = useCallback((payload: DragPayload | null) => {
    dragPayloadRef.current = payload;
    setDragPayload(payload);
  }, []);

  const assignAppToProject = useCallback(
    async (payload: DragPayload, projectId: number) => {
      const dates = [...new Set(payload.dates)];
      if (dates.length === 0) return;
      await Promise.all(
        dates.map((d) => api.setAssignment(d, payload.appKey, payload.title, projectId))
      );
      setAssignmentVersion((v) => v + 1);
      await refreshProjects();
    },
    [refreshProjects]
  );

  useEffect(() => {
    refreshProjects();
  }, [refreshProjects]);

  return (
    <div className="app">
      <Sidebar
        projects={projects}
        selection={selection}
        onSelect={setSelection}
        onProjectsChanged={refreshProjects}
        dragPayload={dragPayload}
        getDragPayload={() => dragPayloadRef.current}
        onAssignApp={assignAppToProject}
      />
      <main className="main">
        {selection.kind === "day" ? (
          <DayPane
            projects={projects}
            assignmentVersion={assignmentVersion}
            onDragApp={handleDragApp}
            onAssignmentChange={refreshProjects}
          />
        ) : (
          <ProjectPane
            project={projects.find((p) => p.id === selection.id)}
            onDeleted={() => {
              setSelection({ kind: "day" });
              refreshProjects();
            }}
          />
        )}
      </main>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------

function Sidebar(props: {
  projects: Project[];
  selection: Selection;
  onSelect: (s: Selection) => void;
  onProjectsChanged: () => void;
  dragPayload: DragPayload | null;
  getDragPayload: () => DragPayload | null;
  onAssignApp: (payload: DragPayload, projectId: number) => Promise<void>;
}) {
  const {
    projects,
    selection,
    onSelect,
    onProjectsChanged,
    dragPayload,
    getDragPayload,
    onAssignApp,
  } = props;
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState("");

  async function addProject() {
    const trimmed = name.trim();
    if (!trimmed) return;
    const color = PROJECT_COLORS[projects.length % PROJECT_COLORS.length];
    await api.createProject(trimmed, color);
    setName("");
    setAdding(false);
    onProjectsChanged();
  }

  return (
    <aside className="sidebar">
      <div className="sidebar-brand">TimeTracker</div>

      <button
        className={`nav-item ${selection.kind === "day" ? "active" : ""}`}
        onClick={() => onSelect({ kind: "day" })}
      >
        <span className="nav-dot activities" /> Activities
      </button>

      <div className="sidebar-section">
        <span>Projects</span>
        <button className="icon-btn" title="New project" onClick={() => setAdding((a) => !a)}>
          +
        </button>
      </div>

      {adding && (
        <div className="add-project">
          <input
            autoFocus
            value={name}
            placeholder="Project name…"
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") addProject();
              if (e.key === "Escape") setAdding(false);
            }}
          />
        </div>
      )}

      {projects.length === 0 && !adding && (
        <p className="sidebar-empty">No projects yet. Click + to add one, then drag apps onto it.</p>
      )}

      {projects.map((p) => (
        <ProjectNavItem
          key={p.id}
          project={p}
          active={selection.kind === "project" && selection.id === p.id}
          dragPayload={dragPayload}
          getDragPayload={getDragPayload}
          onAssignApp={onAssignApp}
          onSelect={() => onSelect({ kind: "project", id: p.id })}
        />
      ))}
    </aside>
  );
}

/** A project row that is also a drop target for dragged apps. */
function ProjectNavItem(props: {
  project: Project;
  active: boolean;
  dragPayload: DragPayload | null;
  getDragPayload: () => DragPayload | null;
  onAssignApp: (payload: DragPayload, projectId: number) => Promise<void>;
  onSelect: () => void;
}) {
  const { project, active, dragPayload, getDragPayload, onAssignApp, onSelect } = props;
  const [over, setOver] = useState(false);
  const suppressClick = useRef(false);

  function selectProject() {
    if (suppressClick.current) return;
    onSelect();
  }

  return (
    <div
      role="button"
      tabIndex={0}
      className={`nav-item project ${active ? "active" : ""} ${over ? "drop-over" : ""}`}
      onClick={selectProject}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          selectProject();
        }
      }}
      onDragEnter={(e) => {
        e.preventDefault();
        setOver(true);
      }}
      onDragOver={(e) => {
        e.preventDefault();
        e.dataTransfer.dropEffect = "link";
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        e.preventDefault();
        e.stopPropagation();
        suppressClick.current = true;
        window.setTimeout(() => {
          suppressClick.current = false;
        }, 250);
        setOver(false);
        // The live ref is authoritative: it's set synchronously by the dragged
        // element's onDragStart, so it can't be a stale React render or a
        // WKWebView that dropped the custom dataTransfer payload.
        const payload = getDragPayload() ?? parseDragPayload(e.dataTransfer) ?? dragPayload;
        if (!payload) return;
        void onAssignApp(payload, project.id).catch((err) => {
          console.error("Failed to assign app to project", err);
        });
      }}
    >
      <span className="nav-dot" style={{ background: project.color }} />
      <span className="project-name">{project.name}</span>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Day pane (Screen Time-style)
// ---------------------------------------------------------------------------

type RowProject = { id: number; dates: string[] };

type PeriodTitleUsage = {
  title: string;
  seconds: number;
  dates: string[];
  projects: RowProject[]; // explicit title-level links across the period
};

type PeriodAppUsage = Omit<AppUsage, "project_id" | "titles"> & {
  dates: string[];
  projects: RowProject[]; // app-level (title="") links across the period
  titles: PeriodTitleUsage[];
};

type PeriodView = {
  label: string;
  total_seconds: number;
  apps: PeriodAppUsage[];
  hours: number[];
};

function DayPane(props: {
  projects: Project[];
  assignmentVersion: number;
  onDragApp: (payload: DragPayload | null) => void;
  onAssignmentChange: () => void;
}) {
  const { projects, assignmentVersion, onAssignmentChange } = props;
  const [date, setDate] = useState(() => toDateStr(new Date()));
  const [gran, setGran] = useState<Granularity>("day");
  const [view, setView] = useState<PeriodView | null>(null);
  const [axOk, setAxOk] = useState(true); // assume ok until checked, to avoid a flash

  // Poll Accessibility status so the banner disappears once granted.
  useEffect(() => {
    let alive = true;
    const check = () => api.axStatus().then((ok) => alive && setAxOk(ok)).catch(() => {});
    check();
    const id = setInterval(check, 3000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  const load = useCallback(async () => {
    const dates = periodDates(date, gran);
    const days = await Promise.all(dates.map((d) => api.getDayView(d)));
    setView(mergeDayViews(days, gran, date));
  }, [date, gran]);

  useEffect(() => {
    load();
    const id = setInterval(load, 4000); // keep today live as segments close
    return () => clearInterval(id);
  }, [load]);

  useEffect(() => {
    if (assignmentVersion > 0) load();
  }, [assignmentVersion, load]);

  const projectById = useMemo(
    () => new Map(projects.map((p) => [p.id, p])),
    [projects]
  );

  const isCurrentPeriod = samePeriod(date, toDateStr(new Date()), gran);
  const label = periodLabel(date, gran);
  const currentLabel = gran === "day" ? "Today" : gran === "week" ? "This Week" : "This Month";

  return (
    <>
      <header className="pane-header">
        <div className="date-nav">
          <button className="icon-btn" onClick={() => setDate(shiftPeriod(date, gran, -1))}>
            ‹
          </button>
          <button className="today-btn" onClick={() => setDate(toDateStr(new Date()))}>
            {isCurrentPeriod ? currentLabel : label}
          </button>
          <button
            className="icon-btn"
            disabled={isCurrentPeriod}
            onClick={() => setDate(shiftPeriod(date, gran, 1))}
          >
            ›
          </button>
        </div>
        <Segmented
          value={gran}
          options={[
            ["day", "Day"],
            ["week", "Week"],
            ["month", "Month"],
          ]}
          onChange={setGran}
        />
      </header>

      <div className="pane-body">
        {!axOk && (
          <div className="ax-banner">
            <span>
              Enable <strong>window titles</strong> to break apps down by document &amp; tab.
            </span>
            <button
              type="button"
              className="ax-grant"
              onClick={async () => {
                await api.axRequest();
                setAxOk(await api.axStatus());
              }}
            >
              Grant Accessibility…
            </button>
          </div>
        )}
        <h1 className="usage-total">{view ? fmtDur(view.total_seconds) : "—"}</h1>
        <p className="usage-sub">{view?.label ?? label} · tracked time</p>

        {view && <HourChart hours={view.hours} />}

        <div className="app-list">
          {view?.apps.length === 0 && (
            <p className="empty">No activity tracked for this {gran} yet.</p>
          )}
          {view?.apps.map((a) => (
            <AppRow
              key={a.app_key}
              app={a}
              projectById={projectById}
              onUnassign={async (title, dates) => {
                await Promise.all(
                  dates.map((d) => api.setAssignment(d, a.app_key, title, null))
                );
                await load();
                onAssignmentChange();
              }}
              onDragStart={(payload) => props.onDragApp(payload)}
              onDragEnd={() => props.onDragApp(null)}
            />
          ))}
        </div>
      </div>
    </>
  );
}

function startDrag(e: ReactDragEvent, payload: DragPayload) {
  const encoded = JSON.stringify(payload);
  e.dataTransfer.setData("application/x-timetracker-app", encoded);
  e.dataTransfer.setData("application/x-app", encoded);
  e.dataTransfer.setData("text/plain", payload.appKey);
  e.dataTransfer.effectAllowed = "link";
  e.stopPropagation();
}

/** Colored dots for the projects a row is explicitly linked to (click to unassign). */
function ProjectDots(props: {
  projects: RowProject[];
  projectById: Map<number, Project>;
  onUnassign: (dates: string[]) => void;
}) {
  if (props.projects.length === 0) return null;
  return (
    <span className="proj-dots">
      {props.projects.map((p) => {
        const proj = props.projectById.get(p.id);
        if (!proj) return null;
        return (
          <span
            key={p.id}
            className="proj-dot"
            style={{ background: proj.color }}
            title={`${proj.name} — click to unassign`}
            onClick={(e) => {
              e.stopPropagation();
              props.onUnassign(p.dates);
            }}
          />
        );
      })}
    </span>
  );
}

function AppRow(props: {
  app: PeriodAppUsage;
  projectById: Map<number, Project>;
  onUnassign: (title: string, dates: string[]) => void;
  onDragStart: (payload: DragPayload) => void;
  onDragEnd: () => void;
}) {
  const { app, projectById } = props;
  const [icon, setIcon] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const color = appColor(app.app_key);

  useEffect(() => {
    let alive = true;
    const loadTimer = window.setTimeout(() => {
      loadAppIcon(app.bundle_id)
        .then((src) => {
          if (alive) setIcon(src);
        })
        .catch(() => {
          if (alive) setIcon(null);
        });
    }, 120);

    setIcon(null);
    return () => {
      alive = false;
      window.clearTimeout(loadTimer);
    };
  }, [app.bundle_id]);

  const canExpand = app.titles.length > 1; // only worth it with real title detail

  return (
    <div className="app-group">
      <div
        className="app-row"
        draggable
        onDragStart={(e) => {
          const payload: DragPayload = { appKey: app.app_key, title: "", dates: app.dates };
          startDrag(e, payload);
          props.onDragStart(payload);
        }}
        onDragEnd={props.onDragEnd}
        title="Drag onto a project in the sidebar"
      >
        <button
          type="button"
          className={`disclosure ${canExpand ? "" : "empty"}`}
          tabIndex={canExpand ? 0 : -1}
          aria-label={expanded ? "Collapse" : "Expand"}
          onClick={(e) => {
            e.stopPropagation();
            if (canExpand) setExpanded((v) => !v);
          }}
        >
          {canExpand ? (expanded ? "▾" : "▸") : ""}
        </button>
        <span className={`app-badge ${icon ? "with-icon" : ""}`} style={{ background: color }}>
          {icon ? (
            <img src={icon} alt="" draggable={false} onError={() => setIcon(null)} />
          ) : (
            initials(app.app_name)
          )}
        </span>
        <span className="app-name-wrap">
          <span className="app-name">{app.app_name}</span>
          <ProjectDots
            projects={app.projects}
            projectById={projectById}
            onUnassign={(dates) => props.onUnassign("", dates)}
          />
          {canExpand && (
            <span className="title-count" title={`${app.titles.length} window titles`}>
              {app.titles.length}
            </span>
          )}
        </span>
        <MiniHourChart hours={app.hours} color={color} />
        <span className="app-time">{fmtDur(app.seconds)}</span>
      </div>

      {expanded && (
        <div className="title-list">
          {app.titles.map((t) => {
            // Untitled time shares the app-level key (""); assign it via the app row,
            // not by dragging this row, to avoid assigning the whole app by accident.
            const isUntitled = t.title.trim() === "";
            return (
              <div
                key={t.title || "(untitled)"}
                className={`title-row ${isUntitled ? "untitled" : ""}`}
                draggable={!isUntitled}
                onDragStart={
                  isUntitled
                    ? undefined
                    : (e) => {
                        const payload: DragPayload = {
                          appKey: app.app_key,
                          title: t.title,
                          dates: t.dates,
                        };
                        startDrag(e, payload);
                        props.onDragStart(payload);
                      }
                }
                onDragEnd={isUntitled ? undefined : props.onDragEnd}
                title={
                  isUntitled
                    ? "Untitled windows — assign via the app row"
                    : "Drag this title onto a project"
                }
              >
                <span className="title-name">{isUntitled ? "Untitled" : t.title}</span>
                {!isUntitled && (
                  <ProjectDots
                    projects={t.projects}
                    projectById={projectById}
                    onUnassign={(dates) => props.onUnassign(t.title, dates)}
                  />
                )}
                <span className="title-time">{fmtDur(t.seconds)}</span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function MiniHourChart({ hours, color }: { hours: number[]; color: string }) {
  const max = Math.max(1, ...hours);
  return (
    <span className="app-timeline" aria-hidden="true">
      {hours.map((s, i) => (
        <span className="mini-bar-col" key={i} title={`${labelHour(i)}: ${fmtDur(s)}`}>
          <span
            className="mini-bar"
            style={{
              height: `${Math.max(8, (s / max) * 100)}%`,
              opacity: s > 0 ? 1 : 0.16,
              background: color,
            }}
          />
        </span>
      ))}
    </span>
  );
}

/** Screen Time-style 24-hour usage bar chart. */
function HourChart({ hours }: { hours: number[] }) {
  const max = Math.max(1, ...hours);
  return (
    <div className="hour-chart">
      <div className="bars">
        {hours.map((s, i) => (
          <div className="bar-col" key={i} title={`${labelHour(i)}: ${fmtDur(s)}`}>
            <div
              className="bar"
              style={{ height: `${(s / max) * 100}%`, opacity: s > 0 ? 1 : 0.18 }}
            />
          </div>
        ))}
      </div>
      <div className="bar-axis">
        <span>12 AM</span>
        <span>6 AM</span>
        <span>12 PM</span>
        <span>6 PM</span>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Project pane (auto-groups by day / week / month)
// ---------------------------------------------------------------------------

function ProjectPane(props: { project?: Project; onDeleted: () => void }) {
  const { project, onDeleted } = props;
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
      <header className="pane-header">
        <div className="project-title">
          <span className="nav-dot" style={{ background: project.color }} />
          {project.name}
        </div>
        <div className="spacer" />
        <Segmented
          value={gran}
          options={[
            ["day", "Day"],
            ["week", "Week"],
            ["month", "Month"],
          ]}
          onChange={setGran}
        />
        <button
          className="icon-btn danger"
          title="Delete project"
          onClick={async () => {
            const confirmed = window.confirm(
              `Delete "${project.name}"?\n\nAssigned activity will be unassigned. This cannot be undone.`
            );
            if (!confirmed) return;
            await api.deleteProject(project.id);
            onDeleted();
          }}
        >
          Delete
        </button>
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

function Segmented<T extends string>(props: {
  value: T;
  options: [T, string][];
  onChange: (v: T) => void;
}) {
  return (
    <div className="segmented">
      {props.options.map(([v, label]) => (
        <button
          key={v}
          className={props.value === v ? "seg active" : "seg"}
          onClick={() => props.onChange(v)}
        >
          {label}
        </button>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

const appIconRequests = new Map<string, Promise<string | null>>();

function loadAppIcon(bundleId: string | null): Promise<string | null> {
  if (!bundleId) return Promise.resolve(null);
  const cached = appIconRequests.get(bundleId);
  if (cached) return cached;
  const request = api.appIcon(bundleId).catch(() => null);
  appIconRequests.set(bundleId, request);
  return request;
}

function parseDragPayload(dataTransfer: DataTransfer): DragPayload | null {
  for (const type of ["application/x-timetracker-app", "application/x-app"]) {
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

function mergeDayViews(
  days: DayView[],
  gran: Granularity,
  anchorDate: string
): PeriodView {
  type TitleAcc = { seconds: number; dates: string[]; datesByProject: Map<number, string[]> };
  type Accumulator = {
    app_key: string;
    app_name: string;
    bundle_id: string | null;
    seconds: number;
    hours: number[];
    dates: string[];
    datesByProject: Map<number, string[]>; // app-level links
    titles: Map<string, TitleAcc>;
  };

  const byApp = new Map<string, Accumulator>();
  const hours = emptyHours();
  let total = 0;

  for (const day of days) {
    total += day.total_seconds;
    addHours(hours, day.hours);

    for (const app of day.apps) {
      const entry =
        byApp.get(app.app_key) ??
        ({
          app_key: app.app_key,
          app_name: app.app_name,
          bundle_id: app.bundle_id,
          seconds: 0,
          hours: emptyHours(),
          dates: [],
          datesByProject: new Map<number, string[]>(),
          titles: new Map<string, TitleAcc>(),
        } satisfies Accumulator);

      entry.seconds += app.seconds;
      addHours(entry.hours, app.hours);
      entry.dates.push(day.date);
      if (app.project_id != null) pushDate(entry.datesByProject, app.project_id, day.date);

      for (const t of app.titles) {
        const tacc =
          entry.titles.get(t.title) ??
          { seconds: 0, dates: [], datesByProject: new Map<number, string[]>() };
        tacc.seconds += t.seconds;
        tacc.dates.push(day.date);
        if (t.project_id != null) pushDate(tacc.datesByProject, t.project_id, day.date);
        entry.titles.set(t.title, tacc);
      }

      byApp.set(app.app_key, entry);
    }
  }

  const apps: PeriodAppUsage[] = [...byApp.values()]
    .map((app) => ({
      app_key: app.app_key,
      app_name: app.app_name,
      bundle_id: app.bundle_id,
      seconds: app.seconds,
      hours: app.hours,
      dates: [...new Set(app.dates)].sort(),
      projects: rowProjects(app.datesByProject),
      titles: [...app.titles.entries()]
        .map(([title, t]) => ({
          title,
          seconds: t.seconds,
          dates: [...new Set(t.dates)].sort(),
          projects: rowProjects(t.datesByProject),
        }))
        .sort((a, b) => b.seconds - a.seconds),
    }))
    .sort((a, b) => b.seconds - a.seconds);

  return {
    label: periodLabel(anchorDate, gran),
    total_seconds: total,
    apps,
    hours,
  };
}

function pushDate(map: Map<number, string[]>, id: number, date: string) {
  const list = map.get(id) ?? [];
  list.push(date);
  map.set(id, list);
}

function rowProjects(map: Map<number, string[]>): RowProject[] {
  return [...map.entries()]
    .map(([id, dates]) => ({ id, dates: [...new Set(dates)].sort() }))
    .sort((a, b) => a.id - b.id);
}

function emptyHours(): number[] {
  return Array.from({ length: 24 }, () => 0);
}

function addHours(target: number[], source: number[]) {
  for (let i = 0; i < 24; i++) {
    target[i] += source[i] ?? 0;
  }
}

function periodDates(dateStr: string, gran: Granularity): string[] {
  const start = periodStart(dateStr, gran);
  let end = periodEnd(start, gran);
  const today = toDateStr(new Date());
  if (end > today) end = today;

  const dates: string[] = [];
  for (let d = start; d <= end; d = addDays(d, 1)) {
    dates.push(d);
  }
  return dates.length > 0 ? dates : [start];
}

function periodStart(dateStr: string, gran: Granularity): string {
  if (gran === "day") return dateStr;
  if (gran === "week") return weekKey(dateStr);
  return `${monthKey(dateStr)}-01`;
}

function periodEnd(start: string, gran: Granularity): string {
  if (gran === "day") return start;
  if (gran === "week") return addDays(start, 6);
  return addDays(addMonths(start, 1), -1);
}

function shiftPeriod(dateStr: string, gran: Granularity, amount: number): string {
  if (gran === "day") return addDays(dateStr, amount);
  if (gran === "week") return addDays(dateStr, amount * 7);
  return addMonths(periodStart(dateStr, "month"), amount);
}

function samePeriod(a: string, b: string, gran: Granularity): boolean {
  return periodStart(a, gran) === periodStart(b, gran);
}

function periodLabel(dateStr: string, gran: Granularity): string {
  const today = toDateStr(new Date());
  if (gran === "day") return dateStr === today ? "Today" : prettyDate(dateStr);
  if (gran === "week") {
    return samePeriod(dateStr, today, "week")
      ? "This Week"
      : `Week of ${prettyDate(weekKey(dateStr))}`;
  }
  return samePeriod(dateStr, today, "month") ? "This Month" : monthLabel(monthKey(dateStr));
}

function addMonths(dateStr: string, amount: number): string {
  const [y, m] = dateStr.split("-").map(Number);
  return toDateStr(new Date(y, m - 1 + amount, 1));
}

function rollup(rows: DayTotal[], gran: Granularity) {
  const map = new Map<string, number>();
  for (const r of rows) {
    const key = gran === "day" ? r.date : gran === "week" ? weekKey(r.date) : monthKey(r.date);
    map.set(key, (map.get(key) ?? 0) + r.seconds);
  }
  return [...map.entries()]
    .sort((a, b) => (a[0] < b[0] ? 1 : -1))
    .map(([key, seconds]) => ({ key, seconds, label: bucketLabel(key, gran) }));
}

function bucketLabel(key: string, gran: Granularity): string {
  if (gran === "month") {
    return monthLabel(key);
  }
  if (gran === "week") return `Week of ${prettyDate(key)}`;
  return prettyDate(key);
}

function monthLabel(key: string): string {
  const [y, m] = key.split("-").map(Number);
  return new Date(y, m - 1, 1).toLocaleDateString([], { month: "long", year: "numeric" });
}

function prettyDate(dateStr: string): string {
  const [y, m, d] = dateStr.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString([], {
    weekday: "short",
    day: "numeric",
    month: "short",
  });
}

function labelHour(h: number): string {
  if (h === 0) return "12 AM";
  if (h < 12) return `${h} AM`;
  if (h === 12) return "12 PM";
  return `${h - 12} PM`;
}
