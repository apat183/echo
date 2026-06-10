// 24-hour usage charts: the full bar chart at the top of the day pane and the
// compact per-app timeline shown on each app row.

import { fmtDur } from "../api";
import { labelHour } from "../period";

export function MiniHourChart({ hours, color }: { hours: number[]; color: string }) {
  const max = Math.max(1, ...hours);
  return (
    <span className="app-timeline" aria-hidden="true">
      {hours.map((s, i) => (
        <span className="mini-bar-col" key={i} title={`${labelHour(i)}: ${fmtDur(s)}`}>
          <span
            className="mini-bar"
            style={{
              height: `${Math.max(8, (s / max) * 100)}%`,
              opacity: s > 0 ? 1 : 0.16,
              background: color,
            }}
          />
        </span>
      ))}
    </span>
  );
}

/** Screen Time-style 24-hour usage bar chart. */
export function HourChart({ hours }: { hours: number[] }) {
  const max = Math.max(1, ...hours);
  return (
    <div className="hour-chart">
      <div className="bars">
        {hours.map((s, i) => (
          <div className="bar-col" key={i} title={`${labelHour(i)}: ${fmtDur(s)}`}>
            <div
              className="bar"
              style={{ height: `${(s / max) * 100}%`, opacity: s > 0 ? 1 : 0.18 }}
            />
          </div>
        ))}
      </div>
      <div className="bar-axis">
        <span>12 AM</span>
        <span>6 AM</span>
        <span>12 PM</span>
        <span>6 PM</span>
      </div>
    </div>
  );
}
