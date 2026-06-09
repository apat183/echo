//! App-level foreground poller (DESIGN.md decisions #3, #4, #6).
//!
//! Every ~2s we read the frontmost application. We keep the *current* segment
//! in memory and only write a row when the frontmost app changes — so storage
//! is segments, not point-samples. Window titles are out of scope here
//! (app-level only); Accessibility comes later.

use crate::{ax, db};
use chrono::Utc;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const POLL_SECS: u64 = 2;
/// If two ticks are more than this far apart, the machine was suspended
/// (sleep / lid-close) — the polling thread is frozen during system sleep.
/// We close the open segment at the last good tick and don't bill the gap.
const SLEEP_GAP_SECS: i64 = 60;

#[derive(Clone, PartialEq, Eq)]
pub struct FrontApp {
    pub bundle: Option<String>,
    pub name: Option<String>,
    pub title: Option<String>,
}

#[cfg(target_os = "macos")]
fn read_frontmost() -> Option<FrontApp> {
    use objc2_app_kit::NSWorkspace;
    // NSWorkspace / NSRunningApplication are not main-thread-only, so reading
    // the frontmost app from this background thread is fine.
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    let bundle = app.bundleIdentifier().map(|s| s.to_string());
    let name = app.localizedName().map(|s| s.to_string());
    // Best-effort window title via Accessibility; None when not permitted.
    let title = ax::focused_window_title(app.processIdentifier());
    Some(FrontApp { bundle, name, title })
}

#[cfg(not(target_os = "macos"))]
fn read_frontmost() -> Option<FrontApp> {
    None
}

/// Spawn the polling thread. Runs for the lifetime of the process.
pub fn spawn(db: Arc<Mutex<Connection>>) {
    thread::spawn(move || {
        // (segment_start_ts, app)
        let mut current: Option<(i64, FrontApp)> = None;
        let mut last_tick = Utc::now().timestamp();

        loop {
            thread::sleep(Duration::from_secs(POLL_SECS));
            let now = Utc::now().timestamp();

            // Detect a suspend gap (sleep / lid-close): close the open segment at
            // the last good tick so the asleep stretch is never billed.
            if now - last_tick > SLEEP_GAP_SECS {
                if let Some((start, app)) = current.take() {
                    if let Ok(conn) = db.lock() {
                        let _ = db::insert_segment(
                            &conn,
                            start,
                            last_tick,
                            app.bundle.as_deref(),
                            app.name.as_deref(),
                            app.title.as_deref(),
                        );
                    }
                }
            }
            last_tick = now;

            let front = read_frontmost();

            let changed = match (&current, &front) {
                (Some((_, cur)), Some(f)) => cur != f,
                (None, None) => false,
                _ => true,
            };

            if changed {
                // Close and persist the segment that just ended.
                if let Some((start, app)) = current.take() {
                    if let Ok(conn) = db.lock() {
                        let _ = db::insert_segment(
                            &conn,
                            start,
                            now,
                            app.bundle.as_deref(),
                            app.name.as_deref(),
                            app.title.as_deref(),
                        );
                    }
                }
                // Open a new segment for whatever is frontmost now.
                current = front.map(|f| (now, f));
            }
        }
    });
}
