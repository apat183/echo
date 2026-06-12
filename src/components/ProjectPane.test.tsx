import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Project } from "../api";
import { ProjectPane } from "./ProjectPane";

const { mockApi } = vi.hoisted(() => ({
  mockApi: {
    projectBreakdown: vi.fn(),
    projectApps: vi.fn(),
    listProjectPeriodNotes: vi.fn(),
    setProjectPeriodNote: vi.fn(),
    removeProjectAppAssignments: vi.fn(),
    removeProjectTitleAssignments: vi.fn(),
  },
}));

vi.mock("../api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../api")>();
  return {
    ...actual,
    api: {
      ...actual.api,
      ...mockApi,
    },
  };
});

vi.mock("../appIcon", () => ({
  loadAppIcon: vi.fn(() => Promise.resolve(null)),
}));

const project: Project = { id: 1, name: "Flowstate", color: "#34c759" };

function mockProjectData() {
  mockApi.projectBreakdown.mockResolvedValue([
    { date: "2026-06-11", seconds: 7_800 },
    { date: "2026-06-10", seconds: 9_660 },
  ]);
  mockApi.projectApps.mockResolvedValue([
    {
      app_key: "com.warp",
      app_name: "Warp",
      bundle_id: "dev.warp.Warp-Stable",
      seconds: 13_200,
      titles: [
        { title: "Flow plan", seconds: 7_200, can_remove: true },
        { title: "", seconds: 6_000, can_remove: false },
      ],
    },
  ]);
  mockApi.listProjectPeriodNotes.mockResolvedValue([
    { granularity: "week", period_key: "2026-06-08", note: "Week note" },
    { granularity: "day", period_key: "2026-06-11", note: "Note for Thursday" },
  ]);
  mockApi.setProjectPeriodNote.mockResolvedValue(undefined);
  mockApi.removeProjectAppAssignments.mockResolvedValue(undefined);
  mockApi.removeProjectTitleAssignments.mockResolvedValue(undefined);
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal("confirm", vi.fn(() => true));
  mockProjectData();
});

describe("ProjectPane", () => {
  it("groups day notes under the week note", async () => {
    render(<ProjectPane project={project} onAssignmentChange={vi.fn()} />);

    expect(await screen.findByText("Week note")).toBeInTheDocument();
    expect(screen.getByText("Week of Mon, Jun 8")).toBeInTheDocument();
    expect(screen.getAllByText("4h 51m")).toHaveLength(2);

    await userEvent.click(screen.getByLabelText("Expand notes"));

    expect(await screen.findByText("Thu, Jun 11")).toBeInTheDocument();
    expect(screen.getByText("2h 10m")).toBeInTheDocument();
    expect(screen.getByText("Note for Thursday")).toBeInTheDocument();
  });

  it("removes all app assignments from the project", async () => {
    const onAssignmentChange = vi.fn();
    render(<ProjectPane project={project} onAssignmentChange={onAssignmentChange} />);

    expect(await screen.findByText("Warp")).toBeInTheDocument();
    await userEvent.click(screen.getByTitle("Remove Warp from project"));

    await waitFor(() => {
      expect(mockApi.removeProjectAppAssignments).toHaveBeenCalledWith(1, "com.warp");
    });
    expect(onAssignmentChange).toHaveBeenCalledOnce();
  });

  it("removes an individual title assignment from the project", async () => {
    const onAssignmentChange = vi.fn();
    render(<ProjectPane project={project} onAssignmentChange={onAssignmentChange} />);

    expect(await screen.findByText("Warp")).toBeInTheDocument();
    await userEvent.click(screen.getByLabelText("Expand"));
    await userEvent.click(await screen.findByTitle("Remove title from project"));

    await waitFor(() => {
      expect(mockApi.removeProjectTitleAssignments).toHaveBeenCalledWith(
        1,
        "com.warp",
        "Flow plan",
      );
    });
    expect(onAssignmentChange).toHaveBeenCalledOnce();
  });
});
