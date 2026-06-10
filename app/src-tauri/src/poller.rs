//! App-level foreground poller (DESIGN.md decisions #3, #4, #6).
//!
//! Every ~2s we read the frontmost application. The *current* segment lives in
//! shared TrackerState (so pause/quit can flush it); a row is written only when
//! the frontmost app+title changes — segments, not point-samples.

use crate::tracker::{self, FrontApp, OpenSegment, TrackerState};
use crate::{ax, db, tray};
use chrono::Utc;
use db::DbState;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::AppHandle;

const POLL_SECS: u64 = 2;
/// If two ticks are more than this far apart, the machine was suspended
/// (sleep / lid-close) — the polling thread is frozen during system sleep.
/// We close the open segment at the last good tick and don't bill the gap.
const SLEEP_GAP_SECS: i64 = 60;

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

/// One poll tick. Everything mutating runs under the `current` lock, and the
/// paused flag is re-checked inside it, so a concurrent pause/quit can never
/// race a tick into reopening a segment.
pub fn process_tick(
    tracker: &TrackerState,
    db: &DbState,
    front: Option<FrontApp>,
    now: i64,
    last_tick: i64,
) {
    // Recover a poisoned lock: one panic must not permanently stop tracking.
    let mut current = tracker
        .current
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // Suspend gap (sleep / lid-close): close at the last good tick so the
    // asleep stretch is never billed.
    if now - last_tick > SLEEP_GAP_SECS {
        tracker::close_open_segment(&mut current, db, last_tick);
    }

    if tracker.paused.load(Ordering::SeqCst) {
        // Pause normally closed the segment already; this catches a tick that
        // was in flight when pause flipped.
        tracker::close_open_segment(&mut current, db, now);
        return;
    }

    let changed = match (&*current, &front) {
        (Some(open), Some(f)) => open.app != *f,
        (None, None) => false,
        _ => true,
    };

    if changed {
        tracker::close_open_segment(&mut current, db, now);
        *current = front.map(|f| OpenSegment { start_ts: now, app: f });
    }
}

/// Spawn the polling thread. Runs for the lifetime of the process.
pub fn spawn(app: AppHandle, db: DbState, tracker: Arc<TrackerState>) {
    thread::spawn(move || {
        let mut last_tick = Utc::now().timestamp();
        loop {
            thread::sleep(Duration::from_secs(POLL_SECS));
            let now = Utc::now().timestamp();
            let paused = tracker.paused.load(Ordering::SeqCst);
            // Skip the AX/NSWorkspace work entirely while paused.
            let front = if paused { None } else { read_frontmost() };
            process_tick(&tracker, &db, front, now, last_tick);
            last_tick = now;

            // Push the Today readout to the tray (closed segments + the open
            // segment's elapsed time; an open segment spanning midnight is
            // counted fully into today — a ~once-a-day rounding we accept).
            let date = db::local_date_string(now);
            let mut total = match db.lock() {
                Ok(conn) => db::day_total_seconds(&conn, &date).unwrap_or(0),
                Err(_) => 0,
            };
            if let Ok(cur) = tracker.current.lock() {
                if let Some(seg) = &*cur {
                    total += (now - seg.start_ts).max(0);
                }
            }
            tray::refresh_today(&app, total, paused);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DbState;
    use crate::tracker::TrackerState;
    use std::path::Path;
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};

    fn mem_db() -> DbState {
        Arc::new(Mutex::new(db::open(Path::new(":memory:")).unwrap()))
    }

    fn segments(db: &DbState) -> Vec<(i64, i64, Option<String>)> {
        let conn = db.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT start_ts, end_ts, app_name FROM segments ORDER BY id")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    fn front(name: &str) -> FrontApp {
        FrontApp {
            bundle: Some(format!("com.test.{name}")),
            name: Some(name.to_string()),
            title: None,
        }
    }

    #[test]
    fn app_change_closes_old_segment_and_opens_new() {
        let db = mem_db();
        let t = TrackerState::default();

        process_tick(&t, &db, Some(front("A")), 100, 98);
        assert!(segments(&db).is_empty()); // first sighting only opens

        process_tick(&t, &db, Some(front("A")), 102, 100);
        assert!(segments(&db).is_empty()); // unchanged → no write

        process_tick(&t, &db, Some(front("B")), 110, 108);
        assert_eq!(segments(&db), vec![(100, 110, Some("A".to_string()))]);
        let cur = t.current.lock().unwrap();
        assert_eq!(cur.as_ref().unwrap().start_ts, 110);
    }

    #[test]
    fn sleep_gap_closes_at_last_good_tick() {
        let db = mem_db();
        let t = TrackerState::default();

        process_tick(&t, &db, Some(front("A")), 100, 98);
        // 200s gap > SLEEP_GAP_SECS: close at last_tick (110), reopen at now (300)
        process_tick(&t, &db, Some(front("A")), 300, 110);

        assert_eq!(segments(&db), vec![(100, 110, Some("A".to_string()))]);
        let cur = t.current.lock().unwrap();
        assert_eq!(cur.as_ref().unwrap().start_ts, 300);
    }

    #[test]
    fn paused_tick_closes_current_and_records_nothing() {
        let db = mem_db();
        let t = TrackerState::default();

        process_tick(&t, &db, Some(front("A")), 100, 98);
        t.paused.store(true, Ordering::SeqCst);

        // paused: any leftover open segment is closed at `now`, nothing reopens
        process_tick(&t, &db, None, 110, 108);
        assert_eq!(segments(&db), vec![(100, 110, Some("A".to_string()))]);

        process_tick(&t, &db, None, 112, 110);
        assert_eq!(segments(&db).len(), 1);
        assert!(t.current.lock().unwrap().is_none());
    }

    #[test]
    fn paused_tick_with_live_front_app_records_nothing() {
        let db = mem_db();
        let t = TrackerState::default();

        process_tick(&t, &db, Some(front("A")), 100, 98);
        t.paused.store(true, Ordering::SeqCst);

        // A tick that raced past the paused check in spawn() and still read a
        // frontmost app must not record or reopen anything.
        process_tick(&t, &db, Some(front("B")), 110, 108);
        assert_eq!(segments(&db), vec![(100, 110, Some("A".to_string()))]);
        assert!(t.current.lock().unwrap().is_none());
    }

    #[test]
    fn resume_starts_a_fresh_segment() {
        let db = mem_db();
        let t = TrackerState::default();
        t.paused.store(true, Ordering::SeqCst);
        process_tick(&t, &db, None, 100, 98);

        t.paused.store(false, Ordering::SeqCst);
        process_tick(&t, &db, Some(front("A")), 110, 108);

        assert!(segments(&db).is_empty());
        let cur = t.current.lock().unwrap();
        assert_eq!(cur.as_ref().unwrap().start_ts, 110);
    }
}
