// Left rail: the Activities entry, the project list, and inline project
// creation. Each project row doubles as a drop target for dragged app/title
// time (ProjectNavItem).

import { useRef, useState } from "react";
import { api, PROJECT_COLORS, type Project } from "../api";
import { type DragPayload, parseDragPayload } from "../drag";

export type Selection = { kind: "day" } | { kind: "project"; id: number };

export function Sidebar(props: {
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
      <div className="sidebar-brand">Echo</div>

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
