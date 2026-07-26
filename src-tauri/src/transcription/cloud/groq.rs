// Groq transcription cloud (whisper-large-v3-turbo) en batch.
//
// Reference VoiceInk : LLMkit OpenAITranscriptionClient.swift.
// Endpoint : POST https://api.groq.com/openai/v1/audio/transcriptions
// Headers : Authorization: Bearer <apiKey>
// Body multipart : file, model, language?, prompt?, response_format=json, temperature=0

use std::path::Path;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::Deserialize;

use super::http::{http_status_error, BatchHttpClient, HttpRequest, MultipartEncoder};
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
        let url = "https://api.groq.com/openai/v1/models";
        let client = BatchHttpClient::new(url)?;
        let request =
            HttpRequest::new("GET", url).header("Authorization", format!("Bearer {api_key}"));
        let response = client.send(request.clone()).await?;
        if !(200..300).contains(&response.status) {
            return Err(http_status_error(response.status, &response.body, &request));
        }
        Ok(())
    }

    async fn transcribe(
        &self,
        wav_path: &Path,
        api_key: &str,
        request: &TranscribeRequest,
    ) -> Result<String> {
        let audio = tokio::fs::read(wav_path).await?;
        let filename = wav_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("audio.wav");
        let mut form = MultipartEncoder::random()
            .file("file", filename, "audio/wav", audio)
            .field("model", request.model.clone())
            .field("response_format", "json")
            .field("temperature", "0");

        if let Some(lang) = request.language.as_deref() {
            if !lang.is_empty() && lang != "auto" {
                form = form.field("language", lang.to_string());
            }
        }
        if let Some(prompt) = request.prompt.as_deref() {
            if !prompt.is_empty() {
                form = form.field("prompt", prompt.to_string());
            }
        }

        let url = "https://api.groq.com/openai/v1/audio/transcriptions";
        let client = BatchHttpClient::new(url)?;
        let http_request = HttpRequest::new("POST", url)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", form.content_type())
            .body(form.try_encode()?);
        let response = client.send(http_request.clone()).await?;
        if !(200..300).contains(&response.status) {
            return Err(http_status_error(
                response.status,
                &response.body,
                &http_request,
            ));
        }

        // Fallback : si le JSON ne decode pas, essayer le body brut en UTF-8.
        match serde_json::from_slice::<GroqResponse>(&response.body) {
            Ok(r) => r.text.ok_or_else(|| anyhow!("reponse sans champ text")),
            Err(_) => Ok(String::from_utf8_lossy(&response.body).trim().to_string()),
        }
    }
}
