// Ollama local.
//
// Reference VoiceInk : LLMkit OllamaClient.swift + Services/OllamaService.swift.
// Base URL par defaut : http://localhost:11434.
// Endpoints :
//   GET  /api/tags     -> liste des modeles (champ models[].name)
//   POST /api/generate -> generation non-streamee
//     body: { "model", "prompt", "system", "options": {"temperature": 0.3},
//             "stream": false }
//     resp: { "response": "..." }
//
// La base URL est parametrable cote user (store parla.settings.json clef
// "ollama_base_url"). Aucune cle API requise. Pas de rate limit.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tauri_plugin_store::StoreExt;

use crate::enhancement::provider::{EnhancementRequest, EnhancementResponse, LLMProvider};

use super::url_validator;

pub struct OllamaProvider;

const DEFAULT_BASE_URL: &str = "http://localhost:11434";
const STORE_FILE: &str = "parla.settings.json";
const KEY_BASE_URL: &str = "ollama_base_url";

pub fn get_base_url(app: &tauri::AppHandle) -> String {
    app.store(STORE_FILE)
        .ok()
        .and_then(|s| {
            s.get(KEY_BASE_URL)
                .and_then(|v| v.as_str().map(String::from))
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
}

pub fn set_base_url(app: &tauri::AppHandle, url: &str) -> Result<()> {
    let store = app.store(STORE_FILE).map_err(|e| anyhow!("store: {e}"))?;
    let trimmed = url.trim();
    if trimmed.is_empty() {
        store.delete(KEY_BASE_URL);
    } else {
        // Ollama tourne typiquement en local - on tolere http sur loopback,
        // mais on refuse http vers un host externe.
        let canonical = url_validator::validate_endpoint(trimmed, true)?;
        store.set(KEY_BASE_URL, serde_json::Value::String(canonical));
    }
    store.save().map_err(|e| anyhow!("store save: {e}"))?;
    Ok(())
}

/// Liste les modeles Ollama installes sur la machine.
pub async fn list_models(base_url: &str) -> Result<Vec<String>> {
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let parsed_url = reqwest::Url::parse(&url)?;
    let resp = crate::services::proxy::apply_for_url(reqwest::Client::builder(), &parsed_url)?
        .build()?
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow!("ollama list: {e}"))?;
    if !resp.status().is_success() {
        return Err(anyhow!("ollama list http {}", resp.status()));
    }
    let json: Value = resp.json().await.map_err(|e| anyhow!("ollama json: {e}"))?;
    let mut names = Vec::new();
    if let Some(arr) = json.get("models").and_then(|v| v.as_array()) {
        for m in arr {
            if let Some(name) = m.get("name").and_then(|v| v.as_str()) {
                names.push(name.to_string());
            }
        }
    }
    Ok(names)
}

#[async_trait]
impl LLMProvider for OllamaProvider {
    fn id(&self) -> &'static str {
        "ollama"
    }
    fn label(&self) -> &'static str {
        "Ollama (local)"
    }
    fn default_models(&self) -> &'static [&'static str] {
        &[]
    }
    fn default_model(&self) -> &'static str {
        "mistral"
    }
    fn endpoint(&self) -> &'static str {
        DEFAULT_BASE_URL
    }
    fn requires_api_key(&self) -> bool {
        false
    }
    fn rate_limited(&self) -> bool {
        false
    }

    async fn chat_completion(
        &self,
        _api_key: &str,
        req: &EnhancementRequest,
    ) -> Result<EnhancementResponse> {
        let base = req
            .endpoint_override
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let url = format!("{}/api/generate", base.trim_end_matches('/'));
        let body = json!({
            "model": req.model,
            "prompt": req.user_message,
            "system": req.system_prompt,
            "options": { "temperature": req.temperature },
            "stream": false,
        });
        let parsed_url = reqwest::Url::parse(&url)?;
        let client =
            crate::services::proxy::apply_for_url(reqwest::Client::builder(), &parsed_url)?
                .timeout(req.timeout)
                .build()
                .map_err(|e| anyhow!("http client: {e}"))?;
        let resp = client.post(&url).json(&body).send().await.map_err(|e| {
            if e.is_timeout() {
                anyhow!("timeout: {e}")
            } else if e.is_connect() {
                anyhow!("network_error: ollama inaccessible: {e}")
            } else {
                anyhow!("http: {e}")
            }
        })?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let truncated: String = body.chars().take(500).collect();
            if status.as_u16() == 404 {
                return Err(anyhow!("ollama model introuvable: {truncated}"));
            }
            return Err(anyhow!("http {status}: {truncated}"));
        }
        let json: Value = resp.json().await.map_err(|e| anyhow!("json: {e}"))?;
        let text = json
            .get("response")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("reponse Ollama sans champ response"))?;
        Ok(EnhancementResponse { text: text.into() })
    }
}
