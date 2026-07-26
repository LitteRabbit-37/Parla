// Commandes Tauri pour la gestion de l'enhancement LLM.
//
// Reference VoiceInk : l'UI appelle AIEnhancementService (toggles, custom prompts)
// + AIService (selection provider/model) + APIKeyManager.

use serde::Serialize;
use tauri::{command, AppHandle, Manager};

use crate::enhancement::prompts::{self, CustomPrompt};
use crate::enhancement::service::{self, EnhancementState};

// ---- Enable toggle --------------------------------------------------------

#[command]
pub fn get_enhancement_enabled(app: AppHandle) -> bool {
    prompts::is_enhancement_enabled(&app)
}

#[command]
pub fn set_enhancement_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    prompts::set_enhancement_enabled(&app, enabled).map_err(|e| e.to_string())
}

// ---- Prompts CRUD ---------------------------------------------------------

#[command]
pub fn list_prompts(app: AppHandle) -> Result<Vec<CustomPrompt>, String> {
    prompts::load_cached(&app).map_err(|e| e.to_string())
}

#[command]
pub fn add_prompt(app: AppHandle, prompt: CustomPrompt) -> Result<CustomPrompt, String> {
    let mut all = prompts::load_all(&app).map_err(|e| e.to_string())?;
    let mut p = prompt;
    if p.id.is_empty() {
        p.id = prompts::new_uuid();
    }
    all.push(p.clone());
    prompts::save_all(&app, &all).map_err(|e| e.to_string())?;
    prompts::invalidate_cache();
    Ok(p)
}

#[command]
pub fn update_prompt(app: AppHandle, prompt: CustomPrompt) -> Result<(), String> {
    let mut all = prompts::load_all(&app).map_err(|e| e.to_string())?;
    let pos = all
        .iter()
        .position(|p| p.id == prompt.id)
        .ok_or_else(|| format!("prompt introuvable: {}", prompt.id))?;
    all[pos] = prompt;
    prompts::save_all(&app, &all).map_err(|e| e.to_string())?;
    prompts::invalidate_cache();
    Ok(())
}

#[command]
pub fn delete_prompt(app: AppHandle, id: String) -> Result<(), String> {
    if id == prompts::ID_DEFAULT || id == prompts::ID_ASSISTANT {
        return Err("Impossible de supprimer un prompt predefini".into());
    }
    let mut all = prompts::load_all(&app).map_err(|e| e.to_string())?;
    all.retain(|p| p.id != id);
    prompts::save_all(&app, &all).map_err(|e| e.to_string())?;
    // Si c'etait l'actif, bascule sur Default.
    if prompts::get_active_prompt_id(&app).as_deref() == Some(&id) {
        prompts::set_active_prompt_id(&app, Some(prompts::ID_DEFAULT))
            .map_err(|e| e.to_string())?;
    }
    prompts::invalidate_cache();
    Ok(())
}

#[command]
pub fn get_active_prompt_id(app: AppHandle) -> Option<String> {
    prompts::get_active_prompt_id(&app)
}

#[command]
pub fn set_active_prompt_id(app: AppHandle, id: Option<String>) -> Result<(), String> {
    prompts::set_active_prompt_id(&app, id.as_deref()).map_err(|e| e.to_string())
}

#[command]
pub fn list_extra_templates() -> Vec<CustomPrompt> {
    prompts::extra_templates()
}

// ---- Providers LLM --------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct LLMProviderInfo {
    pub id: String,
    pub label: String,
    pub endpoint: String,
    pub default_model: String,
    pub models: Vec<String>,
    pub requires_api_key: bool,
    pub has_api_key: bool,
}

#[command]
pub fn list_llm_providers(app: AppHandle) -> Result<Vec<LLMProviderInfo>, String> {
    let state = app
        .try_state::<EnhancementState>()
        .ok_or_else(|| "EnhancementState absent".to_string())?;
    Ok(state
        .registry
        .list()
        .iter()
        .map(|p| LLMProviderInfo {
            id: p.id().into(),
            label: p.label().into(),
            endpoint: p.endpoint().into(),
            default_model: p.default_model().into(),
            models: p.default_models().iter().map(|m| (*m).into()).collect(),
            requires_api_key: p.requires_api_key(),
            has_api_key: p.requires_api_key() && crate::services::api_keys::has_api_key(p.id()),
        })
        .collect())
}

#[derive(Debug, Serialize)]
pub struct LLMSelection {
    pub provider_id: String,
    pub model: String,
}

#[command]
pub fn get_llm_selection(app: AppHandle) -> Option<LLMSelection> {
    service::get_selection(&app).map(|s| LLMSelection {
        provider_id: s.provider_id,
        model: s.model,
    })
}

#[command]
pub fn set_llm_selection(app: AppHandle, provider_id: String, model: String) -> Result<(), String> {
    service::set_selection(&app, &provider_id, &model).map_err(|e| e.to_string())
}

// ---- Ollama ---------------------------------------------------------------

#[command]
pub fn get_ollama_base_url(app: AppHandle) -> String {
    crate::enhancement::providers::ollama::get_base_url(&app)
}

#[command]
pub fn set_ollama_base_url(app: AppHandle, url: String) -> Result<(), String> {
    crate::enhancement::providers::ollama::set_base_url(&app, &url).map_err(|e| e.to_string())
}

#[command]
pub async fn list_ollama_models(app: AppHandle) -> Result<Vec<String>, String> {
    let base = crate::enhancement::providers::ollama::get_base_url(&app);
    crate::enhancement::providers::ollama::list_models(&base)
        .await
        .map_err(|e| e.to_string())
}

// ---- Custom provider ------------------------------------------------------

#[command]
pub fn get_custom_base_url(app: AppHandle) -> Option<String> {
    crate::enhancement::providers::custom::get_base_url(&app)
}

#[command]
pub fn set_custom_base_url(app: AppHandle, url: String) -> Result<(), String> {
    crate::enhancement::providers::custom::set_base_url(&app, &url).map_err(|e| e.to_string())
}

// ---- LocalCLI -------------------------------------------------------------

#[command]
pub fn get_localcli_custom_cmd(app: AppHandle) -> Option<String> {
    crate::enhancement::providers::local_cli::get_custom_cmd(&app)
}

#[command]
pub fn set_localcli_custom_cmd(app: AppHandle, cmd: String) -> Result<(), String> {
    crate::enhancement::providers::local_cli::set_custom_cmd(&app, &cmd).map_err(|e| e.to_string())
}

#[command]
pub fn get_localcli_timeout_secs(app: AppHandle) -> u64 {
    crate::enhancement::providers::local_cli::get_timeout_secs(&app)
}

#[command]
pub fn set_localcli_timeout_secs(app: AppHandle, secs: u64) -> Result<(), String> {
    crate::enhancement::providers::local_cli::set_timeout_secs(&app, secs)
        .map_err(|e| e.to_string())
}
