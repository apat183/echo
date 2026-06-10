mod ax;
mod db;
mod poller;
mod tracker;
mod tray;

use db::{DayTotal, DayView, Project, ProjectApp, DbState};
use std::sync::{Arc, Mutex};
use tracker::TrackerState;
use tauri::{Manager, State};

#[tauri::command]
fn get_day_view(state: State<'_, DbState>, date: String) -> Result<DayView, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::day_view(&conn, &date).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_projects(state: State<'_, DbState>) -> Result<Vec<Project>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::list_projects(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_project(
    state: State<'_, DbState>,
    name: String,
    color: String,
) -> Result<Project, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::create_project(&conn, &name, &color).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_project(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::delete_project(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
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

#[tauri::command]
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

/// Whether the app has macOS Accessibility permission (needed for window titles).
#[tauri::command]
fn ax_status() -> bool {
    ax::is_trusted()
}

/// Prompt the user to grant Accessibility, returning the (likely still-false) status.
#[tauri::command]
fn ax_request() -> bool {
    ax::prompt_trust();
    ax::is_trusted()
}

#[tauri::command]
fn project_breakdown(state: State<'_, DbState>, project_id: i64) -> Result<Vec<DayTotal>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::project_breakdown(&conn, project_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn project_apps(state: State<'_, DbState>, project_id: i64) -> Result<Vec<ProjectApp>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::project_apps(&conn, project_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_note(
    state: State<'_, DbState>,
    project_id: i64,
    app_key: String,
    title: String,
    note: String,
) -> Result<(), String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    db::set_note(&conn, project_id, &app_key, &title, &note).map_err(|e| e.to_string())
}

#[tauri::command]
fn app_icon_data_url(bundle_id: String) -> Result<Option<String>, String> {
    Ok(platform_app_icon_data_url(&bundle_id))
}

#[cfg(target_os = "macos")]
fn platform_app_icon_data_url(bundle_id: &str) -> Option<String> {
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSBitmapImageRepPropertyKey, NSWorkspace};
    use objc2_foundation::{NSDataBase64EncodingOptions, NSDictionary, NSString};

    let workspace = NSWorkspace::sharedWorkspace();
    let bundle = NSString::from_str(bundle_id);
    let app_url = workspace.URLForApplicationWithBundleIdentifier(&bundle)?;
    let path = app_url.path()?;
    let icon = workspace.iconForFile(&path);
    let tiff = icon.TIFFRepresentation()?;
    let bitmap = NSBitmapImageRep::imageRepWithData(&tiff)?;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .on_window_event(|window, event| {
            // Red close button = hide to the menu bar, keep tracking (spec §2).
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
            std::fs::create_dir_all(&dir).ok();
            let db_path = dir.join("echo.sqlite3");
            let conn = db::open(&db_path).expect("open database");

            let shared: DbState = Arc::new(Mutex::new(conn));
            app.manage(shared.clone());

            let track = Arc::new(TrackerState::default());
            app.manage(track.clone());

            tray::init(app)?;

            poller::spawn(app.handle().clone(), shared, track);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_day_view,
            list_projects,
            create_project,
            delete_project,
            add_assignment,
            remove_assignment,
            project_breakdown,
            project_apps,
            set_note,
            app_icon_data_url,
            ax_status,
            ax_request,
        ])
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
}
