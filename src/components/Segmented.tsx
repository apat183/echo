// A small day/week/month-style segmented control, reused by both panes.
import type { ReactNode } from "react";

export function Segmented<T extends string>(props: {
  value: T;
  options: { value: T; label: string; icon?: ReactNode }[];
  onChange: (v: T) => void;
}) {
  return (
    <div className="segmented">
      {props.options.map((o) => (
        <button
          key={o.value}
          className={props.value === o.value ? "seg active" : "seg"}
          onClick={() => props.onChange(o.value)}
        >
          {o.icon}{o.label}
        </button>
      ))}
    </div>
  );
}
