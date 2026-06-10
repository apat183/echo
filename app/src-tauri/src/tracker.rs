//! Shared tracking state. The open segment lives here (not thread-local in the
//! poller) so pause and quit can close-and-write it — quitting must never lose
//! the in-progress segment (spec §4).

use crate::db::{self, DbState};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

#[derive(Clone, PartialEq, Eq)]
pub struct FrontApp {
    pub bundle: Option<String>,
    pub name: Option<String>,
    pub title: Option<String>,
}

pub struct OpenSegment {
    pub start_ts: i64,
    pub app: FrontApp,
}

#[derive(Default)]
pub struct TrackerState {
    pub paused: AtomicBool,
    pub current: Mutex<Option<OpenSegment>>,
}

/// Close and persist the open segment in `slot` (if any) at `end_ts`.
/// Lock order is always current → db; callers already hold `current`.
pub fn close_open_segment(slot: &mut Option<OpenSegment>, db: &DbState, end_ts: i64) {
    if let Some(seg) = slot.take() {
        if let Ok(conn) = db.lock() {
            let _ = db::insert_segment(
                &conn,
                seg.start_ts,
                end_ts,
                seg.app.bundle.as_deref(),
                seg.app.name.as_deref(),
                seg.app.title.as_deref(),
            );
        }
    }
}

impl TrackerState {
    /// Lock `current` and close-and-write it. Used by pause and quit.
    /// A poisoned lock is recovered — the flush must survive a prior panic.
    pub fn close_current(&self, db: &DbState, end_ts: i64) {
        let mut slot = self
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        close_open_segment(&mut slot, db, end_ts);
    }
}

/// "4h 12m" / "12m" / "0m" — minutes rounded down.
pub fn format_duration(secs: i64) -> String {
    let mins = secs.max(0) / 60;
    let (h, m) = (mins / 60, mins % 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::Arc;

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
    fn format_duration_renders_hours_and_minutes() {
        assert_eq!(format_duration(0), "0m");
        assert_eq!(format_duration(59), "0m");
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(3599), "59m");
        assert_eq!(format_duration(3600), "1h 0m");
        assert_eq!(format_duration(15_120), "4h 12m");
        assert_eq!(format_duration(-5), "0m");
    }

    #[test]
    fn close_current_writes_open_segment() {
        let db = mem_db();
        let state = TrackerState::default();
        *state.current.lock().unwrap() = Some(OpenSegment {
            start_ts: 100,
            app: front("A"),
        });

        state.close_current(&db, 130);

        assert_eq!(segments(&db), vec![(100, 130, Some("A".to_string()))]);
        assert!(state.current.lock().unwrap().is_none());
    }

    #[test]
    fn close_current_with_nothing_open_is_a_noop() {
        let db = mem_db();
        let state = TrackerState::default();
        state.close_current(&db, 130);
        assert!(segments(&db).is_empty());
    }
}
