// Groq chat completions.
//
// Reference VoiceInk : AIService.swift (case .groq) - baseURL
// https://api.groq.com/openai/v1/chat/completions.

use anyhow::Result;
use async_trait::async_trait;

use crate::enhancement::provider::{EnhancementRequest, EnhancementResponse, LLMProvider};

use super::openai_compat;

pub struct GroqProvider;

// VoiceInk 2.13 AIService.availableModels (.groq).
const MODELS: &[&str] = &[
    "openai/gpt-oss-120b",
    "openai/gpt-oss-20b",
];

#[async_trait]
impl LLMProvider for GroqProvider {
    fn id(&self) -> &'static str {
        "groq"
    }
    fn label(&self) -> &'static str {
        "Groq"
    }
    fn default_models(&self) -> &'static [&'static str] {
        MODELS
    }
    fn default_model(&self) -> &'static str {
        "openai/gpt-oss-120b"
    }
    fn endpoint(&self) -> &'static str {
        "https://api.groq.com/openai/v1/chat/completions"
    }

    async fn chat_completion(
        &self,
        api_key: &str,
        req: &EnhancementRequest,
    ) -> Result<EnhancementResponse> {
        openai_compat::chat_completion(self.endpoint(), api_key, req).await
    }
}
