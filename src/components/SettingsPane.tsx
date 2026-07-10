// Settings pane: Appearance, Storage, Startup, and About groups. Both the
// sidebar gear and the tray "Settings…" item route here. Each section is filled
// by its own sibling ticket; this shell ships the layout.

import { Settings } from "lucide-react";

export function SettingsPane() {
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
          <p className="settings-placeholder">Coming soon.</p>
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
