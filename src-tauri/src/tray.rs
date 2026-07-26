// System tray with a rich menu.
//
// Reference VoiceInk : MenuBarView.swift. We keep the useful subset for a
// Windows tray UX :
//  - Open Parla       : show main window
//  - Toggle recording : same as the global hotkey action
//  - Copy last text   : copy the latest enhanced (or raw) transcription
//  - Divider
//  - Settings         : open main window on Settings tab
//  - Check for update : trigger the updater
//  - Divider
//  - Quit Parla
//
// Left-click on the tray icon shows the main window (standard Windows UX).

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, Runtime,
};
use tracing::warn;

use crate::db::{transcription as history_repo, Database};

pub fn setup<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Parla", true, None::<&str>)?;
    let toggle_record =
        MenuItem::with_id(app, "toggle_record", "Toggle recording", true, None::<&str>)?;
    let copy_last = MenuItem::with_id(
        app,
        "copy_last",
        "Copy last transcription",
        true,
        None::<&str>,
    )?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let check_update =
        MenuItem::with_id(app, "check_update", "Check for updates", true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Parla", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &open,
            &toggle_record,
            &copy_last,
            &sep1,
            &settings,
            &check_update,
            &sep2,
            &quit,
        ],
    )?;

    TrayIconBuilder::with_id("main")
        .tooltip("Parla")
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "toggle_record" => {
                // The frontend listens to "tray:toggle-record" and dispatches
                // the same action as the global hotkey (start or stop
                // depending on current state).
                let _ = app.emit("tray:toggle-record", ());
            }
            "copy_last" => {
                if let Err(e) = copy_last_transcription(app) {
                    warn!(error = %e, "copy last transcription failed");
                    let _ = app.emit("tray:notice", format!("Copy failed: {e}"));
                } else {
                    let _ = app.emit("tray:notice", "Copied last transcription");
                }
            }
            "settings" => {
                show_main_window(app);
                let _ = app.emit("tray:navigate", "settings");
            }
            "check_update" => {
                show_main_window(app);
                let _ = app.emit("tray:check-update", ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

fn copy_last_transcription<R: Runtime>(app: &AppHandle<R>) -> anyhow::Result<()> {
    let db = app
        .try_state::<Database>()
        .ok_or_else(|| anyhow::anyhow!("database not initialized"))?;
    let last = {
        let conn = db.0.lock();
        history_repo::list_page(&conn, 1, None, None)?
    };
    let record = last
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("history is empty"))?;
    let text = record
        .enhanced_text
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(record.text);
    if text.trim().is_empty() {
        anyhow::bail!("last transcription is empty");
    }
    // Use arboard through the paste module's clipboard helper. We avoid
    // importing arboard directly here - the paste module owns the global
    // clipboard lock.
    crate::paste::copy_to_clipboard(&text)?;
    Ok(())
}
