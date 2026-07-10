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
