// Commandes Tauri pour les reglages persistes (modele selectionne, langue, etc.).
//
// Reference VoiceInk : stockage UserDefaults dans VoiceInk. Ici on utilise
// tauri-plugin-store qui ecrit dans un JSON sous AppConfig.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tauri_plugin_store::StoreExt;

use crate::services::proxy;
use crate::text_processing::filler_words;
use crate::transcription::pipeline;

const STORE_FILE: &str = "parla.settings.json";
const PROXY_SETTINGS_KEY: &str = "proxy_settings";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxySettings {
    pub enabled: bool,
    pub url: Option<String>,
    #[serde(default)]
    pub no_proxy_entries: Vec<String>,
}
impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            url: None,
            no_proxy_entries: Vec::new(),
        }
    }
}

#[tauri::command]
pub fn get_proxy_settings(app: AppHandle) -> ProxySettings {
    app.store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(PROXY_SETTINGS_KEY))
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}
#[tauri::command]
pub fn set_proxy_settings(app: AppHandle, settings: ProxySettings) -> Result<(), String> {
    let mut settings = settings;
    let embedded_credentials = settings
        .url
        .as_deref()
        .filter(|u| !u.trim().is_empty())
        .map(proxy::sanitize_proxy_url)
        .transpose()?;
    if let Some((url, credentials)) = embedded_credentials {
        settings.url = Some(url);
        if let Some((username, password)) = credentials {
            let identity = proxy::proxy_identity(settings.url.as_deref().unwrap())
                .map_err(|e| e.to_string())?;
            proxy::set_credentials_for(&username, &password, &identity)
                .map_err(|e| e.to_string())?;
        }
    }
    proxy::validate_settings(
        settings.enabled,
        settings.url.as_deref(),
        &settings.no_proxy_entries,
    )?;
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let previous = store.get(PROXY_SETTINGS_KEY);
    store.set(
        PROXY_SETTINGS_KEY,
        serde_json::to_value(settings).map_err(|e| e.to_string())?,
    );
    if let Err(error) = store.save() {
        return Err(error.to_string());
    }
    if let Err(error) = proxy::configure(&app) {
        match previous {
            Some(value) => {
                store.set(PROXY_SETTINGS_KEY, value);
            }
            None => {
                store.delete(PROXY_SETTINGS_KEY);
            }
        }
        let _ = store.save();
        let _ = proxy::configure(&app);
        return Err(error.to_string());
    }
    Ok(())
}
#[tauri::command]
pub fn set_proxy_credentials(
    app: AppHandle,
    username: String,
    password: String,
) -> Result<(), String> {
    let settings = get_proxy_settings(app.clone());
    let url = settings
        .url
        .as_deref()
        .ok_or("proxy URL required for credentials")?;
    let identity = proxy::proxy_identity(url).map_err(|e| e.to_string())?;
    proxy::set_credentials_for(&username, &password, &identity).map_err(|e| e.to_string())?;
    proxy::configure(&app).map_err(|e| e.to_string())
}
#[tauri::command]
pub fn delete_proxy_credentials(app: AppHandle) -> Result<(), String> {
    proxy::delete_credentials().map_err(|e| e.to_string())?;
    proxy::configure(&app).map_err(|e| e.to_string())
}
#[tauri::command]
pub fn has_proxy_credentials() -> bool {
    proxy::has_credentials()
}

/// Emet un event Tauri "source:changed" avec la source actuelle.
/// Appele chaque fois que la selection de source (kind / modele) change,
/// que ce soit via une commande UI ou via PowerMode (apply / restore).
/// Permet au frontend (ModelsPage, etc.) de rafraichir sans polling.
pub fn emit_source_changed(app: &AppHandle) {
    let source = get_transcription_source(app.clone());
    if let Err(e) = app.emit("source:changed", &source) {
        tracing::warn!(error = %e, "emit source:changed a echoue");
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TextProcessingSettings {
    pub text_formatting_enabled: bool,
    pub remove_filler_words: bool,
    pub filler_words: Vec<String>,
    pub append_trailing_space: bool,
    pub restore_clipboard_after_paste: bool,
}

#[tauri::command]
pub fn set_selected_whisper_model(app: AppHandle, id: Option<String>) -> Result<(), String> {
    pipeline::set_selected_model(&app, id.as_deref()).map_err(|e| e.to_string())?;
    emit_source_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn get_selected_whisper_model(app: AppHandle) -> Option<String> {
    pipeline::get_selected_model(&app)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TranscriptionSource {
    /// "local" | "cloud" | "parakeet"
    pub kind: String,
    /// Si kind=local : id du modele Whisper (ggml-base, imported:foo...)
    pub whisper_model_id: Option<String>,
    /// Si kind=cloud : provider id (groq, deepgram, ...)
    pub cloud_provider: Option<String>,
    /// Si kind=cloud : model id chez ce provider (whisper-large-v3-turbo, nova-3, ...)
    pub cloud_model: Option<String>,
    /// Si kind=parakeet : id de variante (parakeet-tdt-0.6b-v2, ...).
    pub parakeet_model_id: Option<String>,
}

impl Default for TranscriptionSource {
    fn default() -> Self {
        Self {
            kind: "local".into(),
            whisper_model_id: None,
            cloud_provider: None,
            cloud_model: None,
            parakeet_model_id: None,
        }
    }
}

#[tauri::command]
pub fn get_transcription_source(app: AppHandle) -> TranscriptionSource {
    let Some(store) = app.store(STORE_FILE).ok() else {
        return TranscriptionSource::default();
    };
    TranscriptionSource {
        kind: store
            .get("transcription_source_kind")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "local".into()),
        whisper_model_id: store
            .get("selected_whisper_model")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        cloud_provider: store
            .get("selected_cloud_provider")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        cloud_model: store
            .get("selected_cloud_model")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        parakeet_model_id: store
            .get("selected_parakeet_model")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
    }
}

/// Lightweight source kind switch. Does NOT touch the per-kind selection
/// fields (whisper_model_id, cloud_provider, cloud_model, parakeet_model_id)
/// so the user can flip between tabs without losing their current selection
/// in each tab.
#[tauri::command]
pub fn set_transcription_kind(app: AppHandle, kind: String) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set("transcription_source_kind", serde_json::Value::String(kind));
    store.save().map_err(|e| e.to_string())?;
    emit_source_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn set_transcription_source(app: AppHandle, source: TranscriptionSource) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(
        "transcription_source_kind",
        serde_json::Value::String(source.kind),
    );
    match source.cloud_provider {
        Some(v) => store.set("selected_cloud_provider", serde_json::Value::String(v)),
        None => {
            store.delete("selected_cloud_provider");
        }
    }
    match source.cloud_model {
        Some(v) => store.set("selected_cloud_model", serde_json::Value::String(v)),
        None => {
            store.delete("selected_cloud_model");
        }
    }
    match source.parakeet_model_id {
        Some(v) => store.set("selected_parakeet_model", serde_json::Value::String(v)),
        None => {
            store.delete("selected_parakeet_model");
        }
    }
    store.save().map_err(|e| e.to_string())?;
    emit_source_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn get_text_processing_settings(app: AppHandle) -> TextProcessingSettings {
    TextProcessingSettings {
        text_formatting_enabled: pipeline::get_text_formatting_enabled(&app),
        remove_filler_words: filler_words::is_enabled(&app),
        filler_words: filler_words::current_list(&app),
        append_trailing_space: pipeline::get_append_trailing_space(&app),
        restore_clipboard_after_paste: pipeline::get_restore_clipboard(&app),
    }
}

#[tauri::command]
pub fn set_text_formatting_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set("text_formatting_enabled", serde_json::Value::Bool(enabled));
    store.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_remove_filler_words(app: AppHandle, enabled: bool) -> Result<(), String> {
    filler_words::set_enabled(&app, enabled)
}

#[tauri::command]
pub fn set_filler_words(app: AppHandle, words: Vec<String>) -> Result<(), String> {
    filler_words::set_list(&app, words)
}

#[tauri::command]
pub fn set_append_trailing_space(app: AppHandle, enabled: bool) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set("append_trailing_space", serde_json::Value::Bool(enabled));
    store.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_restore_clipboard_after_paste(app: AppHandle, enabled: bool) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(
        "restore_clipboard_after_paste",
        serde_json::Value::Bool(enabled),
    );
    store.save().map_err(|e| e.to_string())
}

const KEY_CLOSE_TO_TRAY: &str = "close_to_tray";

/// Close-to-tray behavior for the main window. Default: true (hide instead
/// of quit when the X button is clicked). The tray provides "Quit Parla"
/// as the explicit exit path.
pub fn close_to_tray_enabled(app: &AppHandle) -> bool {
    app.store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(KEY_CLOSE_TO_TRAY).and_then(|v| v.as_bool()))
        .unwrap_or(true)
}

#[tauri::command]
pub fn get_close_to_tray(app: AppHandle) -> bool {
    close_to_tray_enabled(&app)
}

#[tauri::command]
pub fn set_close_to_tray(app: AppHandle, enabled: bool) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(KEY_CLOSE_TO_TRAY, serde_json::Value::Bool(enabled));
    store.save().map_err(|e| e.to_string())
}

// -- Dictation language (whisper / cloud / parakeet) -----------------------

/// Lit la langue de dictee active. "auto" = auto-detect, code ISO sinon.
/// Stockee sous la cle `whisper_language` (legacy : la cle existait avant
/// l'ajout des sources cloud / parakeet, on la garde pour compat).
#[tauri::command]
pub fn get_dictation_language(app: AppHandle) -> String {
    pipeline::get_language(&app).unwrap_or_else(|| "auto".into())
}

/// Ecrit la langue de dictee. `code` = "auto" | "en" | "fr" | etc.
/// "auto" est stocke tel quel ; le pipeline le mappe vers None au runtime.
#[tauri::command]
pub fn set_dictation_language(app: AppHandle, code: String) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set("whisper_language", serde_json::Value::String(code));
    store.save().map_err(|e| e.to_string())
}

// -- System mute during recording (VoiceInk MediaController) ----------------

#[tauri::command]
pub fn get_system_mute_enabled(app: AppHandle) -> bool {
    crate::audio::mute::is_enabled(&app)
}

#[tauri::command]
pub fn set_system_mute_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    crate::audio::mute::set_enabled(&app, enabled).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_audio_resumption_delay(app: AppHandle) -> f64 {
    crate::audio::mute::resumption_delay(&app).as_secs_f64()
}

#[tauri::command]
pub fn set_audio_resumption_delay(app: AppHandle, secs: f64) -> Result<(), String> {
    crate::audio::mute::set_resumption_delay(&app, secs).map_err(|e| e.to_string())
}

// -- Sound feedback cues (VoiceInk SoundManager) ----------------------------

#[tauri::command]
pub fn get_sound_feedback_enabled(app: AppHandle) -> bool {
    crate::audio::feedback::is_enabled(&app)
}

#[tauri::command]
pub fn set_sound_feedback_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    crate::audio::feedback::set_enabled(&app, enabled).map_err(|e| e.to_string())
}
