import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ManualEntries } from "./ManualEntries";

const { mockApi } = vi.hoisted(() => ({
  mockApi: {
    trackedApps: vi.fn(),
    listManualEntries: vi.fn(),
    addManualEntry: vi.fn(),
    deleteManualEntry: vi.fn(),
  },
}));
vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  api: mockApi,
}));

const date = "2000-01-15";
const start = new Date(`${date}T09:00:00`).getTime() / 1000;
const entry = {
  id: 7,
  app_key: "com.a",
  app_name: "Editor",
  bundle_id: "com.a",
  start_ts: start,
  end_ts: start + 5400,
};

beforeEach(() => {
  vi.clearAllMocks();
  mockApi.trackedApps.mockResolvedValue([
    { app_key: "com.a", app_name: "Editor", bundle_id: "com.a", seconds: 60 },
  ]);
  mockApi.listManualEntries.mockResolvedValue([]);
  mockApi.addManualEntry.mockResolvedValue({ status: "saved", entry });
  mockApi.deleteManualEntry.mockResolvedValue(undefined);
});

async function fillForm() {
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "Add manual entry" }));
  await user.selectOptions(screen.getByLabelText("App"), "com.a");
  await user.type(screen.getByLabelText("Start time"), "09:00");
  await user.type(screen.getByLabelText("Stop time"), "10:30");
  return user;
}

describe("ManualEntries", () => {
  it("refreshes newly tracked apps when the form opens", async () => {
    mockApi.trackedApps.mockResolvedValueOnce([]);
    render(<ManualEntries date={date} onChanged={vi.fn()} />);
    await screen.findByText("No manual entries for this day.");
    await userEvent.click(screen.getByRole("button", { name: "Add manual entry" }));
    expect(await screen.findByRole("option", { name: "Editor" })).toBeInTheDocument();
  });

  it("cancels without saving and returns keyboard focus to the add button", async () => {
    render(<ManualEntries date={date} onChanged={vi.fn()} />);
    const user = await fillForm();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByLabelText("Start time")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add manual entry" })).toHaveFocus();
    expect(mockApi.addManualEntry).not.toHaveBeenCalled();
  });

  it("restores keyboard focus after the saved entry finishes reloading", async () => {
    let finishLoad!: (entries: (typeof entry)[]) => void;
    render(<ManualEntries date={date} onChanged={vi.fn()} />);
    const user = await fillForm();
    mockApi.listManualEntries.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishLoad = resolve;
        }),
    );
    await user.click(screen.getByRole("button", { name: "Save entry" }));
    await screen.findByText("Loading manual entries…");
    await act(async () => {
      finishLoad([entry]);
    });
    expect(screen.getByRole("button", { name: "Add manual entry" })).toHaveFocus();
  });

  it("rejects a stop time later than the current time", async () => {
    render(<ManualEntries date={date} onChanged={vi.fn()} />);
    const user = await fillForm();
    const clock = vi.spyOn(Date, "now").mockReturnValue(new Date(`${date}T10:00:00`).getTime());
    try {
      await user.click(screen.getByRole("button", { name: "Save entry" }));
      expect(await screen.findByRole("alert")).toHaveTextContent(/already be in the past/);
      expect(mockApi.addManualEntry).not.toHaveBeenCalled();
    } finally {
      clock.mockRestore();
    }
  });

  it("does not navigate back to a day after its pending save is unmounted", async () => {
    let finishSave!: (result: unknown) => void;
    mockApi.addManualEntry.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishSave = resolve;
        }),
    );
    const changed = vi.fn();
    const pane = render(<ManualEntries date={date} onChanged={changed} />);
    const user = await fillForm();
    await user.click(screen.getByRole("button", { name: "Save entry" }));
    pane.unmount();
    await act(async () => {
      finishSave({ status: "saved", entry });
    });
    expect(changed).not.toHaveBeenCalled();
  });

  it("adds time on an empty day, shows duration, and refreshes the day", async () => {
    const changed = vi.fn();
    render(<ManualEntries date={date} onChanged={changed} />);
    const user = await fillForm();
    expect(screen.getByLabelText("Date")).toHaveValue(date);
    expect(screen.getByText("Duration: 1h 30m")).toBeInTheDocument();
    mockApi.listManualEntries.mockResolvedValue([entry]);
    await user.click(screen.getByRole("button", { name: "Save entry" }));
    await waitFor(() => expect(changed).toHaveBeenCalledWith(date));
    expect(mockApi.addManualEntry).toHaveBeenCalledWith(
      { appKey: "com.a", date, startTime: "09:00", endTime: "10:30" },
      false,
    );
    expect(await screen.findByRole("button", { name: /Delete manual entry/ })).toBeInTheDocument();
  });

  it("warns without saving until confirmation, and resets consent when fields change", async () => {
    mockApi.addManualEntry.mockResolvedValue({
      status: "overlap",
      conflicts: [{ app_name: "Browser", start_ts: start, end_ts: start + 3600 }],
    });
    const changed = vi.fn();
    render(<ManualEntries date={date} onChanged={changed} />);
    const user = await fillForm();
    await user.click(screen.getByRole("button", { name: "Save entry" }));
    expect(await screen.findByText(/counts both durations/)).toBeInTheDocument();
    expect(screen.getByText(/Browser/)).toBeInTheDocument();
    expect(changed).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Go back" }));
    expect(screen.getByLabelText("App")).toHaveFocus();
    expect(screen.getByLabelText("Start time")).toHaveValue("09:00");
    await user.click(screen.getByRole("button", { name: "Save entry" }));
    await screen.findByRole("button", { name: "Save anyway" });
    await user.clear(screen.getByLabelText("Stop time"));
    await user.type(screen.getByLabelText("Stop time"), "11:00");
    expect(screen.queryByRole("button", { name: "Save anyway" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Save entry" }));
    mockApi.addManualEntry.mockResolvedValue({ status: "saved", entry });
    await user.click(await screen.findByRole("button", { name: "Save anyway" }));
    expect(mockApi.addManualEntry).toHaveBeenLastCalledWith(
      expect.objectContaining({ endTime: "11:00" }),
      true,
    );
    await waitFor(() => expect(changed).toHaveBeenCalledOnce());
  });

  it("rejects reversed intervals without invoking the backend", async () => {
    render(<ManualEntries date={date} onChanged={vi.fn()} />);
    const user = await fillForm();
    await user.clear(screen.getByLabelText("Stop time"));
    await user.type(screen.getByLabelText("Stop time"), "08:00");
    await user.click(screen.getByRole("button", { name: "Save entry" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/after start time/);
    expect(mockApi.addManualEntry).not.toHaveBeenCalled();
  });

  it("preserves fields on failure, resets acknowledgement, and prevents duplicate submissions", async () => {
    let rejectSave!: (reason: string) => void;
    mockApi.addManualEntry.mockImplementation(
      () =>
        new Promise((_, reject) => {
          rejectSave = reject;
        }),
    );
    render(<ManualEntries date={date} onChanged={vi.fn()} />);
    const user = await fillForm();
    await user.dblClick(screen.getByRole("button", { name: "Save entry" }));
    expect(mockApi.addManualEntry).toHaveBeenCalledOnce();
    rejectSave("Could not save entry");
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not save entry");
    expect(screen.getByLabelText("Stop time")).toHaveValue("10:30");
    mockApi.addManualEntry.mockResolvedValue({ status: "saved", entry });
    await user.click(screen.getByRole("button", { name: "Save entry" }));
    expect(mockApi.addManualEntry).toHaveBeenLastCalledWith(expect.anything(), false);
  });

  it("keeps saved entries visible if deletion fails and allows retry", async () => {
    mockApi.listManualEntries.mockResolvedValue([entry]);
    mockApi.deleteManualEntry.mockRejectedValueOnce("Could not delete entry");
    const changed = vi.fn();
    render(<ManualEntries date={date} onChanged={changed} />);
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: /Delete manual entry/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not delete entry");
    expect(changed).not.toHaveBeenCalled();
    mockApi.listManualEntries.mockResolvedValue([]);
    await user.click(screen.getByRole("button", { name: /Delete manual entry/ }));
    await waitFor(() => expect(changed).toHaveBeenCalledWith(date));
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /Delete manual entry/ })).not.toBeInTheDocument(),
    );
    expect(mockApi.deleteManualEntry).toHaveBeenLastCalledWith(7);
  });

  it("shows a retry action for loading failures", async () => {
    mockApi.listManualEntries.mockRejectedValueOnce("Could not load entries");
    render(<ManualEntries date={date} onChanged={vi.fn()} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not load entries");
    await userEvent.click(screen.getByRole("button", { name: "Retry loading" }));
    expect(await screen.findByText("No manual entries for this day.")).toBeInTheDocument();
  });
});
