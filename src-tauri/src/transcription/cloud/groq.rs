// Groq transcription cloud (whisper-large-v3-turbo) en batch.
//
// Reference VoiceInk : LLMkit OpenAITranscriptionClient.swift.
// Endpoint : POST https://api.groq.com/openai/v1/audio/transcriptions
// Headers : Authorization: Bearer <apiKey>
// Body multipart : file, model, language?, prompt?, response_format=json, temperature=0

use std::path::Path;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::multipart::Form;
use serde::Deserialize;

use super::http::{batch_client, map_http_err, wav_part_from_path};
use super::provider::{CloudTranscriptionProvider, TranscribeRequest};

pub struct GroqProvider;

#[derive(Debug, Deserialize)]
struct GroqResponse {
    text: Option<String>,
}

#[async_trait]
impl CloudTranscriptionProvider for GroqProvider {
    fn id(&self) -> &'static str {
        "groq"
    }

    async fn verify_api_key(&self, api_key: &str) -> Result<()> {
        let client = batch_client("https://api.groq.com/openai/v1/models")?;
        let resp = client
            .get("https://api.groq.com/openai/v1/models")
            .bearer_auth(api_key)
            .send()
            .await
            .map_err(map_http_err)?;
        if !resp.status().is_success() {
            anyhow::bail!("HTTP {} (cle API invalide ?)", resp.status());
        }
        Ok(())
    }

    async fn transcribe(
        &self,
        wav_path: &Path,
        api_key: &str,
        request: &TranscribeRequest,
    ) -> Result<String> {
        let mut form = Form::new()
            .part("file", wav_part_from_path(wav_path).await?)
            .text("model", request.model.clone())
            .text("response_format", "json")
            .text("temperature", "0");

        if let Some(lang) = request.language.as_deref() {
            if !lang.is_empty() && lang != "auto" {
                form = form.text("language", lang.to_string());
            }
        }
        if let Some(prompt) = request.prompt.as_deref() {
            if !prompt.is_empty() {
                form = form.text("prompt", prompt.to_string());
            }
        }

        let client = batch_client("https://api.groq.com/openai/v1/audio/transcriptions")?;
        let resp = client
            .post("https://api.groq.com/openai/v1/audio/transcriptions")
            .bearer_auth(api_key)
            .multipart(form)
            .send()
            .await
            .map_err(map_http_err)?;

        let status = resp.status();
        let body = resp.bytes().await?;
        if !status.is_success() {
            let msg = String::from_utf8_lossy(&body);
            anyhow::bail!("HTTP {status}: {msg}");
        }

        // Fallback : si le JSON ne decode pas, essayer le body brut en UTF-8.
        match serde_json::from_slice::<GroqResponse>(&body) {
            Ok(r) => r.text.ok_or_else(|| anyhow!("reponse sans champ text")),
            Err(_) => Ok(String::from_utf8_lossy(&body).trim().to_string()),
        }
    }
}
