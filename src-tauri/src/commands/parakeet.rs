// Commandes Tauri pour les modeles Parakeet (NVIDIA NeMo via parakeet-rs).

use std::sync::Arc;

use tauri::{command, AppHandle, Manager};

pub use crate::transcription::parakeet_model_manager::ParakeetModelManagerState;
use crate::transcription::parakeet_model_manager::{ParakeetModelManager, ParakeetModelState};

pub fn ensure_state(app: &AppHandle) -> Arc<ParakeetModelManager> {
    if let Some(s) = app.try_state::<ParakeetModelManagerState>() {
        return s.0.clone();
    }
    let s = ParakeetModelManagerState::new(app.clone());
    let arc = s.0.clone();
    app.manage(s);
    arc
}

#[command]
pub fn list_parakeet_models(app: AppHandle) -> Result<Vec<ParakeetModelState>, String> {
    let mgr = ensure_state(&app);
    mgr.list().map_err(|e| e.to_string())
}

#[command]
pub async fn download_parakeet_model(app: AppHandle, id: String) -> Result<String, String> {
    let mgr = ensure_state(&app);
    mgr.download(&id)
        .await
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| crate::services::download::diagnostic(&e))
}

#[command]
pub fn cancel_download_parakeet_model(app: AppHandle, id: String) {
    let mgr = ensure_state(&app);
    mgr.cancel_download(&id);
}

#[command]
pub fn delete_parakeet_model(app: AppHandle, id: String) -> Result<(), String> {
    let mgr = ensure_state(&app);
    // Decharge le modele si c'est celui qui est actif.
    if let Some(engine) = app.try_state::<crate::transcription::parakeet::ParakeetEngineState>() {
        engine.0.unload();
    }
    mgr.delete(&id).map_err(|e| e.to_string())
}

#[command]
pub fn parakeet_execution_provider() -> &'static str {
    #[cfg(feature = "cuda-onnx")]
    {
        "cuda"
    }
    #[cfg(all(not(feature = "cuda-onnx"), feature = "directml-onnx"))]
    {
        "directml"
    }
    #[cfg(all(not(feature = "cuda-onnx"), not(feature = "directml-onnx")))]
    {
        "cpu"
    }
}
