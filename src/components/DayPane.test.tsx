import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DayPane } from "./DayPane";

const { mockApi } = vi.hoisted(() => ({
  mockApi: {
    getDayView: vi.fn(),
    axStatus: vi.fn(),
    axRequest: vi.fn(),
    axOpenSettings: vi.fn(),
    updateStatus: vi.fn(),
    installUpdate: vi.fn(),
    removeAssignment: vi.fn(),
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

function emptyDay(date: string) {
  return { date, total_seconds: 0, apps: [], hours: Array(24).fill(0) };
}

function renderPane() {
  return render(
    <DayPane
      projects={[]}
      assignmentVersion={0}
      onDragApp={vi.fn()}
      onAssignmentChange={vi.fn()}
    />
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  mockApi.getDayView.mockImplementation((date: string) => Promise.resolve(emptyDay(date)));
  mockApi.axStatus.mockResolvedValue(true);
  mockApi.axRequest.mockResolvedValue(true);
  mockApi.axOpenSettings.mockResolvedValue(undefined);
  mockApi.updateStatus.mockResolvedValue({ state: "idle" });
  mockApi.installUpdate.mockResolvedValue(undefined);
  mockApi.removeAssignment.mockResolvedValue(undefined);
});

// A day with one app carrying an app-level link (→ Work) and a title-level
// link (Flow plan → Personal), used to exercise the unassign confirm guards.
function linkedDay(date: string) {
  return {
    date,
    total_seconds: 300,
    hours: Array(24).fill(0),
    apps: [
      {
        app_key: "com.warp",
        app_name: "Warp",
        bundle_id: "com.warp",
        seconds: 300,
        hours: Array(24).fill(0),
        project_ids: [1],
        titles: [
          { title: "Flow plan", seconds: 200, project_ids: [2] },
          { title: "Other doc", seconds: 100, project_ids: [] },
        ],
      },
    ],
  };
}

const linkedProjects = [
  { id: 1, name: "Work", color: "#4f8cff" },
  { id: 2, name: "Personal", color: "#ff6f61" },
];

function renderLinkedPane() {
  return render(
    <DayPane
      projects={linkedProjects}
      assignmentVersion={0}
      onDragApp={vi.fn()}
      onAssignmentChange={vi.fn()}
    />
  );
}

describe("DayPane unassign confirmation", () => {
  beforeEach(() => {
    mockApi.getDayView.mockImplementation((date: string) => Promise.resolve(linkedDay(date)));
  });

  it("confirms before unassigning an app-level project link", async () => {
    const confirmSpy = vi.fn(() => true);
    vi.stubGlobal("confirm", confirmSpy);
    renderLinkedPane();

    await userEvent.click(await screen.findByTitle("Work — click to unassign"));

    expect(confirmSpy).toHaveBeenCalledWith("Remove Warp from Work?");
    await waitFor(() =>
      expect(mockApi.removeAssignment).toHaveBeenCalledWith(
        expect.any(String),
        "com.warp",
        "",
        1,
      )
    );
  });

  it("leaves an app-level link untouched when the confirm is cancelled", async () => {
    const confirmSpy = vi.fn(() => false);
    vi.stubGlobal("confirm", confirmSpy);
    renderLinkedPane();

    await userEvent.click(await screen.findByTitle("Work — click to unassign"));

    expect(confirmSpy).toHaveBeenCalledWith("Remove Warp from Work?");
    expect(mockApi.removeAssignment).not.toHaveBeenCalled();
  });

  it("confirms before unassigning a title-level project link", async () => {
    const confirmSpy = vi.fn(() => true);
    vi.stubGlobal("confirm", confirmSpy);
    renderLinkedPane();

    await userEvent.click(await screen.findByLabelText("Expand"));
    await userEvent.click(await screen.findByTitle("Personal — click to unassign"));

    expect(confirmSpy).toHaveBeenCalledWith("Remove Flow plan from Personal?");
    await waitFor(() =>
      expect(mockApi.removeAssignment).toHaveBeenCalledWith(
        expect.any(String),
        "com.warp",
        "Flow plan",
        2,
      )
    );
  });
});

describe("DayPane update banner", () => {
  it("renders nothing while the updater is idle", async () => {
    renderPane();
    await waitFor(() => expect(mockApi.updateStatus).toHaveBeenCalled());
    expect(screen.queryByText("Restart & Update")).not.toBeInTheDocument();
  });

  it("offers an available update and installs on click", async () => {
    mockApi.updateStatus.mockResolvedValue({ state: "available", version: "0.1.42" });
    renderPane();

    expect(await screen.findByText("0.1.42")).toBeInTheDocument();
    // Signed builds keep the Accessibility grant across updates, so the banner
    // must not tell the user to re-enable it.
    expect(screen.queryByText(/Accessibility/i)).not.toBeInTheDocument();
    expect(screen.getByText(/restart to finish installing/i)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Restart & Update" }));

    await waitFor(() => expect(mockApi.installUpdate).toHaveBeenCalledOnce());
  });

  it("shows download progress as a percentage when the total is known", async () => {
    mockApi.updateStatus.mockResolvedValue({
      state: "downloading",
      version: "0.1.42",
      downloaded: 3_700_000,
      total: 10_000_000,
    });
    renderPane();

    expect(await screen.findByText(/37%/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Restart & Update" })).not.toBeInTheDocument();
  });

  it("surfaces install failures with a retry", async () => {
    mockApi.updateStatus.mockResolvedValue({ state: "error", message: "signature mismatch" });
    renderPane();

    expect(await screen.findByText(/signature mismatch/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(mockApi.installUpdate).toHaveBeenCalledOnce());
  });
});

describe("DayPane accessibility banner", () => {
  it("opens System Settings from the re-grant flow", async () => {
    mockApi.axStatus.mockResolvedValue(false);
    renderPane();

    expect(await screen.findByText("Grant Accessibility…")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Open Settings" }));

    await waitFor(() => expect(mockApi.axOpenSettings).toHaveBeenCalledOnce());
  });
});
