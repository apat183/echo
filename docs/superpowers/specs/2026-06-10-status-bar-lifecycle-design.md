# Status Bar Lifecycle — Design

**Date:** 2026-06-10
**Status:** Approved
**Scope:** Tray menu, automatic dock hiding, pause tracking, flush-on-quit.
Icons (app + tray) and manual entries are explicitly deferred.

## Goal

Make TimeTracker behave like a real menu-bar app: closing the window doesn't kill
tracking, the tray icon gets a useful menu, tracking can be paused, and quitting
never loses the in-progress segment.

## Approach (decided)

Everything native, Rust-side (`lib.rs` + `poller.rs`). Tray menu via Tauri's Menu
API; pause is shared state the poller checks; window-close intercepted to hide
instead of quit; tooltip/menu text refreshed from the existing ~2s poller tick.
No frontend changes. Rejected: forwarding tray events to the React frontend (the
webview may not be loaded while hidden) and a custom popover panel (overkill).

## Components

### 1. Tray menu

Native macOS menu attached to the existing tray icon, opens on click:

```
Today: 4h 12m          (disabled / greyed out)
──────────────────────
Pause time tracking    (toggles to "Resume time tracking")
Open TimeTracker
──────────────────────
Quit TimeTracker
```

- **Today line** and the tray **tooltip** (`TimeTracker — Today: 4h 12m`, or
  `TimeTracker — paused` while paused) are push-updated from the poller tick,
  only when the rendered text changes. Tauri has no "menu about to open" hook on
  macOS, so push-updating is the reliable mechanism.
- "Today" = sum of today's segment durations (local timezone), same definition
  as the Activities day view.

### 2. Window / dock lifecycle (automatic dock hiding)

- Launch: unchanged — window visible, dock icon present (`ActivationPolicy::Regular`).
- Red close button: intercept `CloseRequested`, prevent close, hide the window,
  switch to `ActivationPolicy::Accessory` (drops out of dock and Cmd+Tab).
  Tracking continues.
- "Open TimeTracker": switch back to `Regular` (before showing, for proper
  focus), then show + focus the window.
- No persisted setting — dock presence simply follows window visibility.

### 3. Pause / resume

- "Pause time tracking": closes the current segment at that instant (written to
  DB); the poller keeps ticking but records nothing; the menu item flips to
  "Resume time tracking"; tooltip shows paused.
- "Resume time tracking": next tick opens a fresh segment.
- Pause is in-memory only. Every launch starts tracking — a forgotten pause must
  not silently lose days of data.

### 4. Quit & flush-on-quit (bug fix)

Today the poller keeps the current segment only in its thread-local memory and
writes it when the frontmost app *changes* — so quitting (currently: closing the
window) silently discards the entire in-progress segment, potentially 30+
minutes.

Fix: move the current segment into shared state (`Mutex`) owned alongside the DB
handle. The poller updates it; "Quit TimeTracker" closes the open segment
(end = now), writes it, then exits. Pause uses the same close-and-write path.

## Shared state shape

One `TrackerState` (managed by Tauri, also given to the poller thread):

- `paused: AtomicBool`
- `current: Mutex<Option<(start_ts, FrontApp)>>` — moved out of the poller loop
- existing `DbState` (`Arc<Mutex<Connection>>`) stays as-is

A single `close_current(end_ts)` helper does close-and-write; used by the
poller (app change, sleep gap), pause, and quit.

## Error handling

- DB write failures on flush: best-effort (matches existing poller behavior);
  quit proceeds regardless.
- Tray/menu build failure at setup: propagate (app is useless without it in
  hidden mode).

## Testing

- Unit-testable pure parts: "Today" total formatting (`4h 12m`), close-current
  segment logic.
- Manual verification: close window → dock icon gone, tracking continues;
  Open → window + dock return; pause → segment written, nothing recorded while
  paused; resume → recording resumes; quit mid-segment → segment present in DB
  up to quit time.

## Out of scope

App icon, tray icon design (template image), manual entries, launch-at-login,
persisted pause, custom popover panel.
