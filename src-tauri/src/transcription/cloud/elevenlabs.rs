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
use reqwest::multipart::Form;
use serde::Deserialize;

use super::http::{batch_client, map_http_err, wav_part_from_path};
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
        let client = batch_client("https://api.elevenlabs.io/v1/user")?;
        let resp = client
            .get("https://api.elevenlabs.io/v1/user")
            .header("xi-api-key", api_key)
            .send()
            .await
            .map_err(map_http_err)?;
        if !resp.status().is_success() {
            anyhow::bail!("HTTP {}", resp.status());
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
            .text("model_id", request.model.clone())
            .text("temperature", "0.0")
            .text("tag_audio_events", "false");

        if let Some(lang) = request.language.as_deref() {
            if !lang.is_empty() && lang != "auto" {
                form = form.text("language_code", lang.to_string());
            }
        }

        let client = batch_client("https://api.elevenlabs.io/v1/speech-to-text")?;
        let resp = client
            .post("https://api.elevenlabs.io/v1/speech-to-text")
            .header("xi-api-key", api_key)
            .header("Accept", "application/json")
            .multipart(form)
            .send()
            .await
            .map_err(map_http_err)?;

        let status = resp.status();
        let body = resp.bytes().await?;
        if !status.is_success() {
            anyhow::bail!("HTTP {status}: {}", String::from_utf8_lossy(&body));
        }

        let parsed: ElevenLabsResponse =
            serde_json::from_slice(&body).map_err(|e| anyhow!("parse JSON: {e}"))?;
        Ok(parsed.text)
    }
}
