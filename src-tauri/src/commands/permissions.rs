// Commandes Tauri pour la page Permissions.
//
// Reference VoiceInk Views/PermissionsView.swift : liste de PermissionCards
// avec status dot + bouton action. VoiceInk verifie TCC macOS (micro,
// accessibility, screen recording). Sur Windows les permissions sont :
// - Microphone : peut etre bloque Settings > Privacy > Microphone.
// - Accessibility : pas de TCC, SendInput marche toujours.
// - Screen Recording : pas de TCC, Windows.Media.Ocr fonctionne
//   si un pack de langue OCR est installe.
// - Auto-start : tauri-plugin-autostart.

use serde::Serialize;
use serde_json::{json, Value as JsonValue};
use tauri::{command, AppHandle};

#[derive(Debug, Serialize)]
pub struct PermissionStatus {
    pub microphone: PermissionState,
    pub ocr: PermissionState,
    pub autostart: PermissionState,
    pub hotkey: PermissionState,
}

/// Status d'une permission. Le backend ne retourne JAMAIS de string visible
/// par l'utilisateur : il fournit une cle i18n (`label_key`) plus eventuellement
/// des arguments (`label_args` pour interpoler `{count}`, etc.) et le frontend
/// resoud via `react-i18next` selon la langue active.
#[derive(Debug, Serialize)]
pub struct PermissionState {
    pub ok: bool,
    pub label_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_args: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint_key: Option<String>,
    /// Diagnostic technique brut (message d'erreur natif Windows, etc.).
    /// Affiche seulement en complement du hint traduit. Reste en anglais
    /// comme tous les messages d'erreur Win32, donc non traduit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
}

#[command]
pub fn check_permissions(app: AppHandle) -> PermissionStatus {
    // Microphone : on tente de lister les devices audio. Liste vide =
    // micro bloque Privacy ou driver absent.
    let devs = crate::audio::list_input_devices();
    let microphone = if !devs.is_empty() {
        PermissionState {
            ok: true,
            label_key: "permissions.audio.detected".into(),
            label_args: Some(json!({ "count": devs.len() })),
            hint_key: None,
            diagnostic: None,
        }
    } else {
        PermissionState {
            ok: false,
            label_key: "permissions.audio.none".into(),
            label_args: None,
            hint_key: Some("permissions.audio.hint".into()),
            diagnostic: None,
        }
    };

    // OCR : tente de creer un OcrEngine. None si aucun pack langue installe.
    let ocr = match windows::Media::Ocr::OcrEngine::TryCreateFromUserProfileLanguages() {
        Ok(_) => PermissionState {
            ok: true,
            label_key: "permissions.ocr.available".into(),
            label_args: None,
            hint_key: None,
            diagnostic: None,
        },
        Err(e) => PermissionState {
            ok: false,
            label_key: "permissions.ocr.unavailable".into(),
            label_args: None,
            hint_key: Some("permissions.ocr.hint".into()),
            diagnostic: Some(e.to_string()),
        },
    };

    // Autostart : tauri-plugin-autostart expose is_enabled via invoke.
    let autostart = match autostart_enabled(&app) {
        Ok(true) => PermissionState {
            ok: true,
            label_key: "permissions.autostart.enabled".into(),
            label_args: None,
            hint_key: None,
            diagnostic: None,
        },
        Ok(false) => PermissionState {
            ok: false,
            label_key: "permissions.autostart.disabled".into(),
            label_args: None,
            hint_key: Some("permissions.autostart.disabledHint".into()),
            diagnostic: None,
        },
        Err(e) => PermissionState {
            ok: false,
            label_key: "permissions.autostart.unknown".into(),
            label_args: None,
            hint_key: None,
            diagnostic: Some(e),
        },
    };

    // Hotkey : sur Windows WH_KEYBOARD_LL ne necessite pas de permission
    // speciale. On indique toujours OK.
    let hotkey = PermissionState {
        ok: true,
        label_key: "permissions.hotkey.active".into(),
        label_args: None,
        hint_key: Some("permissions.hotkey.hint".into()),
        diagnostic: None,
    };

    PermissionStatus {
        microphone,
        ocr,
        autostart,
        hotkey,
    }
}

fn autostart_enabled(app: &AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let mgr = app.autolaunch();
    mgr.is_enabled().map_err(|e| e.to_string())
}

#[command]
pub fn set_autostart_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let mgr = app.autolaunch();
    if enabled {
        mgr.enable().map_err(|e| e.to_string())
    } else {
        mgr.disable().map_err(|e| e.to_string())
    }
}

#[command]
pub fn open_privacy_microphone(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url("ms-settings:privacy-microphone", None::<&str>)
        .map_err(|e| e.to_string())
}

#[command]
pub fn open_language_settings(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url("ms-settings:regionlanguage", None::<&str>)
        .map_err(|e| e.to_string())
}

#[command]
pub fn get_recorder_style(app: AppHandle) -> String {
    crate::mini_recorder::get_style(&app).as_str().to_string()
}

const STORE_FILE: &str = "parla.settings.json";
const KEY_ONBOARDING: &str = "onboarding_completed";

#[command]
pub fn get_onboarding_completed(app: AppHandle) -> bool {
    use tauri_plugin_store::StoreExt;
    app.store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(KEY_ONBOARDING).and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

#[command]
pub fn set_onboarding_completed(app: AppHandle, completed: bool) -> Result<(), String> {
    use tauri_plugin_store::StoreExt;
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(KEY_ONBOARDING, serde_json::Value::Bool(completed));
    store.save().map_err(|e| e.to_string())
}

#[command]
pub fn set_recorder_style(app: AppHandle, style: String) -> Result<(), String> {
    let s = crate::mini_recorder::RecorderStyle::parse(&style);
    crate::mini_recorder::set_style(&app, s).map_err(|e| e.to_string())?;
    // Re-positionne la fenetre si ouverte.
    if let Some(win) = tauri::Manager::get_webview_window(&app, crate::mini_recorder::LABEL) {
        crate::mini_recorder::ensure_open(&app);
        let _ = win.show();
    }
    Ok(())
}
