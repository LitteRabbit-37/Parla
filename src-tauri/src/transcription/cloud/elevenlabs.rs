// ElevenLabs Scribe v1/v2 en batch.
//
// Reference VoiceInk : LLMkit ElevenLabsClient.swift.
// Endpoint : POST https://api.elevenlabs.io/v1/speech-to-text
// Headers : xi-api-key: <key> (pas Bearer)
// Body multipart : file, model_id, temperature=0.0, tag_audio_events=false,
// language_code? (si langue non vide).

use std::path::Path;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::Deserialize;

use super::http::{http_status_error, BatchHttpClient, HttpRequest, MultipartEncoder};
use super::provider::{CloudTranscriptionProvider, TranscribeRequest};

pub struct ElevenLabsProvider;

#[derive(Debug, Deserialize)]
struct ElevenLabsResponse {
    text: String,
}

#[async_trait]
impl CloudTranscriptionProvider for ElevenLabsProvider {
    fn id(&self) -> &'static str {
        "elevenlabs"
    }

    async fn verify_api_key(&self, api_key: &str) -> Result<()> {
        let url = "https://api.elevenlabs.io/v1/user";
        let client = BatchHttpClient::new(url)?;
        let request = HttpRequest::new("GET", url).header("xi-api-key", api_key);
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
            .field("model_id", request.model.clone())
            .field("temperature", "0.0")
            .field("tag_audio_events", "false");

        if let Some(lang) = request.language.as_deref() {
            if !lang.is_empty() && lang != "auto" {
                form = form.field("language_code", lang.to_string());
            }
        }

        let url = "https://api.elevenlabs.io/v1/speech-to-text";
        let client = BatchHttpClient::new(url)?;
        let request = HttpRequest::new("POST", url)
            .header("xi-api-key", api_key)
            .header("Accept", "application/json")
            .header("Content-Type", form.content_type())
            .body(form.try_encode()?);
        let response = client.send(request.clone()).await?;
        if !(200..300).contains(&response.status) {
            return Err(http_status_error(response.status, &response.body, &request));
        }

        let parsed: ElevenLabsResponse =
            serde_json::from_slice(&response.body).map_err(|e| anyhow!("parse JSON: {e}"))?;
        Ok(parsed.text)
    }
}
