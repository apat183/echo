// Rules view: the one place standing assignment rules are written, listed and
// deleted. A rule says "this app's time is always that project" and is resolved
// when time is read, so it applies to every day at once — including days
// already tracked, unless it is created forward-only (see docs/adr/0001).

import { useCallback, useEffect, useMemo, useState } from "react";
import { Plus, Wand2, X } from "lucide-react";
import {
  api,
  appColor,
  fmtDur,
  initials,
  toDateStr,
  type AssignmentRule,
  type Project,
  type TrackedApp,
} from "../api";
import { loadAppIcon } from "../appIcon";

export function RulesPane(props: {
  projects: Project[];
  refreshKey: number;
  onChanged: () => void;
}) {
  const { projects, refreshKey, onChanged } = props;
  const [rules, setRules] = useState<AssignmentRule[]>([]);
  const [apps, setApps] = useState<TrackedApp[]>([]);
  const [appKey, setAppKey] = useState("");
  const [projectId, setProjectId] = useState<number | "">("");
  const [retroactive, setRetroactive] = useState(true);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    api.listRules().then(setRules).catch(() => {});
    api.trackedApps().then(setApps).catch(() => {});
  }, []);

  useEffect(() => {
    load();
  }, [load, refreshKey]);

  const projectById = useMemo(
    () => new Map(projects.map((p) => [p.id, p])),
    [projects]
  );
  const appByKey = useMemo(() => new Map(apps.map((a) => [a.app_key, a])), [apps]);

  const canAdd = appKey !== "" && projectId !== "" && !busy;

  async function addRule() {
    if (appKey === "" || projectId === "") return;
    setBusy(true);
    try {
      // Forward-only rules are stamped with today; a retroactive rule carries no
      // date at all and so reaches everything already tracked.
      const from = retroactive ? null : toDateStr(new Date());
      await api.createRule(projectId, appKey, appByKey.get(appKey)?.app_name ?? null, from);
      setAppKey("");
      load();
      onChanged();
    } finally {
      setBusy(false);
    }
  }

  async function removeRule(rule: AssignmentRule) {
    const name = rule.app_name || rule.app_key;
    const project = projectById.get(rule.project_id)?.name ?? "this project";
    if (
      !window.confirm(
        `Delete the rule putting ${name} in ${project}?\n\n` +
          `Time it was classifying goes back to unassigned on every day, including days already reviewed.`
      )
    ) {
      return;
    }
    await api.deleteRule(rule.id);
    load();
    onChanged();
  }

  return (
    <>
      <header className="pane-header" data-tauri-drag-region="">
        <div className="project-title">
          <span className="ignore-pane-icon">
            <Wand2 size={15} />
          </span>
          Rules
        </div>
        <div className="spacer" />
      </header>

      <div className="pane-body">
        <p className="usage-sub rules-intro">
          A rule sends an app's time to a project on every day, so you stop dragging
          the same thing over and over.
        </p>

        <div className="rule-form">
          <select
            aria-label="App"
            value={appKey}
            onChange={(e) => setAppKey(e.target.value)}
          >
            <option value="">Choose an app…</option>
            {apps.map((a) => (
              <option key={a.app_key} value={a.app_key}>
                {a.app_name} · {fmtDur(a.seconds)}
              </option>
            ))}
          </select>

          <span className="rule-form-arrow">→</span>

          <select
            aria-label="Project"
            value={projectId}
            onChange={(e) => setProjectId(e.target.value === "" ? "" : Number(e.target.value))}
          >
            <option value="">Choose a project…</option>
            {projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>

          <label className="rule-form-retro">
            <input
              type="checkbox"
              checked={retroactive}
              onChange={(e) => setRetroactive(e.target.checked)}
            />
            Apply to days already tracked
          </label>

          <button type="button" className="rule-add" disabled={!canAdd} onClick={addRule}>
            <Plus size={14} /> Add rule
          </button>
        </div>

        <div className="project-apps">
          <div className="section-label">
            {rules.length} rule{rules.length === 1 ? "" : "s"}
          </div>
          {rules.length === 0 && <p className="empty">No rules yet.</p>}
          {rules.map((rule) => (
            <RuleRow
              key={rule.id}
              rule={rule}
              project={projectById.get(rule.project_id)}
              onRemove={removeRule}
            />
          ))}
        </div>
      </div>
    </>
  );
}

function RuleRow(props: {
  rule: AssignmentRule;
  project: Project | undefined;
  onRemove: (rule: AssignmentRule) => void;
}) {
  const { rule, project, onRemove } = props;
  const [icon, setIcon] = useState<string | null>(null);
  const appName = rule.app_name || rule.app_key;

  useEffect(() => {
    let alive = true;
    loadAppIcon(rule.app_key)
      .then((src) => alive && setIcon(src))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [rule.app_key]);

  return (
    <div className="project-app-row ignored-rule-row">
      <span
        className={`app-badge ${icon ? "with-icon" : ""}`}
        style={{ background: appColor(rule.app_key) }}
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
          {rule.effective_from ? `From ${rule.effective_from}` : "All history"}
        </span>
      </span>
      {project && (
        <span className="rule-project">
          <span className="nav-dot" style={{ background: project.color }} />
          {project.name}
        </span>
      )}
      <button
        type="button"
        className="icon-btn ignored-remove"
        title="Delete rule"
        onClick={() => onRemove(rule)}
      >
        <X size={14} />
      </button>
    </div>
  );
}
