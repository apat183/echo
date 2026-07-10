# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Echo is a personal, macOS-first, local-only menu-bar time tracker built with **Tauri 2 + React 19 + TypeScript** (frontend) and **Rust + SQLite** (backend). It records the foreground app and window title, then lets you group that time into projects after the fact. No network, no accounts, no billing/timesheet concepts — see `README.md`.

The package manager is **Bun**. The Rust crate is `echo` (package) / `echo_lib` (lib); `src-tauri/src/main.rs` just calls `echo_lib::run()` in `lib.rs`.

## Commands

```sh
bun install                 # install JS deps
bun run tauri dev           # run the full app (spawns Vite on :1420, then Tauri)
bun run dev                 # Vite only (frontend in a browser, no Tauri APIs)
bun run build               # tsc typecheck + Vite production build into dist/

cd src-tauri && cargo test            # all Rust tests
cd src-tauri && cargo test paused     # single test / filter by name substring
cd src-tauri && cargo build           # compile Rust without running

bun run test                # frontend unit tests (Vitest)
bun run test:coverage       # frontend coverage report
bun run test period         # single frontend file / filter by name substring
bun run mask.js             # regenerate app + tray icons from app-icon.png (needs `tauri icon`)
```

Two test suites. **Rust** unit tests live in `#[cfg(test)]` modules (`poller.rs`, `tracker.rs`, `db.rs`) and cover the tracking state machine and the SQLite aggregation/resolution logic. **Frontend** tests (Vitest) cover pure logic plus high-value React component flows. Run coverage with `bun run test:coverage`. There is no frontend linter; typechecking happens via `tsc` inside `bun run build`.

## Architecture

### The segment model (this is the core idea)

The `segments` table is **append-only, immutable ground truth**: one row per continuous stretch the same app+title was frontmost (`start_ts`, `end_ts`, bundle id, name, title). It is *never* point-sampled — a row is written only when the foreground app/title *changes*.

Data flow:
- `poller.rs` runs a background thread, ticking every 2s (`read_frontmost` via `NSWorkspace` + `ax.rs` for the title).
- The **currently-open segment lives in shared `TrackerState` (`tracker.rs`)**, not thread-local, specifically so pause and quit can flush it. `process_tick` closes-and-writes the old segment and opens a new one only on change.
- Everything downstream (`db.rs` aggregations) reads `segments` and rolls it up on demand.

Two invariants the tests guard — preserve them when touching the poller/tracker:
1. **Lock order is always `current` → `db`.** Callers hold the `current` mutex, then `close_open_segment` grabs `db`. Don't invert this.
2. **The in-progress segment must never be lost or over-billed.** Pause flips `paused` *then* closes the segment, and the poller re-checks `paused` *inside* the `current` lock so a racing tick can't reopen. Sleep/lid-close gaps (`> SLEEP_GAP_SECS`) close at the last good tick so suspended time isn't billed. There is a **single flush point for every exit path** in `lib.rs`'s `RunEvent::Exit` handler — tray Quit, Cmd-Q, etc. all funnel through it.

### Time attribution & the assignment model

- A segment is attributed to the **local date of its `start_ts`** (`local_date_string` / `day_start_ts` in `db.rs`). All views use this convention consistently.
- Each row is read through the private **`Segment`** type in `db.rs`, which owns the domain derivations in one place: `key()` (the stable assignment key = bundle id, else name, else `"unknown"`), `display_name()`, `title()`, `duration()`, `local_date()`. Aggregations iterate `read_segments(...)` rather than re-deriving these inline.
- Projects are buckets. Time is linked to projects via **`day_assignments`**, keyed `(date, app_key, title, project_id)` — this is **many-to-many**: one app/title row on a given day can carry multiple project tags, and each tag bills the full duration (overlapping totals by design). Dragging an app/title onto a project in the UI assigns only the selected day(s), not all history. Use `add_assignment` / `remove_assignment`; there is no single-project `set_assignment`.
- `title = ""` is the **app-level** assignment: a fallback covering every title not assigned on its own. The resolution rule lives in one place — the private **`Assignments`** module in `db.rs`: `links(date, key, title)` returns the set of project ids for the exact link (what the day view shows per row, so inheritance renders) and `resolve(date, key, title)` adds the app-level fallback (what `project_breakdown` / `project_apps` bill each segment to). Don't re-open-code this fallback at call sites.
- Projects have a user-controlled `sort_order`; the `set_project_order` command reorders them. The frontend sends the full ordered id list; the backend updates each row's `sort_order` in a transaction.
- `project_period_notes` attaches notes to project day/week/month buckets. Week views group day notes underneath the week note; month views group week notes underneath the month note. The old per-entry notes concept is retired.
- Ignored activity is held in `ignored_entries` and excluded from Activities until the user removes the ignored rule.

The Rust structs in `db.rs` are `#[derive(Serialize)]` and mirror the TS types in `src/api.ts` — keep these two files in sync when changing the wire shape. `api.ts` is the **only** place that calls `invoke`; the rest of the React app goes through the `api` object.

### Frontend layout

`src/api.ts` holds the `invoke` wrappers, shared wire types, and small pure helpers (`fmtDur`, `weekKey`, …). The view logic is split into pure, testable modules with no React: **`period.ts`** (day/week/month calendar math + `rollup`), **`merge.ts`** (`mergeDayViews` folds N `DayView`s into one `PeriodView`), and **`drag.ts`** (the `DragPayload` encode/decode for assigning time by drag). React components live in `src/components/` (`Sidebar`, `DayPane`, `ProjectPane`, `Segmented`, `charts`); `App.tsx` is just the root that wires the sidebar selection to the two panes. App icons load through `appIcon.ts` (memoised per bundle id).

### macOS menu-bar lifecycle

Echo is a menu-bar app, and several behaviors depend on macOS `ActivationPolicy`:
- Clicking the window's red close button does **not** quit — `lib.rs`'s `on_window_event` intercepts `CloseRequested` (main window only), hides the window, and switches to `ActivationPolicy::Accessory` (drops the dock icon, keeps tracking).
- "Open Echo" from the tray switches back to `ActivationPolicy::Regular` before showing, so focus/Cmd-Tab work (`tray.rs`).
- The tray "Today: Xh Ym" line and tooltip are **push-updated from the poller tick** (`tray::refresh_today`), because Tauri has no "menu about to open" hook on macOS. `last_today` dedupes no-op updates.
- The window uses `titleBarStyle: "Overlay"` + `hiddenTitle: true` (`tauri.conf.json`): the native title bar is hidden and macOS traffic-light buttons overlay the top-left ~64×28 px of the web content. The sidebar's `.sidebar-titlebar` strip (34 px, `data-tauri-drag-region`) sits behind the traffic lights; the `.sidebar-brand` row and the `.pane-header` elements also carry `data-tauri-drag-region` so the entire top edge is draggable. Child buttons (collapse toggle, nav controls) must NOT carry the attribute.

### Window titles need Accessibility permission

`ax.rs` reads the focused window title via the macOS Accessibility (AX) C API (`AXUIElementCopyAttributeValue`). This is **best-effort**: without the permission (or for apps exposing no title) it returns `None` and tracking degrades gracefully to app-level only. The `ax_status` / `ax_request` commands drive the permission UX. All macOS-specific code is `#[cfg(target_os = "macos")]`-gated with non-macOS stubs, so the crate still compiles elsewhere (with tracking inert).

Because builds are **ad-hoc signed**, every installed update changes the code hash and silently invalidates the TCC Accessibility grant (System Settings still shows Echo enabled, but `AXIsProcessTrusted()` is false and the prompt won't re-fire). The ax banner in `DayPane` covers this: `ax_open_settings` deep-links to the Accessibility pane and the copy tells the user to toggle Echo off and on.

### Auto-update

The update flow is Rust-owned in `updater.rs`, mirroring existing patterns: the frontend polls the `update_status` command (like `ax_status`), the tray "Check for Updates…" item is push-updated (like `refresh_today`), and a background thread checks the release endpoint every 6h (release builds only — a debug build reports the committed `0.1.0` and would always see an "update"). Install is user-triggered (banner or tray), never silent. Before `app.restart()` the tracker is paused and the open segment flushed via `flush_for_restart` — `restart()` can skip `RunEvent::Exit`, so the exit-handler flush alone is not relied on (a double flush is a no-op). CI stamps the real version `0.1.<run_number>` via `tauri build --config` injection; the committed version stays `0.1.0` and local builds never need the updater signing key. The release workflow uploads `Echo.app.tar.gz` + `.sig` and composes `latest.json` before the draft release flips public.

### Adding a Tauri command

1. Write the `db.rs` function (pure, takes `&Connection`).
2. Add a `#[tauri::command]` wrapper in `lib.rs` that locks `DbState` and maps errors to `String`.
3. Register it in the `generate_handler!` list in `lib.rs`.
4. Add the typed wrapper + any shared types in `src/api.ts`.

## Notes

- Source comments reference a design spec (`spec §1`, `#7`, etc.). That `DESIGN.md` / `docs/` spec has been removed from the repo — treat the comments as historical pointers, not live docs.
- The project was recently flattened from an `app/` subdirectory to the repo root; ignore stale `app/...` paths in older git history.
