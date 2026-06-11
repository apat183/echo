// Period usage charts: the full chart at the top of the activities pane and
// the compact per-app timeline shown on each app row.

import { useMemo, useState } from "react";
import { fmtDur } from "../api";
import type { PeriodChartBucket } from "../merge";
import type { Granularity } from "../period";

export function MiniUsageChart({
  buckets,
  color,
}: {
  buckets: PeriodChartBucket[];
  color: string;
}) {
  const max = Math.max(1, ...buckets.map((b) => b.seconds));
  return (
    <span className="app-timeline" aria-hidden="true">
      {buckets.map((bucket) => (
        <span
          className="mini-bar-col"
          key={bucket.key}
          title={`${bucket.label}: ${fmtDur(bucket.seconds)}`}
        >
          <span
            className="mini-bar"
            style={{
              height: `${Math.max(8, (bucket.seconds / max) * 100)}%`,
              opacity: bucket.seconds > 0 ? 1 : 0.16,
              background: color,
            }}
          />
        </span>
      ))}
    </span>
  );
}

/** Screen Time-style usage bar chart that follows the selected period scale. */
export function UsageChart({
  buckets,
  granularity,
}: {
  buckets: PeriodChartBucket[];
  granularity: Granularity;
}) {
  const [activeKey, setActiveKey] = useState<string | null>(null);
  const max = Math.max(1, ...buckets.map((b) => b.seconds));
  const total = buckets.reduce((sum, bucket) => sum + bucket.seconds, 0);
  const axisBuckets = chartAxisBuckets(buckets, granularity);
  const scaleTicks = useMemo(() => chartScaleTicks(max), [max]);
  const activeIndex = Math.max(0, buckets.findIndex((bucket) => bucket.key === activeKey));
  const activeBucket = activeKey ? buckets.find((bucket) => bucket.key === activeKey) : null;
  const rawTooltipLeft = buckets.length <= 1 ? 50 : (activeIndex / (buckets.length - 1)) * 100;
  const tooltipLeft = Math.min(92, Math.max(8, rawTooltipLeft));

  return (
    <div className="hour-chart">
      <div className="chart-stage">
        <div className="chart-scale" aria-hidden="true">
          {scaleTicks.map((tick) => (
            <span key={tick} style={{ bottom: `${(tick / max) * 100}%` }}>
              {formatScaleDuration(tick)}
            </span>
          ))}
        </div>

        <div className="chart-plot">
          <div className="chart-grid" aria-hidden="true">
            {scaleTicks.map((tick) => (
              <span key={tick} style={{ bottom: `${(tick / max) * 100}%` }} />
            ))}
          </div>

          <div className="bars" onMouseLeave={() => setActiveKey(null)}>
            {buckets.map((bucket) => {
              const isActive = bucket.key === activeKey;
              return (
                <button
                  type="button"
                  className={`bar-col ${isActive ? "active" : ""}`}
                  key={bucket.key}
                  aria-label={`${bucket.label}: ${fmtDur(bucket.seconds)}`}
                  onMouseEnter={() => setActiveKey(bucket.key)}
                  onFocus={() => setActiveKey(bucket.key)}
                  onBlur={() => setActiveKey(null)}
                >
                  <span
                    className="bar"
                    style={{
                      height: `${(bucket.seconds / max) * 100}%`,
                      opacity: bucket.seconds > 0 ? 1 : 0.18,
                    }}
                  />
                </button>
              );
            })}
          </div>

          {activeBucket && (
            <div className="chart-tooltip" style={{ left: `${tooltipLeft}%` }}>
              <span className="tooltip-label">{activeBucket.label}</span>
              <span className="tooltip-value">{fmtDur(activeBucket.seconds)}</span>
              <span className="tooltip-meta">{formatPercent(activeBucket.seconds, total)}</span>
            </div>
          )}
        </div>
      </div>

      <div className="bar-axis">
        {axisBuckets.map((bucket) => (
          <span key={bucket.key}>{bucket.axisLabel}</span>
        ))}
      </div>
    </div>
  );
}

function chartAxisBuckets(
  buckets: PeriodChartBucket[],
  granularity: Granularity,
): PeriodChartBucket[] {
  if (granularity === "day") return [0, 6, 12, 18].flatMap((i) => buckets[i] ?? []);
  if (granularity === "week") return buckets;

  const last = buckets.length - 1;
  if (last < 0) return [];
  return [
    ...new Set([
      0,
      Math.round(last * 0.25),
      Math.round(last * 0.5),
      Math.round(last * 0.75),
      last,
    ]),
  ]
    .map((i) => buckets[i]);
}

function chartScaleTicks(max: number): number[] {
  return [max, max * 0.5, 0];
}

function formatScaleDuration(seconds: number): string {
  if (seconds <= 0) return "0m";
  return fmtDur(seconds);
}

function formatPercent(seconds: number, total: number): string {
  if (total <= 0 || seconds <= 0) return "0%";
  return `${Math.round((seconds / total) * 100)}%`;
}
