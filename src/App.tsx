import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type Project } from "./api";
import type { DragPayload } from "./drag";
import { Sidebar, type Selection } from "./components/Sidebar";
import { DayPane } from "./components/DayPane";
import { ProjectPane } from "./components/ProjectPane";
import { IgnoredPane } from "./components/IgnoredPane";
import { SettingsPane } from "./components/SettingsPane";
import "./App.css";

export default function App() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [selection, setSelection] = useState<Selection>({ kind: "day" });
  const [dragPayload, setDragPayload] = useState<DragPayload | null>(null);
  const [assignmentVersion, setAssignmentVersion] = useState(0);
  const [ignoredVersion, setIgnoredVersion] = useState(0);
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
        dates.map((d) => api.addAssignment(d, payload.appKey, payload.title, projectId))
      );
      setAssignmentVersion((v) => v + 1);
      await refreshProjects();
    },
    [refreshProjects]
  );

  const ignoreActivity = useCallback(async (payload: DragPayload) => {
    await api.addIgnoredEntry(payload.appKey, payload.appName, payload.title);
    setAssignmentVersion((v) => v + 1);
    setIgnoredVersion((v) => v + 1);
    setSelection({ kind: "ignored" });
  }, []);

  const handleIgnoredChanged = useCallback(() => {
    setAssignmentVersion((v) => v + 1);
    setIgnoredVersion((v) => v + 1);
  }, []);

  const handleAssignmentChanged = useCallback(() => {
    setAssignmentVersion((v) => v + 1);
    refreshProjects();
  }, [refreshProjects]);

  // A Settings clear/reset can delete projects, assignments, and ignore rules —
  // refresh everything the sidebar and panes read.
  const handleDataChanged = useCallback(() => {
    setAssignmentVersion((v) => v + 1);
    setIgnoredVersion((v) => v + 1);
    refreshProjects();
  }, [refreshProjects]);

  useEffect(() => {
    refreshProjects();
  }, [refreshProjects]);

  // The tray "Settings…" item shows the window and asks the UI to open Settings.
  useEffect(() => {
    const unlisten = listen("show-settings", () => setSelection({ kind: "settings" }));
    return () => {
      void unlisten.then((off) => off());
    };
  }, []);

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
        onIgnoreApp={ignoreActivity}
      />
      <main className="main">
        {selection.kind === "day" ? (
          <DayPane
            projects={projects}
            assignmentVersion={assignmentVersion}
            onDragApp={handleDragApp}
            onAssignmentChange={handleAssignmentChanged}
          />
        ) : selection.kind === "ignored" ? (
          <IgnoredPane refreshKey={ignoredVersion} onChanged={handleIgnoredChanged} />
        ) : selection.kind === "settings" ? (
          <SettingsPane onDataChanged={handleDataChanged} />
        ) : (
          <ProjectPane
            project={projects.find((p) => p.id === selection.id)}
            onAssignmentChange={handleAssignmentChanged}
          />
        )}
      </main>
    </div>
  );
}
