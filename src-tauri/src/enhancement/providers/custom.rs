// Custom OpenAI-compatible provider.
//
// Reference VoiceInk : AIService.swift (case .custom). L'endpoint est libre,
// stocke dans parla.settings.json clef "llm_custom_base_url". Le chemin
// /v1/chat/completions est ajoute par le client.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tauri_plugin_store::StoreExt;

use crate::enhancement::provider::{EnhancementRequest, EnhancementResponse, LLMProvider};

use super::openai_compat;
use super::url_validator;

pub struct CustomProvider;

const STORE_FILE: &str = "parla.settings.json";
const KEY_BASE_URL: &str = "llm_custom_base_url";

pub fn get_base_url(app: &tauri::AppHandle) -> Option<String> {
    app.store(STORE_FILE)
        .ok()
        .and_then(|s| {
            s.get(KEY_BASE_URL)
                .and_then(|v| v.as_str().map(String::from))
        })
        .filter(|s| !s.is_empty())
}

pub fn set_base_url(app: &tauri::AppHandle, url: &str) -> Result<()> {
    let store = app.store(STORE_FILE).map_err(|e| anyhow!("store: {e}"))?;
    let trimmed = url.trim();
    if trimmed.is_empty() {
        store.delete(KEY_BASE_URL);
    } else {
        // Custom endpoints sont exposes publiquement (Internet) - on exige
        // https. Les users avec un proxy local peuvent toujours passer par
        // Ollama qui tolere http loopback.
        let canonical = url_validator::validate_endpoint(trimmed, false)?;
        store.set(KEY_BASE_URL, serde_json::Value::String(canonical));
    }
    store.save().map_err(|e| anyhow!("store save: {e}"))?;
    Ok(())
}

#[async_trait]
impl LLMProvider for CustomProvider {
    fn id(&self) -> &'static str {
        "custom"
    }
    fn label(&self) -> &'static str {
        "Custom OpenAI-compatible"
    }
    fn default_models(&self) -> &'static [&'static str] {
        &[]
    }
    fn default_model(&self) -> &'static str {
        ""
    }
    fn endpoint(&self) -> &'static str {
        ""
    }
    fn requires_api_key(&self) -> bool {
        false
    }

    async fn chat_completion(
        &self,
        api_key: &str,
        req: &EnhancementRequest,
    ) -> Result<EnhancementResponse> {
        let base = req
            .endpoint_override
            .as_deref()
            .ok_or_else(|| anyhow!("Custom provider: base URL non configuree"))?;
        // Attend une base URL type https://host/v1 ; on complete /chat/completions.
        let endpoint = format!("{}/chat/completions", base.trim_end_matches('/'));
        openai_compat::chat_completion(&endpoint, api_key, req).await
    }
}
