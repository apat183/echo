import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as ApiModule from "../api";
import { RulesPane } from "./RulesPane";

const { mockApi } = vi.hoisted(() => ({
  mockApi: {
    listRules: vi.fn(),
    trackedApps: vi.fn(),
    trackedTitles: vi.fn(),
    createRule: vi.fn(),
    deleteRule: vi.fn(),
  },
}));

vi.mock("../api", async (importOriginal) => {
  const actual = await importOriginal<typeof ApiModule>();
  return { ...actual, api: { ...actual.api, ...mockApi } };
});

vi.mock("../appIcon", () => ({
  loadAppIcon: vi.fn(() => Promise.resolve(null)),
}));

const projects = [
  { id: 1, name: "Flowstate", color: "#4f8cff" },
  { id: 2, name: "Admin", color: "#ff6f61" },
];

function renderPane() {
  return render(<RulesPane projects={projects} refreshKey={0} onChanged={vi.fn()} />);
}

beforeEach(() => {
  vi.clearAllMocks();
  mockApi.listRules.mockResolvedValue([]);
  mockApi.trackedApps.mockResolvedValue([
    { app_key: "dev.warp", app_name: "Warp", bundle_id: "dev.warp", seconds: 7200 },
  ]);
  mockApi.trackedTitles.mockResolvedValue([
    { title: "ScriptR — Dashboard", seconds: 3600 },
    { title: "scriptr.io | Docs", seconds: 1800 },
    { title: "Hacker News", seconds: 900 },
  ]);
  mockApi.createRule.mockResolvedValue({
    id: 1,
    project_id: 1,
    app_key: "dev.warp",
    app_name: "Warp",
    title: "",
    effective_from: null,
  });
  mockApi.deleteRule.mockResolvedValue(undefined);
});

describe("RulesPane", () => {
  it("creates a retroactive rule by default", async () => {
    renderPane();

    await userEvent.selectOptions(await screen.findByLabelText("App"), "dev.warp");
    await userEvent.selectOptions(screen.getByLabelText("Project"), "1");
    await userEvent.click(screen.getByRole("button", { name: /Add rule/ }));

    // A null effective date is what makes the rule reach days already tracked;
    // an empty pattern is what makes it an app rule rather than a title one.
    await waitFor(() =>
      expect(mockApi.createRule).toHaveBeenCalledWith(1, "dev.warp", "Warp", "", null)
    );
  });

  it("creates a title rule and previews its reach before saving", async () => {
    renderPane();

    await userEvent.selectOptions(await screen.findByLabelText("App"), "dev.warp");
    await userEvent.selectOptions(screen.getByLabelText("Project"), "1");
    // Lower case on purpose: the pattern matches titles case-insensitively.
    await userEvent.type(await screen.findByLabelText("Window title contains"), "scriptr");

    // Two of the three titles contain it: 1h + 30m, and Hacker News is left out.
    expect(await screen.findByText(/Matches 2 titles/)).toBeInTheDocument();
    expect(screen.getByText(/1h 30m/)).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /Add rule/ }));
    await waitFor(() =>
      expect(mockApi.createRule).toHaveBeenCalledWith(1, "dev.warp", "Warp", "scriptr", null)
    );
  });

  it("says so when a pattern catches nothing tracked", async () => {
    renderPane();

    await userEvent.selectOptions(await screen.findByLabelText("App"), "dev.warp");
    await userEvent.type(await screen.findByLabelText("Window title contains"), "zzz");

    expect(await screen.findByText(/Matches nothing tracked so far/)).toBeInTheDocument();
  });

  it("stamps today's date when the rule is forward-only", async () => {
    renderPane();

    await userEvent.selectOptions(await screen.findByLabelText("App"), "dev.warp");
    await userEvent.selectOptions(screen.getByLabelText("Project"), "2");
    await userEvent.click(screen.getByLabelText(/Apply to days already tracked/));
    await userEvent.click(screen.getByRole("button", { name: /Add rule/ }));

    await waitFor(() => expect(mockApi.createRule).toHaveBeenCalledTimes(1));
    const [, , , , effectiveFrom] = mockApi.createRule.mock.calls[0];
    expect(effectiveFrom).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });

  it("refuses to add until both an app and a project are chosen", async () => {
    renderPane();

    const add = await screen.findByRole("button", { name: /Add rule/ });
    expect(add).toBeDisabled();

    await userEvent.selectOptions(screen.getByLabelText("App"), "dev.warp");
    expect(add).toBeDisabled();

    await userEvent.selectOptions(screen.getByLabelText("Project"), "1");
    expect(add).toBeEnabled();
  });

  it("warns that deleting a rule unbills the days it was covering", async () => {
    const confirmSpy = vi.fn(() => true);
    vi.stubGlobal("confirm", confirmSpy);
    mockApi.listRules.mockResolvedValue([
      {
        id: 3,
        project_id: 1,
        app_key: "dev.warp",
        app_name: "Warp",
        title: "",
        effective_from: null,
      },
    ]);
    renderPane();

    await userEvent.click(await screen.findByTitle("Delete rule"));

    expect(confirmSpy).toHaveBeenCalledWith(expect.stringContaining("Warp"));
    expect(confirmSpy).toHaveBeenCalledWith(expect.stringContaining("already reviewed"));
    await waitFor(() => expect(mockApi.deleteRule).toHaveBeenCalledWith(3));
  });

  it("shows a forward-only rule's start date instead of all history", async () => {
    mockApi.listRules.mockResolvedValue([
      {
        id: 4,
        project_id: 2,
        app_key: "com.zen",
        app_name: "Zen",
        title: "",
        effective_from: "2026-07-01",
      },
    ]);
    renderPane();

    expect(await screen.findByText("From 2026-07-01")).toBeInTheDocument();
    expect(screen.queryByText("All history")).not.toBeInTheDocument();
  });

  it("shows a title rule's pattern in the list", async () => {
    mockApi.listRules.mockResolvedValue([
      {
        id: 5,
        project_id: 1,
        app_key: "dev.warp",
        app_name: "Warp",
        title: "ScriptR",
        effective_from: null,
      },
    ]);
    renderPane();

    expect(await screen.findByText(/“ScriptR”/)).toBeInTheDocument();
  });
});
