import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { IgnoredPane } from "./IgnoredPane";

const { mockApi } = vi.hoisted(() => ({
  mockApi: {
    ignoredBreakdown: vi.fn(),
    listIgnoredEntries: vi.fn(),
    removeIgnoredEntry: vi.fn(),
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

beforeEach(() => {
  vi.clearAllMocks();
  mockApi.ignoredBreakdown.mockResolvedValue([{ date: "2026-06-11", seconds: 3_600 }]);
  mockApi.listIgnoredEntries.mockResolvedValue([
    {
      app_key: "com.apple.loginwindow",
      app_name: "loginwindow",
      title: "",
      created_at: 1_780_000_000,
    },
    {
      app_key: "com.warp",
      app_name: "Warp",
      title: "Hidden terminal title",
      created_at: 1_780_000_100,
    },
  ]);
  mockApi.removeIgnoredEntry.mockResolvedValue(undefined);
});

describe("IgnoredPane", () => {
  it("shows ignored app rules and restores one to activity", async () => {
    const onChanged = vi.fn();
    render(<IgnoredPane refreshKey={0} onChanged={onChanged} />);

    expect(await screen.findByText("loginwindow")).toBeInTheDocument();
    expect(screen.getByText("All activity")).toBeInTheDocument();
    expect(screen.getByText("Hidden terminal title")).toBeInTheDocument();

    await userEvent.click(screen.getAllByTitle("Stop ignoring")[0]);

    await waitFor(() => {
      expect(mockApi.removeIgnoredEntry).toHaveBeenCalledWith("com.apple.loginwindow", "");
    });
    expect(onChanged).toHaveBeenCalledOnce();
  });
});
