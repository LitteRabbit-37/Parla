// Anthropic Messages API.
//
// Reference VoiceInk : LLMkit AnthropicLLMClient.swift + AIService.swift
// (case .anthropic).
// POST https://api.anthropic.com/v1/messages
// Headers:
//   x-api-key: {api_key}
//   anthropic-version: 2023-06-01
//   content-type: application/json
// Body:
//   {
//     "model": ...,
//     "system": system_prompt,
//     "messages": [{"role": "user", "content": user_message}],
//     "max_tokens": 4096,
//     "temperature": f32
//   }
// Response : { "content": [{"type":"text","text": "..."}], ... }

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::enhancement::provider::{EnhancementRequest, EnhancementResponse, LLMProvider};

pub struct AnthropicProvider;

const MODELS: &[&str] = &[
    "claude-opus-4-6",
    "claude-sonnet-4-6",
    "claude-opus-4-5",
    "claude-sonnet-4-5",
    "claude-haiku-4-5",
];

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 4096;

#[async_trait]
impl LLMProvider for AnthropicProvider {
    fn id(&self) -> &'static str {
        "anthropic"
    }
    fn label(&self) -> &'static str {
        "Anthropic"
    }
    fn default_models(&self) -> &'static [&'static str] {
        MODELS
    }
    fn default_model(&self) -> &'static str {
        "claude-sonnet-4-6"
    }
    fn endpoint(&self) -> &'static str {
        "https://api.anthropic.com/v1/messages"
    }

    async fn chat_completion(
        &self,
        api_key: &str,
        req: &EnhancementRequest,
    ) -> Result<EnhancementResponse> {
        let body = json!({
            "model": req.model,
            "system": req.system_prompt,
            "messages": [{"role": "user", "content": req.user_message}],
            "max_tokens": DEFAULT_MAX_TOKENS,
            "temperature": req.temperature,
        });

        let url = reqwest::Url::parse(self.endpoint())?;
        let client = crate::services::proxy::apply_for_url(reqwest::Client::builder(), &url)?
            .timeout(req.timeout)
            .build()
            .map_err(|e| anyhow!("http client: {e}"))?;

        let resp = client
            .post(self.endpoint())
            .header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    anyhow!("timeout: {e}")
                } else if e.is_connect() {
                    anyhow!("network_error: {e}")
                } else {
                    anyhow!("http: {e}")
                }
            })?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let truncated: String = body.chars().take(500).collect();
            if status.as_u16() == 429 {
                return Err(anyhow!("rate_limit ({status}): {truncated}"));
            }
            if status.is_server_error() {
                return Err(anyhow!("server_error ({status}): {truncated}"));
            }
            return Err(anyhow!("http {status}: {truncated}"));
        }

        let json: Value = resp.json().await.map_err(|e| anyhow!("json parse: {e}"))?;
        // Concatene tous les blocs content de type text (VoiceInk ne prend que
        // le premier mais Anthropic peut en renvoyer plusieurs).
        let mut out = String::new();
        if let Some(arr) = json.get("content").and_then(|v| v.as_array()) {
            for item in arr {
                if item.get("type").and_then(|v| v.as_str()) == Some("text") {
                    if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                        out.push_str(t);
                    }
                }
            }
        }
        if out.is_empty() {
            return Err(anyhow!("reponse Anthropic sans content.text"));
        }
        Ok(EnhancementResponse { text: out })
    }
}
