// Commandes Tauri pour le modele VAD Silero (download/delete/state/enable).

use serde_json::Value;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_store::StoreExt;

use crate::transcription::vad::{self, VadEngine, VadModelState};

const STORE_FILE: &str = "parla.settings.json";
const VAD_ENABLED_KEY: &str = "vad_enabled";

pub struct VadEngineState(pub std::sync::Arc<VadEngine>);

impl Default for VadEngineState {
    fn default() -> Self {
        Self(std::sync::Arc::new(VadEngine::default()))
    }
}

#[tauri::command]
pub fn vad_get_state(app: AppHandle) -> VadModelState {
    vad::vad_state(&app)
}

#[tauri::command]
pub async fn vad_download(app: AppHandle) -> Result<String, String> {
    match vad::download_vad(&app).await {
        Ok(path) => Ok(path.to_string_lossy().into_owned()),
        Err(error) => {
            let message = crate::services::download::diagnostic(&error);
            let _ = app.emit("vad:download:error", serde_json::json!({ "message": message }));
            Err(message)
        }
    }
}

#[tauri::command]
pub fn vad_delete(app: AppHandle) -> Result<(), String> {
    vad::delete_vad(&app).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn vad_is_enabled(app: AppHandle) -> bool {
    let Some(store) = app.store(STORE_FILE).ok() else {
        return false;
    };
    store
        .get(VAD_ENABLED_KEY)
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

#[tauri::command]
pub fn vad_set_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(VAD_ENABLED_KEY, Value::Bool(enabled));
    store.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn vad_is_ready(app: AppHandle, _engine: State<'_, VadEngineState>) -> bool {
    // Ready = le modele existe sur disque ET vad_is_enabled = true.
    vad_is_enabled(app.clone()) && vad::vad_state(&app).downloaded
}
