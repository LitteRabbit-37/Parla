// Mistral Voxtral en batch.
//
// Reference VoiceInk : LLMkit MistralTranscriptionClient.swift.
// Endpoint : POST https://api.mistral.ai/v1/audio/transcriptions
// Headers : x-api-key: <key> (pas Bearer pour la transcription).
// Body multipart : file, model. Pas de parametre language cote batch.
// verify_api_key : GET /v1/models avec Bearer (note VoiceInk).

use std::path::Path;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::multipart::Form;
use serde::Deserialize;

use super::http::{batch_client, map_http_err, wav_part_from_path};
use super::provider::{CloudTranscriptionProvider, TranscribeRequest};

pub struct MistralProvider;

#[derive(Debug, Deserialize)]
struct MistralResponse {
    text: String,
}

#[async_trait]
impl CloudTranscriptionProvider for MistralProvider {
    fn id(&self) -> &'static str {
        "mistral"
    }

    async fn verify_api_key(&self, api_key: &str) -> Result<()> {
        let client = batch_client("https://api.mistral.ai/v1/models")?;
        let resp = client
            .get("https://api.mistral.ai/v1/models")
            .bearer_auth(api_key)
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
        let form = Form::new()
            .text("model", request.model.clone())
            .part("file", wav_part_from_path(wav_path).await?);

        let client = batch_client("https://api.mistral.ai/v1/audio/transcriptions")?;
        let resp = client
            .post("https://api.mistral.ai/v1/audio/transcriptions")
            .header("x-api-key", api_key)
            .multipart(form)
            .send()
            .await
            .map_err(map_http_err)?;

        let status = resp.status();
        let body = resp.bytes().await?;
        if !status.is_success() {
            anyhow::bail!("HTTP {status}: {}", String::from_utf8_lossy(&body));
        }
        let parsed: MistralResponse =
            serde_json::from_slice(&body).map_err(|e| anyhow!("parse JSON Mistral: {e}"))?;
        Ok(parsed.text)
    }
}
