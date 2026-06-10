// A small day/week/month-style segmented control, reused by both panes.
export function Segmented<T extends string>(props: {
  value: T;
  options: [T, string][];
  onChange: (v: T) => void;
}) {
  return (
    <div className="segmented">
      {props.options.map(([v, label]) => (
        <button
          key={v}
          className={props.value === v ? "seg active" : "seg"}
          onClick={() => props.onChange(v)}
        >
          {label}
        </button>
      ))}
    </div>
  );
}
