import { useEffect, useRef, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import {
  api,
  fmtDur,
  toDateStr,
  type ManualConflict,
  type ManualEntry,
  type ManualEntryInput,
  type TrackedApp,
} from "../api";

function timeLabel(timestamp: number) {
  return new Date(timestamp * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function ManualEntries({
  date,
  onChanged,
}: {
  date: string;
  onChanged: (date: string) => void;
}) {
  const [entries, setEntries] = useState<ManualEntry[]>([]);
  const [apps, setApps] = useState<TrackedApp[]>([]);
  const [open, setOpen] = useState(false);
  const [input, setInput] = useState<ManualEntryInput>({
    appKey: "",
    date,
    startTime: "",
    endTime: "",
  });
  const [conflicts, setConflicts] = useState<ManualConflict[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState("");
  const [error, setError] = useState("");
  const [reload, setReload] = useState(0);
  const mounted = useRef(false);
  const addButton = useRef<HTMLButtonElement>(null);
  const appSelect = useRef<HTMLSelectElement>(null);
  const warning = useRef<HTMLDivElement>(null);
  const wasOpen = useRef(false);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    setLoadError("");
    Promise.all([api.listManualEntries(date), api.trackedApps()])
      .then(([nextEntries, nextApps]) => {
        if (alive) {
          setEntries(nextEntries);
          setApps(nextApps);
        }
      })
      .catch((e) => {
        if (alive) setLoadError(String(e));
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [date, reload]);

  useEffect(() => {
    if (wasOpen.current && !open) addButton.current?.focus();
    wasOpen.current = open;
  }, [open]);

  useEffect(() => {
    if (conflicts) warning.current?.focus();
  }, [conflicts]);

  function change(field: keyof ManualEntryInput, value: string) {
    setInput((current) => ({ ...current, [field]: value }));
    setConflicts(null);
    setError("");
  }

  const start = new Date(`${input.date}T${input.startTime}:00`).getTime();
  const end = new Date(`${input.date}T${input.endTime}:00`).getTime();
  const duration = (end - start) / 1000;

  async function save(allowOverlap: boolean) {
    if (busy) return;
    setError("");
    if (!Number.isFinite(duration) || duration <= 0 || input.endTime <= input.startTime) {
      setError("Stop time must be after start time on the same day.");
      return;
    }
    if (end > Date.now()) {
      setError("Stop time must already be in the past.");
      return;
    }
    setBusy(true);
    try {
      const result = await api.addManualEntry(input, allowOverlap);
      if (!mounted.current) return;
      if (result.status === "overlap") {
        setConflicts(result.conflicts);
      } else {
        setOpen(false);
        setConflicts(null);
        setInput({ appKey: "", date: input.date, startTime: "", endTime: "" });
        setReload((v) => v + 1);
        onChanged(input.date);
      }
    } catch (e) {
      if (mounted.current) {
        setError(String(e));
        setConflicts(null);
      }
    } finally {
      if (mounted.current) setBusy(false);
    }
  }

  async function remove(entry: ManualEntry) {
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      await api.deleteManualEntry(entry.id);
      if (!mounted.current) return;
      setEntries((current) => current.filter((row) => row.id !== entry.id));
      onChanged(date);
    } catch (e) {
      if (mounted.current) setError(String(e));
    } finally {
      if (mounted.current) setBusy(false);
    }
  }

  return (
    <section className="manual-entries" aria-label="Manual entries">
      <div className="manual-heading">
        <h2>Manual entries</h2>
        <button
          ref={addButton}
          type="button"
          className="ax-grant secondary"
          aria-expanded={open}
          disabled={busy || loading || !!loadError}
          onClick={() => {
            setOpen(true);
            setError("");
            setReload((v) => v + 1);
          }}
        >
          <Plus size={13} aria-hidden="true" /> Add manual entry
        </button>
      </div>
      {loadError && (
        <div className="manual-error">
          <p role="alert">{loadError}</p>
          <button
            type="button"
            className="ax-grant secondary"
            onClick={() => setReload((v) => v + 1)}
          >
            Retry loading
          </button>
        </div>
      )}
      {open && (
        <form
          className="manual-form"
          onSubmit={(e) => {
            e.preventDefault();
            void save(false);
          }}
        >
          <fieldset disabled={busy}>
            <div className="manual-fields">
              <label>
                App
                <select
                  ref={appSelect}
                  aria-label="App"
                  autoFocus
                  required
                  value={input.appKey}
                  onChange={(e) => change("appKey", e.target.value)}
                >
                  <option value="">Choose an app…</option>
                  {apps.map((app) => (
                    <option key={app.app_key} value={app.app_key}>
                      {app.app_name}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Date
                <input
                  type="date"
                  required
                  max={toDateStr(new Date())}
                  value={input.date}
                  onChange={(e) => change("date", e.target.value)}
                />
              </label>
              <label>
                Start time
                <input
                  type="time"
                  required
                  step={60}
                  value={input.startTime}
                  onChange={(e) => change("startTime", e.target.value)}
                />
              </label>
              <label>
                Stop time
                <input
                  type="time"
                  required
                  step={60}
                  value={input.endTime}
                  onChange={(e) => change("endTime", e.target.value)}
                />
              </label>
            </div>
            <p className="manual-hint" aria-live="polite">
              {Number.isFinite(duration) && duration > 0
                ? `Duration: ${fmtDur(duration)}`
                : "Choose a start and stop time on the same day."}
            </p>
            {apps.length === 0 && (
              <p className="manual-hint">
                No previously tracked apps are available. Track an app or restore an ignored app
                first.
              </p>
            )}
            {conflicts ? (
              <div className="manual-warning" role="alert" tabIndex={-1} ref={warning}>
                <strong>This entry overlaps existing activity.</strong>
                <p>Saving counts both durations and can make totals exceed elapsed time.</p>
                <ul>
                  {conflicts.map((conflict, i) => (
                    <li key={i}>
                      {conflict.app_name} · {toDateStr(new Date(conflict.start_ts * 1000))}{" "}
                      {timeLabel(conflict.start_ts)} – {toDateStr(new Date(conflict.end_ts * 1000))}{" "}
                      {timeLabel(conflict.end_ts)}
                    </li>
                  ))}
                </ul>
                <div className="manual-actions">
                  <button
                    type="button"
                    className="ax-grant secondary"
                    onClick={() => {
                      setConflicts(null);
                      appSelect.current?.focus();
                    }}
                  >
                    Go back
                  </button>
                  <button type="button" className="ax-grant" onClick={() => void save(true)}>
                    Save anyway
                  </button>
                </div>
              </div>
            ) : (
              <div className="manual-actions">
                <button type="submit" className="ax-grant" disabled={apps.length === 0}>
                  {busy ? "Saving…" : "Save entry"}
                </button>
                <button
                  type="button"
                  className="ax-grant secondary"
                  onClick={() => {
                    setOpen(false);
                    setConflicts(null);
                    setError("");
                  }}
                >
                  Cancel
                </button>
              </div>
            )}
          </fieldset>
        </form>
      )}
      {error && (
        <p className="manual-error" role="alert">
          {error}
        </p>
      )}
      {loading && (
        <p className="manual-hint" role="status">
          Loading manual entries…
        </p>
      )}
      {!loading && !loadError && entries.length === 0 && (
        <p className="manual-hint">No manual entries for this day.</p>
      )}
      {entries.length > 0 && (
        <ul className="manual-list">
          {entries.map((entry) => (
            <li key={entry.id}>
              <div>
                <strong>{entry.app_name}</strong>
                <span>
                  {date} · {timeLabel(entry.start_ts)} – {timeLabel(entry.end_ts)} ·{" "}
                  {fmtDur(entry.end_ts - entry.start_ts)}
                </span>
              </div>
              <button
                type="button"
                className="icon-btn"
                disabled={busy}
                aria-label={`Delete manual entry for ${entry.app_name} from ${timeLabel(entry.start_ts)} to ${timeLabel(entry.end_ts)}`}
                onClick={() => void remove(entry)}
              >
                <Trash2 size={14} aria-hidden="true" />
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
