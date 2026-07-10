//! Status-bar (tray) icon: menu, handlers, and live "Today" readout.
//!
//! Menu layout (spec §1):
//!   Today: 4h 12m          (disabled)
//!   ─────────────────────
//!   Pause time tracking    (toggles to "Resume time tracking")
//!   Open Echo
//!   ─────────────────────
//!   Quit Echo
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
    update_item: MenuItem<tauri::Wry>,
    /// Last rendered "Today: Xh Ym" string, to skip no-op updates.
    last_today: Mutex<String>,
}

pub fn init(app: &tauri::App) -> tauri::Result<()> {
    let today_item = MenuItem::with_id(app, "today", "Today: 0m", false, None::<&str>)?;
    let pause_item = MenuItem::with_id(app, "pause", "Pause time tracking", true, None::<&str>)?;
    let open_item = MenuItem::with_id(app, "open", "Open Echo", true, None::<&str>)?;
    let update_item = MenuItem::with_id(app, "update", "Check for Updates…", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit Echo", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &today_item,
            &sep1,
            &pause_item,
            &open_item,
            &update_item,
            &sep2,
            &quit_item,
        ],
    )?;

    let tray_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/trayTemplate.png"))
        .unwrap_or_else(|_| app.default_window_icon().expect("default icon").clone());

    let tray = TrayIconBuilder::new()
        .icon(tray_icon)
        .tooltip("Echo - tracking")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu_event)
        .build(app)?;

    app.manage(TrayHandles {
        tray,
        today_item,
        pause_item,
        update_item,
        last_today: Mutex::new(String::new()),
    });
    Ok(())
}

fn on_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        "pause" => toggle_pause(app),
        "open" => open_window(app),
        "update" => crate::updater::on_tray_clicked(app),
        "quit" => app.exit(0), // flush happens in RunEvent::Exit (lib.rs)
        _ => {}
    }
}

fn toggle_pause(app: &AppHandle) {
    let track = app.state::<Arc<TrackerState>>();
    let handles = app.state::<TrayHandles>();
    let now_paused = !track.paused.load(Ordering::SeqCst);
    // Order matters: set `paused` first, then close. The poller re-checks
    // `paused` inside the `current` lock, so nothing can reopen after this.
    track.paused.store(now_paused, Ordering::SeqCst);

    if now_paused {
        // Close the in-progress segment right now so paused time is unbilled.
        let db = app.state::<DbState>();
        track.close_current(&db, chrono::Utc::now().timestamp());
        let _ = handles.pause_item.set_text("Resume time tracking");
        let _ = handles.tray.set_tooltip(Some("Echo - paused"));
    } else {
        let _ = handles.pause_item.set_text("Pause time tracking");
        let _ = handles.tray.set_tooltip(Some("Echo - tracking"));
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
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Called from the updater as its state changes. Renames the update menu item
/// ("Check for Updates…" / "Update Echo to X…" / "Downloading update…").
pub fn set_update_text(app: &AppHandle, text: &str, enabled: bool) {
    let Some(handles) = app.try_state::<TrayHandles>() else {
        return;
    };
    let _ = handles.update_item.set_text(text);
    let _ = handles.update_item.set_enabled(enabled);
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
            let _ = handles.tray.set_tooltip(Some(format!("Echo - {today}")));
        }
    }
}
