//! Shared tracking state. The open segment lives here (not thread-local in the
//! poller) so pause and quit can close-and-write it — quitting must never lose
//! the in-progress segment.

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
    pub fn add_manual_entry(
        &self,
        db: &DbState,
        input: &db::ManualEntryInput,
        allow_overlap: bool,
    ) -> Result<db::AddManualEntryResult, String> {
        let current = self.current.lock().map_err(|e| e.to_string())?;
        let conn = db.lock().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().timestamp();
        let open = current.as_ref().map(|seg| db::ManualConflict {
            start_ts: seg.start_ts,
            end_ts: now,
            app_name: seg
                .app
                .name
                .clone()
                .or_else(|| seg.app.bundle.clone())
                .unwrap_or_else(|| "unknown".into()),
        });
        db::add_manual_entry(&conn, input, allow_overlap, now, open)
    }

    /// Lock `current` and close-and-write it. Used by pause and quit.
    /// A poisoned lock is recovered — the flush must survive a prior panic.
    pub fn close_current(&self, db: &DbState, end_ts: i64) {
        let mut slot = self
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        close_open_segment(&mut slot, db, end_ts);
    }

    /// Drop the open segment *without* writing it (poison-recovered), mirroring
    /// `close_current` but discarding instead of flushing. The full-wipe storage
    /// commands call this so a cleared DB stays empty; the poller opens a fresh
    /// segment on its next tick.
    pub fn discard_current(&self) {
        let mut slot = self
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = None;
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

    #[test]
    fn manual_entry_warns_about_open_session_without_changing_tracking() {
        let db = mem_db();
        let state = TrackerState::default();
        let s = db::day_start_ts("2000-01-15");
        db::insert_segment(
            &db.lock().unwrap(),
            s - 60,
            s,
            Some("com.a"),
            Some("A"),
            None,
        )
        .unwrap();
        *state.current.lock().unwrap() = Some(OpenSegment {
            start_ts: s + 9 * 3600,
            app: front("B"),
        });
        let input = db::ManualEntryInput {
            app_key: "com.a".into(),
            date: "2000-01-15".into(),
            start_time: "09:30".into(),
            end_time: "10:00".into(),
        };
        assert!(matches!(
            state.add_manual_entry(&db, &input, false).unwrap(),
            db::AddManualEntryResult::Overlap { .. }
        ));
        assert_eq!(segments(&db).len(), 1);
        assert!(matches!(
            state.add_manual_entry(&db, &input, true).unwrap(),
            db::AddManualEntryResult::Saved { .. }
        ));
        let current = state.current.lock().unwrap();
        assert_eq!(current.as_ref().unwrap().start_ts, s + 9 * 3600);
        assert!(current.as_ref().unwrap().app == front("B"));
        assert!(!state.paused.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn manual_entry_can_be_added_while_paused_without_resuming() {
        let db = mem_db();
        let state = TrackerState::default();
        state
            .paused
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let s = db::day_start_ts("2000-01-15");
        db::insert_segment(&db.lock().unwrap(), s - 60, s, None, Some("A"), None).unwrap();
        let input = db::ManualEntryInput {
            app_key: "A".into(),
            date: "2000-01-15".into(),
            start_time: "09:00".into(),
            end_time: "10:00".into(),
        };
        assert!(matches!(
            state.add_manual_entry(&db, &input, false).unwrap(),
            db::AddManualEntryResult::Saved { .. }
        ));
        assert!(state.paused.load(std::sync::atomic::Ordering::SeqCst));
        assert!(state.current.lock().unwrap().is_none());
    }

    #[test]
    fn discard_current_drops_open_segment_without_writing() {
        let db = mem_db();
        let state = TrackerState::default();
        *state.current.lock().unwrap() = Some(OpenSegment {
            start_ts: 100,
            app: front("A"),
        });

        state.discard_current();

        assert!(segments(&db).is_empty()); // discarded, not flushed
        assert!(state.current.lock().unwrap().is_none());
    }
}
