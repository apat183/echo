# Status Bar Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Native tray menu (today total / pause / open / quit), automatic dock hiding when the window closes, pause tracking, and flush-on-quit so the in-progress segment is never lost.

**Architecture:** All Rust-side in `app/src-tauri`. A new `tracker.rs` holds shared tracking state (`paused` flag + the open segment, moved out of the poller's thread-local memory). A new `tray.rs` owns the menu and its handlers. `lib.rs` wires lifecycle: window-close hides to the menu bar (Accessory policy), `RunEvent::Exit` flushes the open segment. The existing ~2s poller tick push-updates the tray tooltip and the greyed "Today" menu line.

**Tech Stack:** Tauri 2 (tray-icon + menu APIs), Rust, rusqlite, chrono. Tests via `cargo test`. Spec: `docs/superpowers/specs/2026-06-10-status-bar-lifecycle-design.md`.

**Commands note:** Rust is installed via rustup; in shells where `cargo` is missing run `source ~/.cargo/env` first. Cargo commands run from `app/src-tauri/`. The dev app runs from `app/` with `bun run tauri dev`.

---

## File structure

- Create: `app/src-tauri/src/tracker.rs` — `FrontApp` (moved from poller), `OpenSegment`, `TrackerState` (paused + current), `close_open_segment` / `TrackerState::close_current`, `format_duration`.
- Create: `app/src-tauri/src/tray.rs` — menu construction, `TrayHandles` (managed state), menu-event handlers (pause/open/quit), `refresh_today`.
- Modify: `app/src-tauri/src/poller.rs` — extract testable `process_tick`, use shared state, call `tray::refresh_today`.
- Modify: `app/src-tauri/src/db.rs` — `DbState` alias, `day_total_seconds`, make `local_date_string`/`day_start_ts` `pub(crate)`.
- Modify: `app/src-tauri/src/lib.rs` — module wiring, window-close intercept, activation policy, exit flush.
- Modify: `DESIGN.md` — build-log entry (final task).

---

### Task 1: Baseline commit

The repo's only commit is the spec — the whole `app/` tree is untracked. Commit it as the baseline so every later task produces a reviewable diff.

**Files:**
- None modified; git only.

- [ ] **Step 1: Verify the tree builds before baselining**

Run: `source ~/.cargo/env 2>/dev/null; cd /Users/anandpatel/Development/timetracker/app/src-tauri && cargo check`
Expected: `Finished` line, no errors (warnings ok).

- [ ] **Step 2: Commit everything untracked**

```bash
cd /Users/anandpatel/Development/timetracker
git add .gitignore DESIGN.md app/
git commit -m "Baseline: Tauri timetracker app as built through entry notes"
```

Expected: commit created; `git status` shows a clean tree (besides `docs/superpowers/plans/` if not yet committed).

---

### Task 2: `tracker.rs` — shared state, close-and-write, `format_duration`

**Files:**
- Create: `app/src-tauri/src/tracker.rs`
- Modify: `app/src-tauri/src/lib.rs` (add `mod tracker;`)
- Modify: `app/src-tauri/src/poller.rs` (FrontApp moves out)
- Modify: `app/src-tauri/src/db.rs` (add `DbState` alias)
- Test: inline `#[cfg(test)]` in `tracker.rs`

- [ ] **Step 1: Add the `DbState` alias to `db.rs`**

In `app/src-tauri/src/db.rs`, after the `use` block add:

```rust
use std::sync::{Arc, Mutex};

/// Shared handle to our SQLite connection (poller thread + commands + tray).
pub type DbState = Arc<Mutex<Connection>>;
```

And in `app/src-tauri/src/lib.rs` replace:

```rust
use rusqlite::Connection;
use std::sync::{Arc, Mutex};
```
…keep those imports (still used in setup), but replace the local alias:

```rust
/// Shared handle to our SQLite connection (poller thread + commands).
type DbState = Arc<Mutex<Connection>>;
```

with:

```rust
use db::DbState;
```

- [ ] **Step 2: Write `tracker.rs` with failing tests**

Create `app/src-tauri/src/tracker.rs`:

```rust
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
    pub fn close_current(&self, db: &DbState, end_ts: i64) {
        if let Ok(mut slot) = self.current.lock() {
            close_open_segment(&mut slot, db, end_ts);
        }
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
```

- [ ] **Step 3: Register the module and move `FrontApp` out of the poller**

In `app/src-tauri/src/lib.rs`, change:

```rust
mod ax;
mod db;
mod poller;
```

to:

```rust
mod ax;
mod db;
mod poller;
mod tracker;
```

In `app/src-tauri/src/poller.rs`, delete the `FrontApp` struct definition:

```rust
#[derive(Clone, PartialEq, Eq)]
pub struct FrontApp {
    pub bundle: Option<String>,
    pub name: Option<String>,
    pub title: Option<String>,
}
```

and change the import line at the top from:

```rust
use crate::{ax, db};
```

to:

```rust
use crate::tracker::FrontApp;
use crate::{ax, db};
```

- [ ] **Step 4: Run the tests**

Run: `source ~/.cargo/env 2>/dev/null; cd /Users/anandpatel/Development/timetracker/app/src-tauri && cargo test`
Expected: PASS — `format_duration_renders_hours_and_minutes`, `close_current_writes_open_segment`, `close_current_with_nothing_open_is_a_noop` (3 passed).
(If `db::open(":memory:")` errors on the WAL pragma, switch the test helper to a unique file under `std::env::temp_dir()` — but the file-DB code path uses the same pragma successfully, so `:memory:` is expected to work.)

- [ ] **Step 5: Commit**

```bash
cd /Users/anandpatel/Development/timetracker
git add app/src-tauri/src/tracker.rs app/src-tauri/src/lib.rs app/src-tauri/src/poller.rs app/src-tauri/src/db.rs
git commit -m "Add shared tracker state with close-and-write and duration formatting"
```

---

### Task 3: `db.rs` — `day_total_seconds`

**Files:**
- Modify: `app/src-tauri/src/db.rs`
- Test: inline `#[cfg(test)]` in `db.rs`

- [ ] **Step 1: Write the failing test**

Append to `app/src-tauri/src/db.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn day_total_sums_only_segments_starting_that_day() {
        let conn = open(Path::new(":memory:")).unwrap();
        let start = day_start_ts("2026-01-15");

        // inside the day: 60s + 30s
        insert_segment(&conn, start + 100, start + 160, None, Some("A"), None).unwrap();
        insert_segment(&conn, start + 200, start + 230, None, Some("B"), None).unwrap();
        // previous day and next day: excluded
        insert_segment(&conn, start - 50, start - 10, None, Some("A"), None).unwrap();
        insert_segment(&conn, start + 86_400 + 5, start + 86_400 + 25, None, Some("A"), None).unwrap();

        assert_eq!(day_total_seconds(&conn, "2026-01-15").unwrap(), 90);
        assert_eq!(day_total_seconds(&conn, "2026-01-14").unwrap(), 40);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `source ~/.cargo/env 2>/dev/null; cd /Users/anandpatel/Development/timetracker/app/src-tauri && cargo test day_total`
Expected: FAIL to compile — `day_total_seconds` not found.

- [ ] **Step 3: Implement**

In `app/src-tauri/src/db.rs`:

Change the two helper visibilities (the poller will need `local_date_string` in Task 7; the test needs `day_start_ts`):

```rust
pub(crate) fn local_date_string(ts: i64) -> String {
```

```rust
pub(crate) fn day_start_ts(date: &str) -> i64 {
```

Add after `day_view`:

```rust
/// Total tracked seconds for one local day (segments attributed by start_ts,
/// same convention as day_view).
pub fn day_total_seconds(conn: &Connection, date: &str) -> rusqlite::Result<i64> {
    let start = day_start_ts(date);
    let end = start + 86_400;
    conn.query_row(
        "SELECT COALESCE(SUM(end_ts - start_ts), 0) FROM segments
         WHERE start_ts >= ?1 AND start_ts < ?2",
        rusqlite::params![start, end],
        |r| r.get(0),
    )
}
```

- [ ] **Step 4: Run the tests**

Run: `source ~/.cargo/env 2>/dev/null; cd /Users/anandpatel/Development/timetracker/app/src-tauri && cargo test`
Expected: PASS (4 tests total).

- [ ] **Step 5: Commit**

```bash
cd /Users/anandpatel/Development/timetracker
git add app/src-tauri/src/db.rs
git commit -m "Add day_total_seconds for the tray's Today readout"
```

---

### Task 4: Poller refactor — shared state + testable `process_tick`

**Files:**
- Modify: `app/src-tauri/src/poller.rs`
- Modify: `app/src-tauri/src/lib.rs` (manage `TrackerState`, new `spawn` signature)
- Test: inline `#[cfg(test)]` in `poller.rs`

- [ ] **Step 1: Write the failing tests**

Append to `app/src-tauri/src/poller.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `source ~/.cargo/env 2>/dev/null; cd /Users/anandpatel/Development/timetracker/app/src-tauri && cargo test`
Expected: FAIL to compile — `process_tick` not found.

- [ ] **Step 3: Rewrite the poller around `process_tick`**

Replace the entire body of `app/src-tauri/src/poller.rs` below the doc comment with:

```rust
use crate::tracker::{self, FrontApp, OpenSegment, TrackerState};
use crate::{ax, db};
use chrono::Utc;
use db::DbState;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

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
    let Ok(mut current) = tracker.current.lock() else {
        return;
    };

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
pub fn spawn(db: DbState, tracker: Arc<TrackerState>) {
    thread::spawn(move || {
        let mut last_tick = Utc::now().timestamp();
        loop {
            thread::sleep(Duration::from_secs(POLL_SECS));
            let now = Utc::now().timestamp();
            // Skip the AX/NSWorkspace work entirely while paused.
            let front = if tracker.paused.load(Ordering::SeqCst) {
                None
            } else {
                read_frontmost()
            };
            process_tick(&tracker, &db, front, now, last_tick);
            last_tick = now;
        }
    });
}
```

(The old doc comment at the top of the file stays; update its last sentence — titles are no longer "out of scope here": the comment block becomes:)

```rust
//! App-level foreground poller (DESIGN.md decisions #3, #4, #6).
//!
//! Every ~2s we read the frontmost application. The *current* segment lives in
//! shared TrackerState (so pause/quit can flush it); a row is written only when
//! the frontmost app+title changes — segments, not point-samples.
```

- [ ] **Step 4: Wire the new state in `lib.rs`**

In `app/src-tauri/src/lib.rs`, add to the imports:

```rust
use tracker::TrackerState;
```

In `setup`, replace:

```rust
            let shared: DbState = Arc::new(Mutex::new(conn));
            app.manage(shared.clone());

            poller::spawn(shared);
```

with:

```rust
            let shared: DbState = Arc::new(Mutex::new(conn));
            app.manage(shared.clone());

            let track = Arc::new(TrackerState::default());
            app.manage(track.clone());

            poller::spawn(shared, track);
```

- [ ] **Step 5: Run all tests**

Run: `source ~/.cargo/env 2>/dev/null; cd /Users/anandpatel/Development/timetracker/app/src-tauri && cargo test`
Expected: PASS — 8 tests (3 tracker, 1 db, 4 poller).

- [ ] **Step 6: Quick behavioral sanity check in dev**

Run: `cd /Users/anandpatel/Development/timetracker/app && bun run tauri dev` (let it run ~30s, switch apps once, then Ctrl-C)
Expected: app launches, no panics; switching apps still lands segments (check the Activities view shows today's time).

- [ ] **Step 7: Commit**

```bash
cd /Users/anandpatel/Development/timetracker
git add app/src-tauri/src/poller.rs app/src-tauri/src/lib.rs
git commit -m "Move open segment into shared TrackerState; extract testable process_tick"
```

---

### Task 5: `tray.rs` — menu, pause/open/quit handlers

**Files:**
- Create: `app/src-tauri/src/tray.rs`
- Modify: `app/src-tauri/src/lib.rs` (replace bare tray with `tray::init`, register module)

No unit tests — this is Tauri API wiring; behavior is verified manually in Step 3 and at the end of Task 6.

- [ ] **Step 1: Create `app/src-tauri/src/tray.rs`**

```rust
//! Status-bar (tray) icon: menu, handlers, and live "Today" readout.
//!
//! Menu layout (spec §1):
//!   Today: 4h 12m          (disabled)
//!   ─────────────────────
//!   Pause time tracking    (toggles to "Resume time tracking")
//!   Open TimeTracker
//!   ─────────────────────
//!   Quit TimeTracker
//!
//! macOS has no "menu about to open" hook in Tauri, so the Today line and the
//! tooltip are push-updated from the poller tick (refresh_today).

use crate::db::DbState;
use crate::tracker::{self, TrackerState};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager};

pub struct TrayHandles {
    tray: TrayIcon,
    today_item: MenuItem<tauri::Wry>,
    pause_item: MenuItem<tauri::Wry>,
    /// Last rendered "Today: Xh Ym" string, to skip no-op updates.
    last_today: Mutex<String>,
}

pub fn init(app: &tauri::App) -> tauri::Result<()> {
    let today_item = MenuItem::with_id(app, "today", "Today: 0m", false, None::<&str>)?;
    let pause_item = MenuItem::with_id(app, "pause", "Pause time tracking", true, None::<&str>)?;
    let open_item = MenuItem::with_id(app, "open", "Open TimeTracker", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit TimeTracker", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[&today_item, &sep1, &pause_item, &open_item, &sep2, &quit_item],
    )?;

    let tray = TrayIconBuilder::new()
        .icon(app.default_window_icon().expect("default icon").clone())
        .tooltip("TimeTracker — tracking")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu_event)
        .build(app)?;

    app.manage(TrayHandles {
        tray,
        today_item,
        pause_item,
        last_today: Mutex::new(String::new()),
    });
    Ok(())
}

fn on_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        "pause" => toggle_pause(app),
        "open" => open_window(app),
        "quit" => app.exit(0), // flush happens in RunEvent::Exit (lib.rs)
        _ => {}
    }
}

fn toggle_pause(app: &AppHandle) {
    let track = app.state::<Arc<TrackerState>>();
    let handles = app.state::<TrayHandles>();
    let now_paused = !track.paused.load(Ordering::SeqCst);
    track.paused.store(now_paused, Ordering::SeqCst);

    if now_paused {
        // Close the in-progress segment right now so paused time is unbilled.
        let db = app.state::<DbState>();
        track.close_current(&db, chrono::Utc::now().timestamp());
        let _ = handles.pause_item.set_text("Resume time tracking");
        let _ = handles.tray.set_tooltip(Some("TimeTracker — paused"));
    } else {
        let _ = handles.pause_item.set_text("Pause time tracking");
        let _ = handles.tray.set_tooltip(Some("TimeTracker — tracking"));
        // Force the next tick to re-render tooltip with the live total.
        if let Ok(mut last) = handles.last_today.lock() {
            last.clear();
        }
    }
}

fn open_window(app: &AppHandle) {
    // Back into the dock / Cmd-Tab before showing, for proper focus.
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Called from the poller tick. Updates the greyed Today line (always) and the
/// tooltip (only while tracking; the paused tooltip is owned by toggle_pause).
pub fn refresh_today(app: &AppHandle, total_seconds: i64, paused: bool) {
    let Some(handles) = app.try_state::<TrayHandles>() else {
        return;
    };
    let today = format!("Today: {}", tracker::format_duration(total_seconds));
    let Ok(mut last) = handles.last_today.lock() else {
        return;
    };
    if *last != today {
        *last = today.clone();
        let _ = handles.today_item.set_text(&today);
        if !paused {
            let _ = handles
                .tray
                .set_tooltip(Some(format!("TimeTracker — {today}")));
        }
    }
}
```

Note on `&db`: `app.state::<DbState>()` returns `State<DbState>`; `track.close_current(&db, …)` needs `&DbState` — write `track.close_current(&db, ts)` only if deref coercion applies; otherwise use `&*db`. Same for any `State` passed to a `&DbState` parameter. Prefer `&db` first; if the compiler complains, switch to `&*db`.

API fallbacks if the resolved tauri 2.x is older: `show_menu_on_left_click` was previously `menu_on_left_click` (same argument); `event.id()` can also be written `event.id` (public field).

- [ ] **Step 2: Wire into `lib.rs`**

In `app/src-tauri/src/lib.rs`:

Add the module:

```rust
mod ax;
mod db;
mod poller;
mod tracker;
mod tray;
```

Remove the import `use tauri::tray::TrayIconBuilder;`.

In `setup`, replace:

```rust
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().expect("default icon").clone())
                .tooltip("TimeTracker — tracking")
                .build(app)?;
```

with:

```rust
            tray::init(app)?;
```

Make sure `tray::init(app)?` runs **before** `poller::spawn(...)` in `setup` (the poller will look up `TrayHandles` from Task 7 on).

- [ ] **Step 3: Build and manually verify the menu**

Run: `cd /Users/anandpatel/Development/timetracker/app && bun run tauri dev`
Expected, clicking the tray icon:
- Menu shows: greyed "Today: 0m", separator, "Pause time tracking", "Open TimeTracker", separator, "Quit TimeTracker".
- "Pause time tracking" → item text flips to "Resume time tracking"; switch apps for ~10s; the Activities view gains no new time. Click "Resume time tracking" → tracking continues.
- "Open TimeTracker" → window comes to front.
- "Quit TimeTracker" → app exits.

- [ ] **Step 4: Commit**

```bash
cd /Users/anandpatel/Development/timetracker
git add app/src-tauri/src/tray.rs app/src-tauri/src/lib.rs
git commit -m "Add tray menu: today line, pause/resume, open, quit"
```

---

### Task 6: Lifecycle — hide-on-close, dock policy, flush-on-quit

**Files:**
- Modify: `app/src-tauri/src/lib.rs`

No unit tests — OS lifecycle wiring; verified manually in Step 3.

- [ ] **Step 1: Intercept window close → hide to menu bar**

In `app/src-tauri/src/lib.rs`, on the builder chain (after `.plugin(...)`, before `.setup(...)`), add:

```rust
        .on_window_event(|window, event| {
            // Red close button = hide to the menu bar, keep tracking (spec §2).
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                #[cfg(target_os = "macos")]
                let _ = window
                    .app_handle()
                    .set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
        })
```

- [ ] **Step 2: Flush the open segment on every exit path**

Still in `lib.rs`, replace the end of `run()`:

```rust
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
```

with:

```rust
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Single flush point for every exit path (tray Quit, Cmd-Q, …):
            // the in-progress segment must never be lost (spec §4).
            if let tauri::RunEvent::Exit = event {
                let track = app_handle.state::<Arc<TrackerState>>();
                let db = app_handle.state::<DbState>();
                track.close_current(&db, chrono::Utc::now().timestamp());
            }
        });
```

(`use tauri::Manager;` is already imported. If `&db` doesn't coerce to `&DbState`, use `&*db`.)

- [ ] **Step 3: Build and verify the full lifecycle manually**

Run: `cd /Users/anandpatel/Development/timetracker/app && bun run tauri dev`
Verify, in order:
1. Close the window with the red button → window disappears, **dock icon disappears**, app is gone from Cmd-Tab, tray icon remains.
2. Use other apps ~30s → tray menu still opens; "Open TimeTracker" → window AND dock icon return, window focused; Activities view shows the time tracked while hidden.
3. Stay in one app ≥1 min, then tray → "Quit TimeTracker". Relaunch (`bun run tauri dev`): the Activities view includes that final stretch up to the quit moment — the flush worked (before this task it would have been silently lost).

- [ ] **Step 4: Run tests (regression)**

Run: `source ~/.cargo/env 2>/dev/null; cd /Users/anandpatel/Development/timetracker/app/src-tauri && cargo test`
Expected: PASS (8 tests).

- [ ] **Step 5: Commit**

```bash
cd /Users/anandpatel/Development/timetracker
git add app/src-tauri/src/lib.rs
git commit -m "Hide to menu bar on close, drop dock icon, flush segment on exit"
```

---

### Task 7: Live "Today" readout in tray (tooltip + greyed menu line)

**Files:**
- Modify: `app/src-tauri/src/poller.rs` (`spawn` gains an `AppHandle`, tick computes total + calls `tray::refresh_today`)
- Modify: `app/src-tauri/src/lib.rs` (pass the handle)

- [ ] **Step 1: Compute the running total in the poller tick**

In `app/src-tauri/src/poller.rs`, change the imports:

```rust
use crate::tracker::{self, FrontApp, OpenSegment, TrackerState};
use crate::{ax, db, tray};
```

and add `use tauri::AppHandle;` to the imports.

Change `spawn` to:

```rust
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
```

- [ ] **Step 2: Pass the handle in `lib.rs`**

In `setup`, change:

```rust
            poller::spawn(shared, track);
```

to:

```rust
            poller::spawn(app.handle().clone(), shared, track);
```

(`shared` here is the clone created in setup; `app.manage(shared.clone())` on the line above keeps its own.)

- [ ] **Step 3: Run tests (regression)**

Run: `source ~/.cargo/env 2>/dev/null; cd /Users/anandpatel/Development/timetracker/app/src-tauri && cargo test`
Expected: PASS (8 tests — `process_tick` untouched).

- [ ] **Step 4: Manual verification**

Run: `cd /Users/anandpatel/Development/timetracker/app && bun run tauri dev`
Expected:
- Within ~2s the tray menu's greyed line reads today's real total (matches the Activities view, within a minute).
- Hovering the tray icon shows `TimeTracker — Today: Xh Ym`.
- After ≥1 min of use, the readout advances.
- Pause → tooltip shows `TimeTracker — paused`; the Today line stays put. Resume → tooltip returns to the live total within ~2s.

- [ ] **Step 5: Commit**

```bash
cd /Users/anandpatel/Development/timetracker
git add app/src-tauri/src/poller.rs app/src-tauri/src/lib.rs
git commit -m "Push live Today total to tray tooltip and menu line"
```

---

### Task 8: Final verification + DESIGN.md build log

**Files:**
- Modify: `DESIGN.md`

- [ ] **Step 1: Full test suite**

Run: `source ~/.cargo/env 2>/dev/null; cd /Users/anandpatel/Development/timetracker/app/src-tauri && cargo test`
Expected: PASS, 8 tests, 0 failures.

- [ ] **Step 2: Full manual pass (spec §Testing)**

Run: `cd /Users/anandpatel/Development/timetracker/app && bun run tauri dev`
- Close window → dock icon gone, tracking continues. ✓
- Tray → Open → window + dock return. ✓
- Pause → segment written at pause instant; nothing recorded while paused. ✓
- Resume → recording resumes. ✓
- Quit mid-segment → after relaunch, segment present up to quit time. ✓

- [ ] **Step 3: Append to DESIGN.md's build log**

Add at the end of the "Build log & refinements" section of `DESIGN.md`:

```markdown
**Session — status bar lifecycle (done & verified):** Native tray menu
(greyed live "Today: Xh Ym" line + tooltip, Pause/Resume time tracking, Open,
Quit). Closing the window now hides to the menu bar and drops the dock icon
(ActivationPolicy::Accessory); Open restores both. The open segment moved into
shared `TrackerState` (`tracker.rs`), so pause and quit flush it —
**fixing silent loss of the in-progress segment on quit**. Pause is in-memory
only; every launch starts tracking. `tray.rs` owns the menu; the ~2s poller
tick push-updates the readout. Spec:
`docs/superpowers/specs/2026-06-10-status-bar-lifecycle-design.md`.
```

- [ ] **Step 4: Commit**

```bash
cd /Users/anandpatel/Development/timetracker
git add DESIGN.md
git commit -m "Record status bar lifecycle work in DESIGN.md build log"
```

---

## Self-review notes

- **Spec coverage:** §1 tray menu → Tasks 5 & 7; §2 dock lifecycle → Task 6 (+ open handler in Task 5); §3 pause → Tasks 4 & 5; §4 flush-on-quit → Tasks 2 & 6; shared-state shape → Task 2/4; testing section → unit tests in Tasks 2–4, manual passes in Tasks 5–8.
- **Known API-version contingencies** (called out inline where they apply): `show_menu_on_left_click` vs older `menu_on_left_click`; `event.id()` vs `event.id`; `&db` vs `&*db` deref of `State<DbState>`; `:memory:` WAL pragma fallback to a temp file.
- Menu/tray handles are stored in managed state and touched from the poller thread — Tauri 2's menu/tray types proxy to the main thread and are `Send + Sync`; if a build error contradicts that, route updates through `app.run_on_main_thread` instead.
