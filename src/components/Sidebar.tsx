// Left rail: the Activities entry, the project list, and inline project
// creation. Each project row doubles as a drop target for dragged app/title
// time (ProjectNavItem) and a drag source for reordering.

import {
  type CSSProperties,
  type MouseEvent,
  type RefObject,
  useRef,
  useState,
} from "react";
import { Activity, Ban, PanelLeftClose, PanelLeftOpen, Settings, Trash2 } from "lucide-react";
import { ask } from "@tauri-apps/plugin-dialog";
import { api, initials, PROJECT_COLORS, type Project } from "../api";
import {
  type DragPayload,
  isProjectDrag,
  parseDragPayload,
  parseProjectDragId,
  reorderIds,
  startProjectDrag,
} from "../drag";

export type Selection =
  | { kind: "day" }
  | { kind: "ignored" }
  | { kind: "settings" }
  | { kind: "project"; id: number };

export function Sidebar(props: {
  projects: Project[];
  selection: Selection;
  onSelect: (s: Selection) => void;
  onProjectsChanged: () => void;
  dragPayload: DragPayload | null;
  getDragPayload: () => DragPayload | null;
  onAssignApp: (payload: DragPayload, projectId: number) => Promise<void>;
  onIgnoreApp: (payload: DragPayload) => Promise<void>;
}) {
  const {
    projects,
    selection,
    onSelect,
    onProjectsChanged,
    dragPayload,
    getDragPayload,
    onAssignApp,
    onIgnoreApp,
  } = props;
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState("");
  const [collapsed, setCollapsed] = useState(
    () => localStorage.getItem("echo.sidebar-collapsed") === "1"
  );
  const [ignoreOver, setIgnoreOver] = useState(false);
  // Live ref for the in-flight project drag — WKWebView may drop custom
  // dataTransfer payloads between dragStart and drop.
  const draggingProject = useRef<number | null>(null);

  function toggleCollapsed() {
    const next = !collapsed;
    setCollapsed(next);
    localStorage.setItem("echo.sidebar-collapsed", next ? "1" : "0");
  }

  async function addProject() {
    const trimmed = name.trim();
    if (!trimmed) return;
    const color = PROJECT_COLORS[projects.length % PROJECT_COLORS.length];
    await api.createProject(trimmed, color);
    setName("");
    setAdding(false);
    onProjectsChanged();
  }

  function openAdding() {
    if (collapsed) {
      setCollapsed(false);
      localStorage.setItem("echo.sidebar-collapsed", "0");
    }
    setAdding((a) => !a);
  }

  return (
    <aside className={`sidebar ${collapsed ? "collapsed" : ""}`}>
      {/* Draggable strip behind the macOS traffic-light buttons */}
      <div className="sidebar-titlebar" data-tauri-drag-region="" />

      <button
        className={`nav-item activities-nav ${selection.kind === "day" ? "active" : ""}`}
        title="Activities"
        onClick={() => onSelect({ kind: "day" })}
      >
        <span className="activity-icon">
          <Activity size={16} />
        </span>
        {!collapsed && <span className="activity-label">Activities</span>}
      </button>

      <div className="sidebar-section">
        {!collapsed && <span>Projects</span>}
        <button className="icon-btn" title="New project" onClick={openAdding}>
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

      {projects.length === 0 && !adding && !collapsed && (
        <p className="sidebar-empty">No projects yet. Click + to add one, then drag apps onto it.</p>
      )}

      {projects.map((p) => (
        <ProjectNavItem
          key={p.id}
          project={p}
          projects={projects}
          active={selection.kind === "project" && selection.id === p.id}
          dragPayload={dragPayload}
          getDragPayload={getDragPayload}
          draggingProject={draggingProject}
          onAssignApp={onAssignApp}
          onSelect={() => onSelect({ kind: "project", id: p.id })}
          onProjectsChanged={onProjectsChanged}
          onSelectDay={() => onSelect({ kind: "day" })}
          collapsed={collapsed}
        />
      ))}

      <div className="sidebar-bottom">
        <button
          type="button"
          className={`nav-item ignore-target ${
            selection.kind === "ignored" ? "active" : ""
          } ${ignoreOver ? "drop-over" : ""}`}
          title="Ignore dragged activity"
          onClick={() => onSelect({ kind: "ignored" })}
          onDragEnter={(e) => {
            if (isProjectDrag(e.dataTransfer.types)) return;
            e.preventDefault();
            setIgnoreOver(true);
          }}
          onDragOver={(e) => {
            if (isProjectDrag(e.dataTransfer.types)) {
              e.dataTransfer.dropEffect = "none";
              return;
            }
            e.preventDefault();
            e.dataTransfer.dropEffect = "move";
            setIgnoreOver(true);
          }}
          onDragLeave={() => setIgnoreOver(false)}
          onDrop={(e) => {
            e.preventDefault();
            e.stopPropagation();
            setIgnoreOver(false);
            if (isProjectDrag(e.dataTransfer.types)) return;

            const payload = getDragPayload() ?? parseDragPayload(e.dataTransfer) ?? dragPayload;
            if (!payload) return;
            void onIgnoreApp(payload).catch((err) => {
              console.error("Failed to ignore activity", err);
            });
          }}
        >
          <span className="ignore-icon">
            <Ban size={16} />
          </span>
          {!collapsed && <span>Ignore</span>}
        </button>

        <button
          type="button"
          className={`nav-item settings-nav ${selection.kind === "settings" ? "active" : ""}`}
          title="Settings"
          onClick={() => onSelect({ kind: "settings" })}
        >
          <span className="settings-icon">
            <Settings size={16} />
          </span>
          {!collapsed && <span>Settings</span>}
        </button>

        <button
          type="button"
          className="nav-item collapse-nav"
          title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
          onClick={toggleCollapsed}
        >
          <span className="collapse-icon">
            {collapsed ? <PanelLeftOpen size={15} /> : <PanelLeftClose size={15} />}
          </span>
          {!collapsed && <span>{collapsed ? "Expand" : "Collapse"}</span>}
        </button>
      </div>
    </aside>
  );
}

/** A project row: drop target for dragged apps, drag source for reordering. */
function ProjectNavItem(props: {
  project: Project;
  projects: Project[];
  active: boolean;
  collapsed: boolean;
  dragPayload: DragPayload | null;
  getDragPayload: () => DragPayload | null;
  draggingProject: RefObject<number | null>;
  onAssignApp: (payload: DragPayload, projectId: number) => Promise<void>;
  onSelect: () => void;
  onProjectsChanged: () => void;
  onSelectDay: () => void;
}) {
  const {
    project,
    projects,
    active,
    collapsed,
    dragPayload,
    getDragPayload,
    draggingProject,
    onAssignApp,
    onSelect,
    onProjectsChanged,
    onSelectDay,
  } = props;
  const [over, setOver] = useState(false);
  const suppressClick = useRef(false);

  function selectProject() {
    if (suppressClick.current) return;
    onSelect();
  }

  async function handleDelete(e: MouseEvent) {
    // Stop propagation FIRST before any async work so the row's click
    // handler never fires even if the confirm dialog takes a moment.
    e.stopPropagation();
    try {
      const ok = await ask(
        "Assigned activity will be unassigned. This cannot be undone.",
        { title: `Delete "${project.name}"?`, kind: "warning" }
      );
      if (!ok) return;
      await api.deleteProject(project.id);
      if (active) onSelectDay();
      onProjectsChanged();
    } catch (err) {
      console.error("Failed to delete project", err);
    }
  }

  return (
    <div
      role="button"
      tabIndex={0}
      draggable
      title={project.name}
      className={`nav-item project ${active ? "active" : ""} ${over ? "drop-over" : ""}`}
      onClick={selectProject}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          selectProject();
        }
      }}
      onDragStart={(e) => {
        startProjectDrag(e, project.id);
        draggingProject.current = project.id;
      }}
      onDragEnd={() => {
        draggingProject.current = null;
      }}
      onDragEnter={(e) => {
        e.preventDefault();
        setOver(true);
      }}
      onDragOver={(e) => {
        e.preventDefault();
        e.dataTransfer.dropEffect = isProjectDrag(e.dataTransfer.types) ? "move" : "link";
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

        // Check PROJECT reorder FIRST (ref is authoritative; WKWebView may
        // have dropped the dataTransfer payload).
        const draggedId = draggingProject.current ?? parseProjectDragId(e.dataTransfer);
        if (draggedId != null) {
          if (draggedId !== project.id) {
            const newOrder = reorderIds(projects.map((p) => p.id), draggedId, project.id);
            void api.setProjectOrder(newOrder).then(onProjectsChanged).catch((err) => {
              console.error("Failed to reorder projects", err);
            });
          }
          return; // do NOT fall through to activity assignment
        }

        // Activity assignment path (unchanged).
        const payload = getDragPayload() ?? parseDragPayload(e.dataTransfer) ?? dragPayload;
        if (!payload) return;
        void onAssignApp(payload, project.id).catch((err) => {
          console.error("Failed to assign app to project", err);
        });
      }}
    >
      {collapsed ? (
        <span
          className="project-initials"
          style={{ "--project-color": project.color } as CSSProperties}
        >
          {initials(project.name)}
        </span>
      ) : (
        <span className="nav-dot" style={{ background: project.color }} />
      )}
      {!collapsed && <span className="project-name">{project.name}</span>}
      {!collapsed && (
        <button className="row-action icon-btn danger" title="Delete project" onClick={handleDelete}>
          <Trash2 size={13} />
        </button>
      )}
    </div>
  );
}
