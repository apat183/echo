//! Opt-in startup/runtime timing, off unless `ECHO_PERF=1`.
//!
//! Cold-start freezes are hard to attribute by feel: the webview spin-up, the
//! first DB query, the icon fetches and the auto-delete sweep all land in the
//! same second. Every line printed here carries **elapsed-since-process-start**
//! so the ordering is readable, plus the duration of the thing being measured.
//!
//! Nothing here is in the hot path when disabled: `enabled()` is a single
//! relaxed atomic load, and every helper short-circuits on it.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

static ENABLED: AtomicBool = AtomicBool::new(false);
static START: OnceLock<Instant> = OnceLock::new();
/// Guards the "time to first IPC" line so it prints once, not per command.
static FIRST_IPC: AtomicBool = AtomicBool::new(false);
/// Rolling totals for the two things we expect to dominate, so a burst of 51
/// icon calls reports as one number instead of 51 lines to add up by hand.
static ICON_TOTAL_US: AtomicU64 = AtomicU64::new(0);
static ICON_COUNT: AtomicU64 = AtomicU64::new(0);
static DAYVIEW_TOTAL_US: AtomicU64 = AtomicU64::new(0);
static DAYVIEW_COUNT: AtomicU64 = AtomicU64::new(0);

/// Call once, first thing in `run()`, to anchor the clock.
pub fn init() {
    START.get_or_init(Instant::now);
    let on = std::env::var("ECHO_PERF")
        .map(|v| v == "1")
        .unwrap_or(false);
    ENABLED.store(on, Ordering::Relaxed);
    if on {
        eprintln!("[perf] enabled (ECHO_PERF=1)");
    }
}

#[inline]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

fn since_start_ms() -> f64 {
    START
        .get()
        .map(|t| t.elapsed().as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

/// A point-in-time event, e.g. "setup done".
pub fn mark(label: &str) {
    if !enabled() {
        return;
    }
    eprintln!("[perf] {:>8.1}ms  · {label}", since_start_ms());
}

/// Time `f`, logging its duration. Returns whatever `f` returns, so it wraps an
/// existing expression without restructuring the call site.
pub fn time<T>(label: &str, f: impl FnOnce() -> T) -> T {
    if !enabled() {
        return f();
    }
    let t0 = Instant::now();
    let out = f();
    eprintln!(
        "[perf] {:>8.1}ms  · {label} took {:.1}ms",
        since_start_ms(),
        t0.elapsed().as_secs_f64() * 1000.0
    );
    out
}

/// The first command to reach Rust. The gap between process start and this line
/// is webview spin-up + JS bundle parse + React first render — i.e. the part of
/// a cold start that no amount of backend work can fix.
pub fn note_first_ipc(command: &str) {
    if !enabled() || FIRST_IPC.swap(true, Ordering::Relaxed) {
        return;
    }
    eprintln!(
        "[perf] {:>8.1}ms  · FIRST IPC ({command}) — everything before this is webview startup",
        since_start_ms()
    );
}

/// Record one `app_icon_data_url` call. `bytes` is the encoded data-URL length —
/// worth logging next to the duration, because the two regress together: the
/// slow path here was always the one that also shipped a megabyte over IPC.
pub fn record_icon(micros: u64, bytes: usize) {
    if !enabled() {
        return;
    }
    let total = ICON_TOTAL_US.fetch_add(micros, Ordering::Relaxed) + micros;
    let n = ICON_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!(
        "[perf] {:>8.1}ms  · icon #{n} took {:.1}ms, {:.1}KB (total {:.1}ms)",
        since_start_ms(),
        micros as f64 / 1000.0,
        bytes as f64 / 1024.0,
        total as f64 / 1000.0
    );
}

/// Record one `get_day_view` call: how long we waited for the DB mutex, and how
/// long the query itself took once we had it.
pub fn record_day_view(date: &str, lock_wait_us: u64, query_us: u64) {
    if !enabled() {
        return;
    }
    let total = DAYVIEW_TOTAL_US.fetch_add(lock_wait_us + query_us, Ordering::Relaxed)
        + lock_wait_us
        + query_us;
    let n = DAYVIEW_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!(
        "[perf] {:>8.1}ms  · day_view #{n} {date} lock {:.1}ms + query {:.1}ms (total {:.1}ms)",
        since_start_ms(),
        lock_wait_us as f64 / 1000.0,
        query_us as f64 / 1000.0,
        total as f64 / 1000.0
    );
}

/// Time the acquisition of a lock separately from the work done under it.
/// Returns `(guard, wait_micros)`.
pub fn timed_lock<T>(
    lock: &std::sync::Mutex<T>,
) -> Result<(std::sync::MutexGuard<'_, T>, u64), String> {
    let t0 = Instant::now();
    let guard = lock.lock().map_err(|e| e.to_string())?;
    Ok((guard, t0.elapsed().as_micros() as u64))
}
