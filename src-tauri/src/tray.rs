// System tray with the VoiceInk 2.x menu bar layout.
//
// Reference VoiceInk : App/MenuBar/MenuBarView.swift (completedOnboardingMenu)
//  - Toggle Recorder
//  - Mode: <active>  ▸ enabled modes (✓ on the effective one), Manage Modes,
//                       Manage Models
//  - Audio Input     ▸ input devices (✓ on the selected one)
//  - Retry Last Transcription
//  - Copy Last Transcription        Shift+Cmd+C
//  - History                        Shift+Cmd+H
//  - Hide / Show Dock Icon          (macOS only, no Windows equivalent)
//  - Launch at Login (check item)
//  - Settings                       Cmd+,
//  - Check for Updates
//  - Quit VoiceInk
// Before the onboarding is completed the menu only offers "Complete
// Onboarding" and "Quit" (MenuBarView.onboardingMenu).
//
// Windows adaptations :
//  - "Open Parla" stays first : a left click on the tray icon also opens
//    the main window (standard Windows UX).
//  - Shortcut labels are displayed in the native accelerator column (text
//    after a tab character, which Windows menus right-align). They are the
//    user's configured global shortcuts (hotkeys::keyboard_hook), the menu
//    itself never registers accelerators : VoiceInk's Shift+Cmd+C is also a
//    menu-only accelerator, the global bindings live in ShortcutMonitor.
//  - The menu is rebuilt (`refresh`) whenever its inputs change : shortcut
//    config, Power Mode profiles / active session, selected microphone,
//    UI language, onboarding state, autostart. A SwiftUI menu is reactive,
//    a Win32 menu is not.
//  - Labels are localized from the UI language mirrored in the settings
//    store by the frontend (`set_ui_language`).

use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, Wry,
};
use tracing::{info, warn};

use crate::audio;
use crate::commands::{hotkey as hotkey_cfg, permissions, settings};
use crate::history::last_transcription;
use crate::hotkeys::keyboard_hook::{trigger_label, HotkeyTrigger};
use crate::power_mode;
use crate::transcription::engine;

const TRAY_ID: &str = "main";

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    let icon = app
        .default_window_icon()
        .cloned()
        .expect("default window icon is bundled");

    TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Parla")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| handle_menu_event(app, event.id.as_ref()))
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

/// Rebuilds the tray menu from the current state. Safe to call from any
/// thread : Tauri marshals menu creation and `set_menu` to the main thread.
/// No-op before `setup` (tray not created yet).
pub fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    match build_menu(app) {
        Ok(menu) => {
            if let Err(e) = tray.set_menu(Some(menu)) {
                warn!(error = %e, "tray: set_menu failed");
            }
        }
        Err(e) => warn!(error = %e, "tray: build_menu failed"),
    }
}

/// Appends the configured shortcut label in the accelerator column.
fn with_accel(text: String, trigger: HotkeyTrigger) -> String {
    match trigger_label(trigger) {
        Some(accel) => format!("{text}\t{accel}"),
        None => text,
    }
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let lang = settings::ui_language(app);
    let t = |key: &str| tr(&lang, key);

    if !permissions::get_onboarding_completed(app.clone()) {
        let complete = MenuItem::with_id(app, "open", t("completeOnboarding"), true, None::<&str>)?;
        let sep = PredefinedMenuItem::separator(app)?;
        let quit = MenuItem::with_id(app, "quit", t("quit"), true, None::<&str>)?;
        return Menu::with_items(app, &[&complete, &sep, &quit]);
    }

    let hk = hotkey_cfg::load(app);

    let open = MenuItem::with_id(app, "open", t("open"), true, None::<&str>)?;
    let toggle = MenuItem::with_id(
        app,
        "toggle_record",
        with_accel(t("toggleRecorder"), hk.primary.trigger),
        true,
        None::<&str>,
    )?;

    // -- Power Mode submenu (VoiceInk "Mode: <name>") -----------------------
    let configs = power_mode::session::enabled_configs(app);
    let effective = power_mode::session::effective_config_id(app);
    let active_name = configs
        .iter()
        .find(|c| Some(&c.id) == effective.as_ref())
        .map(|c| c.name.clone())
        .unwrap_or_else(|| t("none"));
    let mode_menu = Submenu::with_id(
        app,
        "mode_menu",
        format!("{}: {}", t("mode"), active_name),
        true,
    )?;
    if configs.is_empty() {
        mode_menu.append(&MenuItem::with_id(
            app,
            "mode_none",
            t("noModes"),
            false,
            None::<&str>,
        )?)?;
    }
    for c in &configs {
        let is_active = Some(&c.id) == effective.as_ref();
        let text = if is_active {
            format!("{} {}  \u{2713}", c.emoji, c.name)
        } else {
            format!("{} {}", c.emoji, c.name)
        };
        mode_menu.append(&MenuItem::with_id(
            app,
            format!("mode:{}", c.id),
            text,
            true,
            None::<&str>,
        )?)?;
    }
    mode_menu.append(&PredefinedMenuItem::separator(app)?)?;
    mode_menu.append(&MenuItem::with_id(
        app,
        "manage_modes",
        t("manageModes"),
        true,
        None::<&str>,
    )?)?;
    mode_menu.append(&MenuItem::with_id(
        app,
        "manage_models",
        t("manageModels"),
        true,
        None::<&str>,
    )?)?;

    // -- Audio input submenu (VoiceInk "Audio Input") -----------------------
    let selected_device = audio::device::selected_input_device(app);
    let audio_menu = Submenu::with_id(app, "audio_menu", t("audioInput"), true)?;
    audio_menu.append(&CheckMenuItem::with_id(
        app,
        "device:",
        t("systemDefault"),
        true,
        selected_device.is_none(),
        None::<&str>,
    )?)?;
    for d in audio::list_input_devices() {
        let checked = selected_device.as_deref() == Some(d.name.as_str());
        audio_menu.append(&CheckMenuItem::with_id(
            app,
            format!("device:{}", d.name),
            &d.name,
            true,
            checked,
            None::<&str>,
        )?)?;
    }

    // -- Last transcription actions ------------------------------------------
    let retry = MenuItem::with_id(
        app,
        "retry_last",
        with_accel(t("retryLast"), hk.actions.retry_last_transcription),
        true,
        None::<&str>,
    )?;
    let copy_last = MenuItem::with_id(
        app,
        "copy_last",
        with_accel(t("copyLast"), hk.actions.copy_last_transcription),
        true,
        None::<&str>,
    )?;
    let history = MenuItem::with_id(
        app,
        "history",
        with_accel(t("history"), hk.actions.open_history),
        true,
        None::<&str>,
    )?;
    let launch_at_login = CheckMenuItem::with_id(
        app,
        "launch_at_login",
        t("launchAtLogin"),
        true,
        autostart_enabled(app),
        None::<&str>,
    )?;

    let settings_item = MenuItem::with_id(app, "settings", t("settings"), true, None::<&str>)?;
    let check_update = MenuItem::with_id(
        app,
        "check_update",
        t("checkUpdates"),
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", t("quit"), true, None::<&str>)?;

    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;

    Menu::with_items(
        app,
        &[
            &open,
            &toggle,
            &sep1,
            &mode_menu,
            &audio_menu,
            &sep2,
            &retry,
            &copy_last,
            &history,
            &launch_at_login,
            &sep3,
            &settings_item,
            &check_update,
            &quit,
        ],
    )
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        "open" => show_main_window(app),
        "toggle_record" => engine::toggle_from_ui(app),
        "manage_modes" => navigate(app, "powermode"),
        "manage_models" => navigate(app, "models"),
        "retry_last" => report(app, "retry", last_transcription::retry_last(app)),
        "copy_last" => report(app, "copy", last_transcription::copy_last(app)),
        "history" => last_transcription::open_history(app),
        "launch_at_login" => toggle_autostart(app),
        "settings" => navigate(app, "settings"),
        "check_update" => {
            show_main_window(app);
            let _ = app.emit("tray:check-update", ());
        }
        "quit" => app.exit(0),
        other => {
            if let Some(mode_id) = other.strip_prefix("mode:") {
                select_mode(app, mode_id);
            } else if let Some(device) = other.strip_prefix("device:") {
                select_device(app, device);
            }
        }
    }
}

fn report(app: &AppHandle, what: &str, result: anyhow::Result<()>) {
    if let Err(e) = result {
        warn!(action = what, error = %e, "tray action failed");
        let _ = app.emit("tray:notice", format!("{what} failed: {e}"));
    }
}

fn navigate(app: &AppHandle, panel: &str) {
    crate::mini_recorder::show_main_window(app.clone(), Some(panel.to_string()));
}

fn select_mode(app: &AppHandle, mode_id: &str) {
    if let Some(session) = power_mode::session::select_by_id(app, mode_id) {
        let _ = app.emit("power_mode:active", &session);
    }
    refresh(app);
}

fn select_device(app: &AppHandle, device: &str) {
    let name = if device.is_empty() {
        None
    } else {
        Some(device.to_string())
    };
    if let Err(e) = audio::device::set_selected_input_device(app, name) {
        warn!(error = %e, "tray: set_selected_input_device failed");
    }
    refresh(app);
}

fn autostart_enabled(app: &AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

fn toggle_autostart(app: &AppHandle) {
    use tauri_plugin_autostart::ManagerExt;
    let mgr = app.autolaunch();
    let enabled = mgr.is_enabled().unwrap_or(false);
    let result = if enabled { mgr.disable() } else { mgr.enable() };
    match result {
        Ok(()) => info!(enabled = !enabled, "tray: launch at login toggled"),
        Err(e) => warn!(error = %e, "tray: launch at login toggle failed"),
    }
    let _ = app.emit("settings:autostart-changed", !enabled);
    refresh(app);
}

fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

/// Native menu labels. Mirrors the `tray.*` keys of the frontend locales
/// (en / fr / es) ; the native menu cannot use react-i18next.
fn tr(lang: &str, key: &str) -> String {
    let col = match lang {
        "fr" => 1,
        "es" => 2,
        _ => 0,
    };
    let row: [&str; 3] = match key {
        "open" => ["Open Parla", "Ouvrir Parla", "Abrir Parla"],
        "completeOnboarding" => [
            "Complete onboarding",
            "Terminer la configuration",
            "Completar la configuraci\u{f3}n",
        ],
        "toggleRecorder" => [
            "Toggle recording",
            "D\u{e9}marrer / arr\u{ea}ter l'enregistrement",
            "Iniciar / detener la grabaci\u{f3}n",
        ],
        "mode" => ["Power Mode", "Power Mode", "Power Mode"],
        "none" => ["None", "Aucun", "Ninguno"],
        "noModes" => [
            "No profiles available",
            "Aucun profil disponible",
            "Sin perfiles disponibles",
        ],
        "manageModes" => [
            "Manage Power Mode",
            "G\u{e9}rer Power Mode",
            "Gestionar Power Mode",
        ],
        "manageModels" => [
            "Manage AI models",
            "G\u{e9}rer les mod\u{e8}les IA",
            "Gestionar modelos de IA",
        ],
        "audioInput" => ["Audio input", "Entr\u{e9}e audio", "Entrada de audio"],
        "systemDefault" => [
            "System default",
            "D\u{e9}faut syst\u{e8}me",
            "Predeterminado del sistema",
        ],
        "retryLast" => [
            "Retry last transcription",
            "R\u{e9}essayer la derni\u{e8}re transcription",
            "Reintentar la \u{fa}ltima transcripci\u{f3}n",
        ],
        "copyLast" => [
            "Copy last transcription",
            "Copier la derni\u{e8}re transcription",
            "Copiar la \u{fa}ltima transcripci\u{f3}n",
        ],
        "history" => ["History", "Historique", "Historial"],
        "launchAtLogin" => ["Launch at login", "Lancer au d\u{e9}marrage", "Iniciar al arrancar"],
        "settings" => ["Settings", "Param\u{e8}tres", "Ajustes"],
        "checkUpdates" => [
            "Check for updates",
            "V\u{e9}rifier les mises \u{e0} jour",
            "Buscar actualizaciones",
        ],
        "quit" => ["Quit Parla", "Quitter Parla", "Salir de Parla"],
        _ => [key, key, key],
    };
    row[col].to_string()
}
