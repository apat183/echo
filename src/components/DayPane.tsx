// Activities view: a Screen Time-style day/week/month readout. Loads each day's
// view, merges them, and renders draggable app/title rows that can be dropped on
// a project in the sidebar.

import { useCallback, useEffect, useMemo, useState } from "react";
import { Calendar, CalendarDays, CalendarRange, ChevronLeft, ChevronRight } from "lucide-react";
import { api, appColor, fmtDur, initials, type Project, toDateStr } from "../api";
import { type DragPayload, startDrag } from "../drag";
import { mergeDayViews, partitionTitles, type PeriodAppUsage, type PeriodTitleUsage, type PeriodView, type RowProject } from "../merge";
import { type Granularity, periodDates, periodLabel, samePeriod, shiftPeriod } from "../period";
import { loadAppIcon } from "../appIcon";
import { Segmented } from "./Segmented";
import { HourChart, MiniHourChart } from "./charts";

export function DayPane(props: {
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
            <ChevronLeft size={16} />
          </button>
          <button className="today-btn" onClick={() => setDate(toDateStr(new Date()))}>
            {isCurrentPeriod ? currentLabel : label}
          </button>
          <button
            className="icon-btn"
            disabled={isCurrentPeriod}
            onClick={() => setDate(shiftPeriod(date, gran, 1))}
          >
            <ChevronRight size={16} />
          </button>
        </div>
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
              onUnassign={async (title, projectId, dates) => {
                await Promise.all(
                  dates.map((d) => api.removeAssignment(d, a.app_key, title, projectId))
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

/** Colored dots for the projects a row is explicitly linked to (click to unassign). */
function ProjectDots(props: {
  projects: RowProject[];
  projectById: Map<number, Project>;
  onUnassign: (projectId: number, dates: string[]) => void;
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
              props.onUnassign(p.id, p.dates);
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
  onUnassign: (title: string, projectId: number, dates: string[]) => void;
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
            onUnassign={(projectId, dates) => props.onUnassign("", projectId, dates)}
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
        <TitleList
          app={app}
          projectById={projectById}
          onUnassign={props.onUnassign}
          onDragStart={props.onDragStart}
          onDragEnd={props.onDragEnd}
        />
      )}
    </div>
  );
}

function TitleList(props: {
  app: PeriodAppUsage;
  projectById: Map<number, Project>;
  onUnassign: (title: string, projectId: number, dates: string[]) => void;
  onDragStart: (payload: DragPayload) => void;
  onDragEnd: () => void;
}) {
  const { app, projectById } = props;
  const [showTiny, setShowTiny] = useState(false);
  const { major, tiny } = partitionTitles(app.titles);

  const renderTitleRow = (t: PeriodTitleUsage) => {
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
            onUnassign={(projectId, dates) => props.onUnassign(t.title, projectId, dates)}
          />
        )}
        <span className="title-time">{fmtDur(t.seconds)}</span>
      </div>
    );
  };

  const tinySeconds = tiny.reduce((s, t) => s + t.seconds, 0);

  return (
    <div className="title-list">
      {major.map(renderTitleRow)}
      {tiny.length > 0 && (
        <>
          <button
            type="button"
            className="title-row tiny-agg"
            onClick={() => setShowTiny((v) => !v)}
            aria-expanded={showTiny}
          >
            <span className="disclosure-inline">{showTiny ? "▾" : "▸"}</span>
            <span className="title-name">{tiny.length} items under a minute</span>
            <span className="title-time">{fmtDur(tinySeconds)}</span>
          </button>
          {showTiny && tiny.map(renderTitleRow)}
        </>
      )}
    </div>
  );
}
