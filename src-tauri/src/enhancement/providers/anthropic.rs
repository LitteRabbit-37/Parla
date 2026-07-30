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

pub fn build_request(
    endpoint: &str,
    api_key: &str,
    req: &EnhancementRequest,
) -> Result<crate::transcription::cloud::http::HttpRequest> {
    let body = json!({
        "model": req.model,
        "system": req.system_prompt,
        "messages": [{"role": "user", "content": req.user_message}],
        "max_tokens": DEFAULT_MAX_TOKENS,
        "temperature": req.temperature,
    });
    Ok(
        crate::transcription::cloud::http::HttpRequest::new("POST", endpoint)
            .header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&body)?),
    )
}

pub fn parse_response(body: &[u8]) -> Result<EnhancementResponse> {
    let json: Value = serde_json::from_slice(body).map_err(|e| anyhow!("json parse: {e}"))?;
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
        let request = build_request(self.endpoint(), api_key, req)?;
        let response = crate::transcription::cloud::http::BatchHttpClient::new(self.endpoint())?
            .send_with_timeout(request.clone(), req.timeout)
            .await?;
        if !(200..300).contains(&response.status) {
            let detail = crate::transcription::cloud::http::http_status_error(
                response.status,
                &response.body,
                &request,
            );
            if response.status == 429 {
                return Err(anyhow!("rate_limit ({}){}", response.status, detail));
            }
            if response.status >= 500 {
                return Err(anyhow!("server_error ({}){}", response.status, detail));
            }
            return Err(detail);
        }

        parse_response(&response.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enhancement::provider::{EnhancementRequest, ReasoningConfig};
    use std::time::Duration;

    fn request() -> EnhancementRequest {
        EnhancementRequest {
            system_prompt: "system".into(),
            user_message: "user".into(),
            model: "claude-test".into(),
            temperature: 0.3,
            reasoning: ReasoningConfig::default(),
            timeout: Duration::from_secs(1),
            endpoint_override: None,
        }
    }

    #[test]
    fn request_and_response_match_messages_api() {
        let request =
            build_request("https://example.test/v1/messages", "secret", &request()).unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.headers[0], ("x-api-key".into(), "secret".into()));
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["messages"][0]["content"], "user");
        assert_eq!(
            parse_response(
                br#"{"content":[{"type":"text","text":"one"},{"type":"text","text":"two"}]}"#
            )
            .unwrap()
            .text,
            "onetwo"
        );
    }
}
