// Commandes Tauri pour gerer le catalogue local des modeles Whisper.

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::transcription::{ModelManager, ModelState};

pub struct ModelManagerState(pub Arc<ModelManager>);

impl ModelManagerState {
    pub fn new(app: AppHandle) -> Self {
        Self(Arc::new(ModelManager::new(app)))
    }
}

#[tauri::command]
pub async fn list_whisper_models(
    state: State<'_, ModelManagerState>,
) -> Result<Vec<ModelState>, String> {
    state.0.list().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn download_whisper_model(
    state: State<'_, ModelManagerState>,
    id: String,
) -> Result<String, String> {
    let mgr = state.0.clone();
    match mgr.download(&id).await {
        Ok(path) => Ok(path.to_string_lossy().into_owned()),
        Err(e) => Err(crate::services::download::diagnostic(&e)),
    }
}

#[tauri::command]
pub async fn cancel_download_whisper_model(
    state: State<'_, ModelManagerState>,
    id: String,
) -> Result<(), String> {
    state.0.cancel_download(&id);
    Ok(())
}

#[tauri::command]
pub async fn delete_whisper_model(
    state: State<'_, ModelManagerState>,
    id: String,
) -> Result<(), String> {
    if id.starts_with("imported:") {
        return state.0.delete_imported(&id).map_err(|e| e.to_string());
    }
    state.0.delete(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_whisper_model(
    state: State<'_, ModelManagerState>,
    path: String,
) -> Result<String, String> {
    let src = std::path::PathBuf::from(path);
    state.0.import(&src).map_err(|e| e.to_string())
}
