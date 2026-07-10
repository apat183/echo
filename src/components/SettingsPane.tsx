// Settings pane: Appearance, Storage, Startup, and About groups. Both the
// sidebar gear and the tray "Settings…" item route here. Each section is filled
// by its own sibling ticket; this shell ships the layout.

import { useCallback, useEffect, useState } from "react";
import { Coffee, ExternalLink, Monitor, Moon, Settings, Sun } from "lucide-react";
import { ask } from "@tauri-apps/plugin-dialog";
import { api, fmtBytes, type AutodeleteConfig } from "../api";
import { applyTheme, loadTheme, saveTheme, type Theme } from "../theme";
import { Segmented } from "./Segmented";

type OnOff = "on" | "off";

const ON_OFF_OPTIONS: { value: OnOff; label: string }[] = [
  { value: "on", label: "On" },
  { value: "off", label: "Off" },
];

const BUY_ME_A_COFFEE_URL = "https://buymeacoffee.com/anandpatel";
const GITHUB_URL = "https://github.com/apat183/echo";

/** `onDataChanged` fires after a clear/reset so the rest of the app can refetch. */
export function SettingsPane(props: { onDataChanged?: () => void }) {
  const { onDataChanged } = props;
  const [theme, setTheme] = useState<Theme>(loadTheme);
  const [version, setVersion] = useState<string | null>(null);
  const [size, setSize] = useState<number | null>(null);
  const [autodelete, setAutodelete] = useState<AutodeleteConfig>({ enabled: false, days: 30 });
  const [daysText, setDaysText] = useState("30");
  const [launchAtLogin, setLaunchAtLogin] = useState(false);

  const refreshSize = useCallback(() => {
    api.storageSize().then(setSize).catch(() => setSize(null));
  }, []);

  useEffect(() => {
    api.appVersion().then(setVersion).catch(() => setVersion(null));
  }, []);

  useEffect(() => {
    refreshSize();
  }, [refreshSize]);

  useEffect(() => {
    api
      .getAutodeleteConfig()
      .then((cfg) => {
        setAutodelete(cfg);
        setDaysText(String(cfg.days));
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    api.autostartIsEnabled().then(setLaunchAtLogin).catch(() => {});
  }, []);

  async function changeLaunchAtLogin(on: boolean) {
    setLaunchAtLogin(on); // optimistic
    try {
      if (on) await api.autostartEnable();
      else await api.autostartDisable();
    } catch {
      // Reconcile with the real login-item state if the toggle failed.
    }
    api.autostartIsEnabled().then(setLaunchAtLogin).catch(() => {});
  }

  function saveAutodelete(enabled: boolean, days: number) {
    setAutodelete({ enabled, days });
    void api.setAutodeleteConfig(enabled, days).catch(() => {});
  }

  function commitDays() {
    const days = Math.max(1, Math.floor(Number(daysText)) || 1);
    setDaysText(String(days));
    saveAutodelete(autodelete.enabled, days);
  }

  function changeTheme(next: Theme) {
    setTheme(next);
    applyTheme(next);
    saveTheme(next);
  }

  // Confirm via native dialog, run the destructive action, then refresh the
  // size line and let the rest of the app refetch what the clear may have wiped.
  async function confirmAndRun(title: string, message: string, run: () => Promise<unknown>) {
    const ok = await ask(message, { title, kind: "warning" });
    if (!ok) return;
    await run();
    refreshSize();
    onDataChanged?.();
  }

  const clearUntagged = () =>
    confirmAndRun(
      "Clear untagged data?",
      "Delete all activity that isn't assigned to any project? This cannot be undone.",
      api.clearUntagged
    );

  const clearTracking = () =>
    confirmAndRun(
      "Clear all tracking data?",
      "Delete all tracked activity and project assignments? Your projects and ignore rules are kept. This cannot be undone.",
      api.clearTrackingData
    );

  const resetEverything = () =>
    confirmAndRun(
      "Reset everything?",
      "Delete everything — all activity, projects, and ignore rules? This cannot be undone.",
      api.resetEverything
    );

  return (
    <>
      <header className="pane-header" data-tauri-drag-region="">
        <div className="project-title settings-title">
          <span className="settings-pane-icon">
            <Settings size={15} />
          </span>
          Settings
        </div>
      </header>

      <div className="pane-body settings-body">
        <section className="settings-section">
          <h2 className="settings-section-title">Appearance</h2>
          <div className="settings-row">
            <div className="settings-row-copy">
              <span className="settings-row-label">Theme</span>
              <span className="settings-row-hint">
                Match the system appearance or force a mode.
              </span>
            </div>
            <div className="settings-row-control">
              <Segmented
                value={theme}
                options={[
                  { value: "light" as Theme, label: "Light", icon: <Sun size={13} /> },
                  { value: "dark" as Theme, label: "Dark", icon: <Moon size={13} /> },
                  { value: "system" as Theme, label: "System", icon: <Monitor size={13} /> },
                ]}
                onChange={changeTheme}
              />
            </div>
          </div>
        </section>

        <section className="settings-section">
          <h2 className="settings-section-title">Storage</h2>
          <div className="settings-row">
            <div className="settings-row-copy">
              <span className="settings-row-label">Database size</span>
              <span className="settings-row-hint">Local SQLite file (main + WAL + SHM).</span>
            </div>
            <div className="settings-row-control">
              <span className="settings-storage-size">
                {size == null ? "…" : fmtBytes(size)}
              </span>
            </div>
          </div>
          <div className="settings-row">
            <div className="settings-row-copy">
              <span className="settings-row-label">Clear untagged data</span>
              <span className="settings-row-hint">
                Delete activity not assigned to any project.
              </span>
            </div>
            <div className="settings-row-control">
              <button type="button" className="ax-grant secondary" onClick={clearUntagged}>
                Clear
              </button>
            </div>
          </div>
          <div className="settings-row">
            <div className="settings-row-copy">
              <span className="settings-row-label">Clear all tracking data</span>
              <span className="settings-row-hint">
                Delete all activity and assignments. Keeps projects and ignore rules.
              </span>
            </div>
            <div className="settings-row-control">
              <button type="button" className="ax-grant danger" onClick={clearTracking}>
                Clear
              </button>
            </div>
          </div>
          <div className="settings-row">
            <div className="settings-row-copy">
              <span className="settings-row-label">Reset everything</span>
              <span className="settings-row-hint">
                Delete all data, including projects and ignore rules.
              </span>
            </div>
            <div className="settings-row-control">
              <button type="button" className="ax-grant danger" onClick={resetEverything}>
                Reset
              </button>
            </div>
          </div>
          <div className="settings-row">
            <div className="settings-row-copy">
              <span className="settings-row-label">Auto-delete untagged data</span>
              <span className="settings-row-hint">
                Periodically remove untagged activity older than the window below.
              </span>
            </div>
            <div className="settings-row-control">
              <Segmented
                value={autodelete.enabled ? "on" : "off"}
                options={ON_OFF_OPTIONS}
                onChange={(v) => saveAutodelete(v === "on", autodelete.days)}
              />
            </div>
          </div>
          {autodelete.enabled && (
            <div className="settings-row">
              <div className="settings-row-copy">
                <span className="settings-row-label">Keep untagged for</span>
                <span className="settings-row-hint">
                  Untagged activity older than this is purged on next launch and every 12h.
                </span>
              </div>
              <div className="settings-row-control">
                <input
                  type="number"
                  min={1}
                  className="settings-days-input"
                  value={daysText}
                  onChange={(e) => setDaysText(e.target.value)}
                  onBlur={commitDays}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                  }}
                />
                <span className="settings-days-unit">days</span>
              </div>
            </div>
          )}
        </section>

        <section className="settings-section">
          <h2 className="settings-section-title">Startup</h2>
          <div className="settings-row">
            <div className="settings-row-copy">
              <span className="settings-row-label">Launch Echo at login</span>
              <span className="settings-row-hint">
                Start tracking automatically when you sign in.
              </span>
            </div>
            <div className="settings-row-control">
              <Segmented
                value={launchAtLogin ? "on" : "off"}
                options={ON_OFF_OPTIONS}
                onChange={(v) => void changeLaunchAtLogin(v === "on")}
              />
            </div>
          </div>
        </section>

        <section className="settings-section">
          <h2 className="settings-section-title">About</h2>
          <div className="settings-about">
            <div className="settings-about-app">
              <span className="settings-about-name">Echo</span>
              <span className="settings-about-version">
                {version ? `Version ${version}` : "…"}
              </span>
            </div>
            <div className="settings-about-links">
              <button
                type="button"
                className="ax-grant"
                onClick={() => void api.openExternal(BUY_ME_A_COFFEE_URL)}
              >
                <Coffee size={14} />
                Buy Me a Coffee
              </button>
              <button
                type="button"
                className="ax-grant secondary"
                onClick={() => void api.openExternal(GITHUB_URL)}
              >
                <ExternalLink size={14} />
                GitHub
              </button>
            </div>
          </div>
        </section>
      </div>
    </>
  );
}
