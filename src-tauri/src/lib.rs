mod ax;
mod db;
mod perf;
mod poller;
mod tracker;
mod tray;
mod updater;

use db::{DayTotal, DayView, DbState, IgnoredEntry, Project, ProjectApp, ProjectPeriodNote};
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};
use tracker::TrackerState;

/// SQLite database filename inside the app data dir. WAL mode adds `-wal`/`-shm`
/// siblings. Single source of truth for the DB path (open + size).
const DB_FILENAME: &str = "echo.sqlite3";

// Every command below is `#[tauri::command(async)]`, deliberately and without
// exception. A plain `#[tauri::command]` runs its body on the **main thread**,
// so any handler that locks the DB, scans segments or touches AppKit freezes the
// UI for its whole duration. The cost of `(async)` on a cheap command is a task
// dispatch; the cost of forgetting it on a command that later grows a table scan
// is a visible hang — so the rule is blanket, not case-by-case. `install_update`
// is already an `async fn` and needs no annotation.

#[tauri::command(async)]
fn get_day_view(state: State<'_, DbState>, date: String) -> Result<DayView, String> {
    perf::note_first_ipc("get_day_view");
    let (conn, lock_wait) = perf::timed_lock(&state)?;
    let t0 = std::time::Instant::now();
    let out = db::day_view(&conn, &date).map_err(|e| e.to_string());
    perf::record_day_view(&date, lock_wait, t0.elapsed().as_micros() as u64);
    out
}

#[tauri::command(async)]
fn list_projects(state: State<'_, DbState>) -> Result<Vec<Project>, String> {
    perf::note_first_ipc("list_projects");
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::list_projects(&conn).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn create_project(
    state: State<'_, DbState>,
    name: String,
    color: String,
) -> Result<Project, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::create_project(&conn, &name, &color).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn delete_project(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::delete_project(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn set_project_order(state: State<'_, DbState>, ids: Vec<i64>) -> Result<(), String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::set_project_order(&conn, &ids).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn add_assignment(
    state: State<'_, DbState>,
    date: String,
    app_key: String,
    title: String,
    project_id: i64,
) -> Result<(), String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::add_assignment(&conn, &date, &app_key, &title, project_id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn remove_assignment(
    state: State<'_, DbState>,
    date: String,
    app_key: String,
    title: String,
    project_id: i64,
) -> Result<(), String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::remove_assignment(&conn, &date, &app_key, &title, project_id).map_err(|e| e.to_string())
}

/// Remove an app (or one title of it) from a project across all days, writing
/// exceptions for whatever a rule or app-level assignment would still bill.
/// Returns the number of days that had to be excepted.
#[tauri::command(async)]
fn remove_from_project(
    state: State<'_, DbState>,
    project_id: i64,
    app_key: String,
    title: Option<String>,
) -> Result<usize, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::remove_from_project(&conn, project_id, &app_key, title.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn tracked_apps(state: State<'_, DbState>) -> Result<Vec<db::TrackedApp>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::tracked_apps(&conn).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn list_rules(state: State<'_, DbState>) -> Result<Vec<db::AssignmentRule>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::list_rules(&conn).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn create_rule(
    state: State<'_, DbState>,
    project_id: i64,
    app_key: String,
    app_name: Option<String>,
    effective_from: Option<String>,
) -> Result<db::AssignmentRule, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::create_rule(
        &conn,
        project_id,
        &app_key,
        app_name.as_deref(),
        effective_from.as_deref(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn delete_rule(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::delete_rule(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn remove_exception(
    state: State<'_, DbState>,
    date: String,
    app_key: String,
    title: String,
    project_id: i64,
) -> Result<(), String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::remove_exception(&conn, &date, &app_key, &title, project_id).map_err(|e| e.to_string())
}

/// Take a row out of a project for one day: delete any assignment that put it
/// there, then record an exception only if it would still be inherited from an
/// app-level assignment or a standing rule (ADR 0002).
#[tauri::command(async)]
fn exclude_for_day(
    state: State<'_, DbState>,
    date: String,
    app_key: String,
    title: String,
    project_id: i64,
) -> Result<(), String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::exclude_for_day(&conn, &date, &app_key, &title, project_id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn project_day_entries(
    state: State<'_, DbState>,
    project_id: i64,
    date: String,
) -> Result<Vec<db::ReceiptEntry>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::project_day_entries(&conn, project_id, &date).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn add_ignored_entry(
    state: State<'_, DbState>,
    app_key: String,
    app_name: Option<String>,
    title: String,
) -> Result<(), String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::add_ignored_entry(&conn, &app_key, app_name.as_deref(), &title).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn list_ignored_entries(state: State<'_, DbState>) -> Result<Vec<IgnoredEntry>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::list_ignored_entries(&conn).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn remove_ignored_entry(
    state: State<'_, DbState>,
    app_key: String,
    title: String,
) -> Result<(), String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::remove_ignored_entry(&conn, &app_key, &title).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn ignored_breakdown(state: State<'_, DbState>) -> Result<Vec<DayTotal>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::ignored_breakdown(&conn).map_err(|e| e.to_string())
}

/// Whether the app has macOS Accessibility permission (needed for window titles).
#[tauri::command(async)]
fn ax_status() -> bool {
    ax::is_trusted()
}

/// Prompt the user to grant Accessibility, returning the (likely still-false) status.
#[tauri::command(async)]
fn ax_request() -> bool {
    ax::prompt_trust();
    ax::is_trusted()
}

/// Open System Settings → Privacy & Security → Accessibility. Needed after an
/// update: the ad-hoc code signature changes, macOS keeps showing Echo as
/// enabled but the grant is stale, and the AX prompt won't re-fire — the user
/// has to toggle the entry off and on (or remove and re-add it).
#[tauri::command(async)]
fn ax_open_settings() -> Result<(), String> {
    tauri_plugin_opener::open_url(
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        None::<&str>,
    )
    .map_err(|e| e.to_string())
}

/// The running app version (CI-stamped `0.1.<run_number>` in releases, `0.1.0`
/// in dev). Shown in the Settings → About section.
#[tauri::command(async)]
fn app_version(app: tauri::AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Open an external URL in the system browser. Used by the About links
/// (Buy Me a Coffee, GitHub). Called Rust-side, so no webview capability change.
#[tauri::command(async)]
fn open_external(url: String) -> Result<(), String> {
    tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|e| e.to_string())
}

/// Total on-disk size of the SQLite database — main file + WAL + SHM — in bytes.
#[tauri::command(async)]
fn storage_size(app: tauri::AppHandle) -> Result<u64, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let mut total = 0u64;
    for suffix in ["", "-wal", "-shm"] {
        let path = dir.join(format!("{DB_FILENAME}{suffix}"));
        if let Ok(meta) = std::fs::metadata(path) {
            total += meta.len();
        }
    }
    Ok(total)
}

/// Clear all tracking data (segments + assignments + notes), keeping projects
/// and ignore rules.
///
/// Drops the open segment *before* the wipe rather than holding `current` across
/// it: `discard_current` empties the slot, so if a poller tick lands in the gap
/// it finds `None`, writes nothing (only a non-empty slot flushes on close), and
/// merely opens a fresh in-memory segment — which the following DELETE leaves
/// untouched. No stale row can survive, and the two locks are taken sequentially
/// (never nested), keeping lock order current → db.
#[tauri::command(async)]
fn clear_tracking_data(
    db: State<'_, DbState>,
    track: State<'_, Arc<TrackerState>>,
) -> Result<(), String> {
    track.discard_current();
    let conn = db.lock().map_err(|e| e.to_string())?;
    db::clear_tracking_data(&conn).map_err(|e| e.to_string())
}

/// Delete only untagged segments (those resolving to zero projects). Leaves the
/// open segment alone — it may become tagged later.
#[tauri::command(async)]
fn clear_untagged(db: State<'_, DbState>) -> Result<usize, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    db::clear_untagged(&conn).map_err(|e| e.to_string())
}

/// Wipe everything — all five tables. Drops the open segment first, as above.
#[tauri::command(async)]
fn reset_everything(
    db: State<'_, DbState>,
    track: State<'_, Arc<TrackerState>>,
) -> Result<(), String> {
    track.discard_current();
    let conn = db.lock().map_err(|e| e.to_string())?;
    db::reset_everything(&conn).map_err(|e| e.to_string())
}

/// The saved auto-delete-untagged config (enabled + day window).
#[tauri::command(async)]
fn get_autodelete_config(db: State<'_, DbState>) -> Result<db::AutodeleteConfig, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    db::get_autodelete_config(&conn).map_err(|e| e.to_string())
}

/// Persist the auto-delete config. The scheduler re-reads it each run, so a
/// change takes effect without a restart.
#[tauri::command(async)]
fn set_autodelete_config(db: State<'_, DbState>, enabled: bool, days: u32) -> Result<(), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    db::set_autodelete_config(&conn, enabled, days).map_err(|e| e.to_string())
}

/// Snapshot of the auto-update state machine; polled by the frontend banner.
#[tauri::command(async)]
fn update_status(state: State<'_, Arc<updater::UpdaterState>>) -> updater::UpdateStatus {
    updater::status(&state)
}

/// Download + install the pending update, then restart. On success the app
/// restarts before this resolves, so the frontend treats it as fire-and-forget.
#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    updater::install(app).await
}

#[tauri::command(async)]
fn project_breakdown(state: State<'_, DbState>, project_id: i64) -> Result<Vec<DayTotal>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::project_breakdown(&conn, project_id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn project_apps(state: State<'_, DbState>, project_id: i64) -> Result<Vec<ProjectApp>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::project_apps(&conn, project_id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn list_project_period_notes(
    state: State<'_, DbState>,
    project_id: i64,
) -> Result<Vec<ProjectPeriodNote>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::list_project_period_notes(&conn, project_id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn set_project_period_note(
    state: State<'_, DbState>,
    project_id: i64,
    granularity: String,
    period_key: String,
    note: String,
) -> Result<(), String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::set_project_period_note(&conn, project_id, &granularity, &period_key, &note)
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn app_icon_data_url(bundle_id: String) -> Result<Option<String>, String> {
    perf::note_first_ipc("app_icon_data_url");
    let t0 = std::time::Instant::now();
    let out = platform_app_icon_data_url(&bundle_id);
    perf::record_icon(
        t0.elapsed().as_micros() as u64,
        out.as_ref().map_or(0, String::len),
    );
    Ok(out)
}

/// Pixel size we ask AppKit for. The badge renders at 26 CSS px (`.app-badge`),
/// so 64 covers @2x displays with headroom while staying tiny to encode.
#[cfg(target_os = "macos")]
const ICON_PX: f64 = 64.0;

#[cfg(target_os = "macos")]
fn platform_app_icon_data_url(bundle_id: &str) -> Option<String> {
    use objc2::runtime::AnyObject;
    use objc2::AllocAnyThread;
    use objc2_app_kit::{
        NSBitmapImageFileType, NSBitmapImageRep, NSBitmapImageRepPropertyKey, NSWorkspace,
    };
    use objc2_foundation::{
        NSDataBase64EncodingOptions, NSDictionary, NSPoint, NSRect, NSSize, NSString,
    };

    let workspace = NSWorkspace::sharedWorkspace();
    let bundle = NSString::from_str(bundle_id);
    let app_url = workspace.URLForApplicationWithBundleIdentifier(&bundle)?;
    let path = app_url.path()?;
    let icon = workspace.iconForFile(&path);

    // `iconForFile` returns a *lazy* NSImage holding every representation in the
    // .icns — up to 1024². The obvious `TIFFRepresentation()` rasterises all of
    // them into one uncompressed bitmap (~350ms measured) and the PNG encode of
    // that monster costs another ~150ms; times ~50 apps on the main thread, that
    // was a multi-second freeze on cold start. Asking for a CGImage at the size
    // we actually draw makes AppKit hand back the small representation instead,
    // so both steps become trivial.
    icon.setSize(NSSize::new(ICON_PX, ICON_PX));
    let mut rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(ICON_PX, ICON_PX));
    let cg_image = unsafe { icon.CGImageForProposedRect_context_hints(&mut rect, None, None) }?;
    let bitmap = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &cg_image);

    let properties = NSDictionary::<NSBitmapImageRepPropertyKey, AnyObject>::new();
    let png = unsafe {
        bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)?
    };
    let encoded = png.base64EncodedStringWithOptions(NSDataBase64EncodingOptions(0));
    Some(format!("data:image/png;base64,{encoded}"))
}

#[cfg(not(target_os = "macos"))]
fn platform_app_icon_data_url(_bundle_id: &str) -> Option<String> {
    None
}

/// Auto-delete scheduler: purge untagged segments older than the configured
/// window once at startup (if enabled) and every 12h after. Config is re-read
/// each run, so toggling takes effect without a restart. Unlike the updater's
/// check, this runs in debug builds too so it can be exercised in `tauri dev`.
fn spawn_autodelete(app: tauri::AppHandle) {
    use std::time::Duration;
    const INTERVAL: Duration = Duration::from_secs(12 * 60 * 60);
    std::thread::spawn(move || loop {
        run_autodelete(&app);
        std::thread::sleep(INTERVAL);
    });
}

fn run_autodelete(app: &tauri::AppHandle) {
    perf::mark("autodelete: waiting for db lock");
    let db = app.state::<DbState>();
    let Ok(conn) = db.lock() else { return };
    let Ok(cfg) = db::get_autodelete_config(&conn) else {
        return;
    };
    if cfg.enabled {
        // Only touches segments older than the window; the open segment (start =
        // now) is never in range, so `current` needs no involvement.
        perf::time("autodelete: purge (holding db lock)", || {
            let _ = db::purge_untagged_older_than(&conn, cfg.days);
        });
    }
    perf::mark("autodelete: released db lock");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    perf::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .on_window_event(|window, event| {
            // Red close button = hide to the menu bar, keep tracking.
            // Main window only: the Accessory switch is process-global.
            if window.label() != "main" {
                return;
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                #[cfg(target_os = "macos")]
                let _ = window
                    .app_handle()
                    .set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
        })
        .setup(|app| {
            let dir = app.path().app_data_dir().expect("app data dir");
            let db_path = dir.join(DB_FILENAME);
            if let Some(parent) = db_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            let conn =
                perf::time("db open + migrate", || db::open(&db_path)).expect("open database");

            let shared: DbState = Arc::new(Mutex::new(conn));
            app.manage(shared.clone());

            let track = Arc::new(TrackerState::default());
            app.manage(track.clone());

            tray::init(app)?;

            poller::spawn(app.handle().clone(), shared, track);

            app.manage(Arc::new(updater::UpdaterState::default()));
            updater::spawn_periodic_check(app.handle().clone());

            spawn_autodelete(app.handle().clone());

            perf::mark("setup done — webview starts loading");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_day_view,
            list_projects,
            create_project,
            delete_project,
            set_project_order,
            add_assignment,
            remove_assignment,
            remove_from_project,
            tracked_apps,
            list_rules,
            create_rule,
            delete_rule,
            remove_exception,
            exclude_for_day,
            project_day_entries,
            add_ignored_entry,
            list_ignored_entries,
            remove_ignored_entry,
            ignored_breakdown,
            project_breakdown,
            project_apps,
            list_project_period_notes,
            set_project_period_note,
            app_icon_data_url,
            ax_status,
            ax_request,
            ax_open_settings,
            app_version,
            open_external,
            storage_size,
            clear_tracking_data,
            clear_untagged,
            reset_everything,
            get_autodelete_config,
            set_autodelete_config,
            update_status,
            install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Single flush point for every exit path (tray Quit, Cmd-Q, …):
            // the in-progress segment must never be lost.
            if let tauri::RunEvent::Exit = event {
                let track = app_handle.state::<Arc<TrackerState>>();
                let db = app_handle.state::<DbState>();
                track.close_current(&db, chrono::Utc::now().timestamp());
            }
        });
}
