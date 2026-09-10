import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Project } from "../api";
import { ProjectPane } from "./ProjectPane";

const { mockApi } = vi.hoisted(() => ({
  mockApi: {
    projectBreakdown: vi.fn(),
    projectApps: vi.fn(),
    projectDayEntries: vi.fn(),
    excludeForDay: vi.fn(),
    listProjectPeriodNotes: vi.fn(),
    setProjectPeriodNote: vi.fn(),
    removeFromProject: vi.fn(),
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
      state: "direct",
      rule_id: null,
      titles: [
        { title: "Flow plan", seconds: 7_200, state: "direct", rule_id: null },
        { title: "", seconds: 6_000, state: "direct", rule_id: null },
      ],
    },
  ]);
  mockApi.listProjectPeriodNotes.mockResolvedValue([
    { granularity: "week", period_key: "2026-06-08", note: "Week note" },
    { granularity: "day", period_key: "2026-06-11", note: "Note for Thursday" },
  ]);
  mockApi.setProjectPeriodNote.mockResolvedValue(undefined);
  mockApi.removeFromProject.mockResolvedValue(0);
  mockApi.projectDayEntries.mockResolvedValue([]);
  mockApi.excludeForDay.mockResolvedValue(undefined);
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

    await userEvent.click(screen.getByLabelText("Expand Week of Mon, Jun 8"));

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
      expect(mockApi.removeFromProject).toHaveBeenCalledWith(1, "com.warp", null);
    });
    expect(onAssignmentChange).toHaveBeenCalledOnce();
  });

  it("removes an individual title assignment from the project", async () => {
    const onAssignmentChange = vi.fn();
    render(<ProjectPane project={project} onAssignmentChange={onAssignmentChange} />);

    expect(await screen.findByText("Warp")).toBeInTheDocument();
    await userEvent.click(screen.getByLabelText("Expand"));
    await userEvent.click(await screen.findByTitle("Remove title from project"));

    expect(window.confirm).toHaveBeenCalledWith("Remove Flow plan from this project?");
    await waitFor(() => {
      expect(mockApi.removeFromProject).toHaveBeenCalledWith(1, "com.warp", "Flow plan");
    });
    expect(onAssignmentChange).toHaveBeenCalledOnce();
  });

  it("keeps an individual title assignment when the confirm is cancelled", async () => {
    const confirmSpy = vi.fn(() => false);
    vi.stubGlobal("confirm", confirmSpy);
    const onAssignmentChange = vi.fn();
    render(<ProjectPane project={project} onAssignmentChange={onAssignmentChange} />);

    expect(await screen.findByText("Warp")).toBeInTheDocument();
    await userEvent.click(screen.getByLabelText("Expand"));
    await userEvent.click(await screen.findByTitle("Remove title from project"));

    expect(confirmSpy).toHaveBeenCalledWith("Remove Flow plan from this project?");
    expect(mockApi.removeFromProject).not.toHaveBeenCalled();
    expect(onAssignmentChange).not.toHaveBeenCalled();
  });

  it("decomposes a day into what made it up and why", async () => {
    mockApi.projectDayEntries.mockResolvedValue([
      {
        app_key: "dev.warp",
        app_name: "Warp",
        bundle_id: "dev.warp",
        title: "",
        seconds: 3_600,
        state: "rule",
        rule_id: 7,
      },
    ]);
    render(<ProjectPane project={project} onAssignmentChange={vi.fn()} />);

    await userEvent.click(await screen.findByLabelText("Expand Week of Mon, Jun 8"));
    await userEvent.click(await screen.findByLabelText("Expand Thu, Jun 11"));

    // The receipt must say why a line is here, since after rules the answer is
    // no longer always "you put it there".
    expect(await screen.findByText("rule")).toBeInTheDocument();
    expect(mockApi.projectDayEntries).toHaveBeenCalledWith(1, "2026-06-11");
  });

  it("excludes a receipt line from just that day", async () => {
    const onAssignmentChange = vi.fn();
    mockApi.projectDayEntries.mockResolvedValue([
      {
        app_key: "dev.warp",
        app_name: "Warp",
        bundle_id: "dev.warp",
        title: "Flow plan",
        seconds: 3_600,
        state: "rule",
        rule_id: 7,
      },
    ]);
    render(<ProjectPane project={project} onAssignmentChange={onAssignmentChange} />);

    await userEvent.click(await screen.findByLabelText("Expand Week of Mon, Jun 8"));
    await userEvent.click(await screen.findByLabelText("Expand Thu, Jun 11"));
    await userEvent.click(
      await screen.findByTitle("Remove from this project on 2026-06-11")
    );

    await waitFor(() =>
      expect(mockApi.excludeForDay).toHaveBeenCalledWith(
        "2026-06-11",
        "dev.warp",
        "Flow plan",
        1,
      )
    );
  });

  it("offers removal for a title that is only inherited", async () => {
    mockApi.projectApps.mockResolvedValue([
      {
        app_key: "com.zen",
        app_name: "Zen",
        bundle_id: "com.zen",
        seconds: 600,
        state: "direct",
        rule_id: null,
        titles: [{ title: "A tab", seconds: 600, state: "inherited", rule_id: null }],
      },
    ]);
    render(<ProjectPane project={project} onAssignmentChange={vi.fn()} />);

    expect(await screen.findByText("Zen")).toBeInTheDocument();
    await userEvent.click(screen.getByLabelText("Expand"));

    // Inherited rows used to have no control at all — the wart ADR 0002 fixes.
    await userEvent.click(await screen.findByTitle("Remove title from project"));
    await waitFor(() =>
      expect(mockApi.removeFromProject).toHaveBeenCalledWith(1, "com.zen", "A tab")
    );
  });

  it("warns that removing a rule-covered app leaves the rule standing", async () => {
    const confirmSpy = vi.fn(() => true);
    vi.stubGlobal("confirm", confirmSpy);
    mockApi.projectApps.mockResolvedValue([
      {
        app_key: "dev.warp",
        app_name: "Warp",
        bundle_id: "dev.warp",
        seconds: 600,
        state: "rule",
        rule_id: 7,
        titles: [],
      },
    ]);
    render(<ProjectPane project={project} onAssignmentChange={vi.fn()} />);

    await userEvent.click(await screen.findByTitle("Remove Warp from project"));

    expect(confirmSpy).toHaveBeenCalledWith(expect.stringContaining("the rule itself stays"));
    await waitFor(() =>
      expect(mockApi.removeFromProject).toHaveBeenCalledWith(1, "dev.warp", null)
    );
  });
});
