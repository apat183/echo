// Ignored activity view: project-style rollups for hidden time plus the rules
// that can be removed to let those apps/titles flow back into Activities.

import { useCallback, useEffect, useMemo, useState } from "react";
import { Ban, Calendar, CalendarDays, CalendarRange, RotateCcw } from "lucide-react";
import { api, appColor, fmtDur, initials, type DayTotal, type IgnoredEntry } from "../api";
import { loadAppIcon } from "../appIcon";
import { rollup, type Granularity } from "../period";
import { Segmented } from "./Segmented";

export function IgnoredPane(props: { refreshKey: number; onChanged: () => void }) {
  const { refreshKey, onChanged } = props;
  const [rows, setRows] = useState<DayTotal[]>([]);
  const [entries, setEntries] = useState<IgnoredEntry[]>([]);
  const [gran, setGran] = useState<Granularity>("week");

  const load = useCallback(() => {
    api.ignoredBreakdown().then(setRows);
    api.listIgnoredEntries().then(setEntries);
  }, []);

  useEffect(() => {
    load();
  }, [load, refreshKey]);

  async function removeEntry(entry: IgnoredEntry) {
    await api.removeIgnoredEntry(entry.app_key, entry.title);
    load();
    onChanged();
  }

  const buckets = useMemo(() => rollup(rows, gran), [rows, gran]);
  const total = useMemo(() => rows.reduce((s, r) => s + r.seconds, 0), [rows]);

  return (
    <>
      <header className="pane-header" data-tauri-drag-region="">
        <div className="project-title ignored-title">
          <span className="ignore-pane-icon">
            <Ban size={15} />
          </span>
          Ignored
        </div>
        <div className="spacer" />
        <Segmented
          value={gran}
          options={[
            { value: "day" as Granularity, label: "Day", icon: <Calendar size={13} /> },
            { value: "week" as Granularity, label: "Week", icon: <CalendarRange size={13} /> },
            { value: "month" as Granularity, label: "Month", icon: <CalendarDays size={13} /> },
          ]}
          onChange={setGran}
        />
      </header>

      <div className="pane-body">
        <h1 className="usage-total">{fmtDur(total)}</h1>
        <p className="usage-sub">
          {buckets.length} {gran}
          {buckets.length === 1 ? "" : "s"} · {entries.length} rule{entries.length === 1 ? "" : "s"}
        </p>

        <div className="project-apps">
          <div className="section-label">Rules</div>
          {entries.length === 0 && <p className="empty">Nothing ignored.</p>}
          {entries.map((entry) => (
            <IgnoredRuleRow
              key={`${entry.app_key}:${entry.title}`}
              entry={entry}
              onRemove={removeEntry}
            />
          ))}
        </div>

        {buckets.length > 0 && <div className="section-label">By {gran}</div>}
        <div className="bucket-list">
          {buckets.map((b) => (
            <div className="bucket-row" key={b.key}>
              <span className="bucket-label">{b.label}</span>
              <span className="bucket-bar-wrap">
                <span
                  className="bucket-bar ignored-bar"
                  style={{ width: `${(b.seconds / Math.max(1, buckets[0].seconds)) * 100}%` }}
                />
              </span>
              <span className="bucket-time">{fmtDur(b.seconds)}</span>
            </div>
          ))}
        </div>
      </div>
    </>
  );
}

function IgnoredRuleRow(props: {
  entry: IgnoredEntry;
  onRemove: (entry: IgnoredEntry) => void;
}) {
  const { entry, onRemove } = props;
  const [icon, setIcon] = useState<string | null>(null);
  const appName = entry.app_name || entry.app_key;

  useEffect(() => {
    let alive = true;
    loadAppIcon(entry.app_key)
      .then((src) => alive && setIcon(src))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [entry.app_key]);

  return (
    <div className="project-app-row ignored-rule-row">
      <span
        className={`app-badge ${icon ? "with-icon" : ""}`}
        style={{ background: appColor(entry.app_key) }}
      >
        {icon ? (
          <img src={icon} alt="" draggable={false} onError={() => setIcon(null)} />
        ) : (
          initials(appName)
        )}
      </span>
      <span className="ignored-rule-copy">
        <span className="project-app-name">{appName}</span>
        <span className="ignored-rule-subtitle">
          {entry.title.trim() === "" ? "All activity" : entry.title}
        </span>
      </span>
      <button
        type="button"
        className="icon-btn ignored-remove"
        title="Stop ignoring"
        onClick={() => onRemove(entry)}
      >
        <RotateCcw size={14} />
      </button>
    </div>
  );
}
