//! Auto-update flow (Rust-owned): periodic background check, tray + banner
//! surfacing, user-triggered install.
//!
//! The frontend polls `update_status` (like `ax_status`) and the tray item is
//! push-updated (like `refresh_today`). Install downloads, then pauses the
//! tracker and flushes the open segment *before* restarting — `app.restart()`
//! can skip `RunEvent::Exit`, so the exit-handler flush alone is not enough.
//! A double flush is harmless: `close_current` takes the slot.

use crate::db::DbState;
use crate::tracker::TrackerState;
use crate::tray;
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

const FIRST_CHECK_DELAY_SECS: u64 = 15;
const CHECK_INTERVAL_SECS: u64 = 6 * 60 * 60;

const IDLE_TRAY_TEXT: &str = "Check for Updates…";

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Available(String),
    Downloading(String),
    Installing(String),
    Error(String),
}

#[derive(Default)]
pub struct UpdaterState {
    pending: Mutex<Option<tauri_plugin_updater::Update>>,
    phase: Mutex<Phase>,
    downloaded: AtomicU64,
    /// Total download size in bytes; 0 = unknown.
    total: AtomicU64,
}

/// Wire shape of the `update_status` command — mirrors `UpdateStatus` in
/// `src/api.ts`; keep the two in sync.
#[derive(Serialize, Clone)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UpdateStatus {
    Idle,
    Available {
        version: String,
    },
    Downloading {
        version: String,
        downloaded: u64,
        total: Option<u64>,
    },
    Installing {
        version: String,
    },
    Error {
        message: String,
    },
}

fn lock_phase(state: &UpdaterState) -> std::sync::MutexGuard<'_, Phase> {
    state.phase.lock().unwrap_or_else(|p| p.into_inner())
}

pub fn status(state: &UpdaterState) -> UpdateStatus {
    match &*lock_phase(state) {
        Phase::Idle => UpdateStatus::Idle,
        Phase::Available(v) => UpdateStatus::Available { version: v.clone() },
        Phase::Downloading(v) => {
            let total = state.total.load(Ordering::SeqCst);
            UpdateStatus::Downloading {
                version: v.clone(),
                downloaded: state.downloaded.load(Ordering::SeqCst),
                total: (total > 0).then_some(total),
            }
        }
        Phase::Installing(v) => UpdateStatus::Installing { version: v.clone() },
        Phase::Error(m) => UpdateStatus::Error { message: m.clone() },
    }
}

/// Spawn the periodic update-check thread. Release builds only: a debug build
/// reports the committed 0.1.0 and would always see the latest CI build as an
/// "update" — and could clobber the installed app with it.
pub fn spawn_periodic_check(app: AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(FIRST_CHECK_DELAY_SECS));
        loop {
            tauri::async_runtime::block_on(check_once(&app));
            thread::sleep(Duration::from_secs(CHECK_INTERVAL_SECS));
        }
    });
}

/// One check against the release endpoint. Network errors leave the current
/// state untouched (transient — the next interval retries); only a definitive
/// answer flips Available/Idle.
async fn check_once(app: &AppHandle) {
    let state = app.state::<Arc<UpdaterState>>();
    if matches!(
        &*lock_phase(&state),
        Phase::Downloading(_) | Phase::Installing(_)
    ) {
        return;
    }
    let Ok(updater) = app.updater() else { return };
    match updater.check().await {
        Ok(Some(update)) => {
            let version = update.version.clone();
            *state.pending.lock().unwrap_or_else(|p| p.into_inner()) = Some(update);
            *lock_phase(&state) = Phase::Available(version.clone());
            tray::set_update_text(app, &format!("Update Echo to {version}…"), true);
        }
        Ok(None) => {
            *state.pending.lock().unwrap_or_else(|p| p.into_inner()) = None;
            *lock_phase(&state) = Phase::Idle;
            tray::set_update_text(app, IDLE_TRAY_TEXT, true);
        }
        Err(_) => {}
    }
}

/// Download and install the pending update, then flush the tracker and restart
/// into the new build. On success this never returns (`app.restart()` is `!`).
pub async fn install(app: AppHandle) -> Result<(), String> {
    let state = app.state::<Arc<UpdaterState>>().inner().clone();

    let pending = state
        .pending
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    let update = match pending {
        Some(u) => u,
        None => {
            // Manual "Check for Updates…" path: check right now.
            let updater = app.updater().map_err(|e| e.to_string())?;
            match updater.check().await.map_err(|e| e.to_string())? {
                Some(u) => u,
                None => return Err("No update available.".into()),
            }
        }
    };

    let version = update.version.clone();
    state.downloaded.store(0, Ordering::SeqCst);
    state.total.store(0, Ordering::SeqCst);
    *lock_phase(&state) = Phase::Downloading(version.clone());
    tray::set_update_text(&app, "Downloading update…", false);

    let progress_state = state.clone();
    let finish_state = state.clone();
    let finish_version = version.clone();
    let result = update
        .download_and_install(
            move |chunk, total| {
                progress_state
                    .downloaded
                    .fetch_add(chunk as u64, Ordering::SeqCst);
                if let Some(t) = total {
                    progress_state.total.store(t, Ordering::SeqCst);
                }
            },
            move || {
                *lock_phase(&finish_state) = Phase::Installing(finish_version.clone());
            },
        )
        .await;

    if let Err(e) = result {
        let message = e.to_string();
        *lock_phase(&state) = Phase::Error(message.clone());
        tray::set_update_text(&app, &format!("Update Echo to {version}…"), true);
        return Err(message);
    }

    // The new build is on disk. Pause + flush before restarting so the
    // in-progress segment is never lost (mirrors tray::toggle_pause).
    let track = app.state::<Arc<TrackerState>>();
    let db = app.state::<DbState>();
    flush_for_restart(&track, &db, chrono::Utc::now().timestamp());
    app.restart();
}

/// Pause tracking and persist the open segment ahead of a process restart.
/// Order matters: set `paused` first — the poller re-checks it inside the
/// `current` lock — then close, so a racing tick can't reopen.
pub fn flush_for_restart(track: &TrackerState, db: &DbState, now: i64) {
    track.paused.store(true, Ordering::SeqCst);
    track.close_current(db, now);
}

/// Tray "Check for Updates…" / "Update Echo to X…" click handler.
pub fn on_tray_clicked(app: &AppHandle) {
    let state = app.state::<Arc<UpdaterState>>();
    let available = matches!(&*lock_phase(&state), Phase::Available(_));
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if available {
            let _ = install(app).await;
        } else {
            check_once(&app).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::poller;
    use crate::tracker::{FrontApp, OpenSegment};
    use std::path::Path;

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
    fn flush_for_restart_persists_segment_and_blocks_reopen() {
        let db = mem_db();
        let t = TrackerState::default();
        *t.current.lock().unwrap() = Some(OpenSegment {
            start_ts: 100,
            app: front("A"),
        });

        flush_for_restart(&t, &db, 130);

        assert_eq!(segments(&db), vec![(100, 130, Some("A".to_string()))]);
        assert!(t.paused.load(Ordering::SeqCst));
        assert!(t.current.lock().unwrap().is_none());

        // A tick that raced past the paused check and still read a frontmost
        // app must not record or reopen anything before the restart.
        poller::process_tick(&t, &db, Some(front("B")), 132, 130);
        assert_eq!(segments(&db).len(), 1);
        assert!(t.current.lock().unwrap().is_none());
    }

    #[test]
    fn update_status_wire_shape_matches_api_ts() {
        let to = |s: &UpdateStatus| serde_json::to_value(s).unwrap();

        assert_eq!(
            to(&UpdateStatus::Idle),
            serde_json::json!({"state": "idle"})
        );
        assert_eq!(
            to(&UpdateStatus::Available {
                version: "0.1.42".into()
            }),
            serde_json::json!({"state": "available", "version": "0.1.42"})
        );
        assert_eq!(
            to(&UpdateStatus::Downloading {
                version: "0.1.42".into(),
                downloaded: 1024,
                total: None,
            }),
            serde_json::json!({
                "state": "downloading", "version": "0.1.42",
                "downloaded": 1024, "total": null
            })
        );
        assert_eq!(
            to(&UpdateStatus::Installing {
                version: "0.1.42".into()
            }),
            serde_json::json!({"state": "installing", "version": "0.1.42"})
        );
        assert_eq!(
            to(&UpdateStatus::Error {
                message: "boom".into()
            }),
            serde_json::json!({"state": "error", "message": "boom"})
        );
    }
}
