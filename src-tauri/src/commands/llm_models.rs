// Commandes Tauri pour le catalogue GGUF local (llama.cpp embarque).

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{command, AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

pub use crate::enhancement::model_manager::GgufModelManagerState;
use crate::enhancement::model_manager::{GgufModelManager, GgufModelState};
use crate::enhancement::providers::llamacpp;

pub fn ensure_state(app: &AppHandle) -> Arc<GgufModelManager> {
    if let Some(s) = app.try_state::<GgufModelManagerState>() {
        return s.0.clone();
    }
    let s = GgufModelManagerState::new(app.clone());
    let arc = s.0.clone();
    app.manage(s);
    arc
}

#[command]
pub fn list_gguf_models(app: AppHandle) -> Result<Vec<GgufModelState>, String> {
    let mgr = ensure_state(&app);
    mgr.list().map_err(|e| e.to_string())
}

#[command]
pub async fn download_gguf_model(app: AppHandle, id: String) -> Result<String, String> {
    let mgr = ensure_state(&app);
    mgr.download(&id)
        .await
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| crate::services::download::diagnostic(&e))
}

#[command]
pub fn cancel_download_gguf_model(app: AppHandle, id: String) {
    let mgr = ensure_state(&app);
    mgr.cancel_download(&id);
}

#[command]
pub fn delete_gguf_model(app: AppHandle, id: String) -> Result<(), String> {
    let mgr = ensure_state(&app);
    // Si c'est le modele actif, decharger le runtime pour liberer la RAM/VRAM.
    if llamacpp::get_selected_gguf(&app).as_deref() == Some(id.as_str()) {
        if let Some(rt) = crate::enhancement::service::llama_runtime() {
            rt.unload();
        }
        let _ = llamacpp::set_selected_gguf(&app, None);
    }
    mgr.delete(&id).map_err(|e| e.to_string())
}

#[command]
pub async fn import_gguf_model(app: AppHandle) -> Result<String, String> {
    // Native file picker -> .gguf only.
    let file = app
        .dialog()
        .file()
        .add_filter("GGUF", &["gguf"])
        .blocking_pick_file();
    let Some(fp) = file else {
        return Err("Import annule".into());
    };
    let p: PathBuf = fp
        .as_path()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "chemin invalide".to_string())?;
    let mgr = ensure_state(&app);
    mgr.import(&p).map_err(|e| e.to_string())
}

// ---- Selection + inference settings ---------------------------------------

#[command]
pub fn get_selected_gguf(app: AppHandle) -> Option<String> {
    llamacpp::get_selected_gguf(&app)
}

#[command]
pub fn set_selected_gguf(app: AppHandle, id: Option<String>) -> Result<(), String> {
    // Si l'ID change, decharger le modele actuellement en memoire.
    let prev = llamacpp::get_selected_gguf(&app);
    if prev != id {
        if let Some(rt) = crate::enhancement::service::llama_runtime() {
            rt.unload();
        }
    }
    llamacpp::set_selected_gguf(&app, id.as_deref()).map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
pub struct LlamaCppSettings {
    pub n_gpu_layers: u32,
    pub context_size: u32,
    pub max_tokens: u32,
}

#[command]
pub fn get_llamacpp_settings(app: AppHandle) -> LlamaCppSettings {
    LlamaCppSettings {
        n_gpu_layers: llamacpp::get_n_gpu_layers(&app),
        context_size: llamacpp::get_context_size(&app),
        max_tokens: llamacpp::get_max_tokens(&app),
    }
}

#[command]
pub fn set_llamacpp_settings(
    app: AppHandle,
    n_gpu_layers: u32,
    context_size: u32,
    max_tokens: u32,
) -> Result<(), String> {
    // Tout changement de params impose un rechargement : decharge maintenant.
    if let Some(rt) = crate::enhancement::service::llama_runtime() {
        rt.unload();
    }
    llamacpp::set_n_gpu_layers(&app, n_gpu_layers).map_err(|e| e.to_string())?;
    llamacpp::set_context_size(&app, context_size).map_err(|e| e.to_string())?;
    llamacpp::set_max_tokens(&app, max_tokens).map_err(|e| e.to_string())?;
    Ok(())
}

#[command]
pub fn llamacpp_cuda_enabled() -> bool {
    cfg!(feature = "cuda-llama")
}
