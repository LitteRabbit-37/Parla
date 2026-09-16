// Mistral chat completions.
//
// Reference VoiceInk : AIService.swift (case .mistral) - baseURL
// https://api.mistral.ai/v1/chat/completions.

use anyhow::Result;
use async_trait::async_trait;

use crate::enhancement::provider::{EnhancementRequest, EnhancementResponse, LLMProvider};

use super::openai_compat;

pub struct MistralProvider;

// VoiceInk 2.13 AIService.availableModels (.mistral), commit fda3169
// "Update Mistral model defaults" : small en premier et par defaut.
const MODELS: &[&str] = &[
    "mistral-small-latest",
    "mistral-medium-latest",
    "mistral-large-latest",
];

#[async_trait]
impl LLMProvider for MistralProvider {
    fn id(&self) -> &'static str {
        "mistral"
    }
    fn label(&self) -> &'static str {
        "Mistral"
    }
    fn default_models(&self) -> &'static [&'static str] {
        MODELS
    }
    fn default_model(&self) -> &'static str {
        "mistral-small-latest"
    }
    fn endpoint(&self) -> &'static str {
        "https://api.mistral.ai/v1/chat/completions"
    }

    async fn chat_completion(
        &self,
        api_key: &str,
        req: &EnhancementRequest,
    ) -> Result<EnhancementResponse> {
        openai_compat::chat_completion(self.endpoint(), api_key, req).await
    }
}
