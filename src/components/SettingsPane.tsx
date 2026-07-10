// Settings pane: Appearance, Storage, Startup, and About groups. Both the
// sidebar gear and the tray "Settings…" item route here. Each section is filled
// by its own sibling ticket; this shell ships the layout.

import { useState } from "react";
import { Monitor, Moon, Settings, Sun } from "lucide-react";
import { applyTheme, loadTheme, saveTheme, type Theme } from "../theme";
import { Segmented } from "./Segmented";

export function SettingsPane() {
  const [theme, setTheme] = useState<Theme>(loadTheme);

  function changeTheme(next: Theme) {
    setTheme(next);
    applyTheme(next);
    saveTheme(next);
  }

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
          <p className="settings-placeholder">Coming soon.</p>
        </section>

        <section className="settings-section">
          <h2 className="settings-section-title">Startup</h2>
          <p className="settings-placeholder">Coming soon.</p>
        </section>

        <section className="settings-section">
          <h2 className="settings-section-title">About</h2>
          <p className="settings-placeholder">Coming soon.</p>
        </section>
      </div>
    </>
  );
}
