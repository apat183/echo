import { useCallback, useEffect, useRef, useState } from "react";
import { api, type Project } from "./api";
import type { DragPayload } from "./drag";
import { Sidebar, type Selection } from "./components/Sidebar";
import { DayPane } from "./components/DayPane";
import { ProjectPane } from "./components/ProjectPane";
import "./App.css";

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
        dates.map((d) => api.addAssignment(d, payload.appKey, payload.title, projectId))
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
          />
        )}
      </main>
    </div>
  );
}
