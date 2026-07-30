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
use serde::Deserialize;

use super::http::{http_status_error, BatchHttpClient, HttpRequest, MultipartEncoder};
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
        let url = "https://api.mistral.ai/v1/models";
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
        let form = MultipartEncoder::random()
            .field("model", request.model.clone())
            .file("file", filename, "audio/wav", audio);

        let url = "https://api.mistral.ai/v1/audio/transcriptions";
        let client = BatchHttpClient::new(url)?;
        let http_request = HttpRequest::new("POST", url)
            .header("x-api-key", api_key)
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
        let parsed: MistralResponse = serde_json::from_slice(&response.body)
            .map_err(|e| anyhow!("parse JSON Mistral: {e}"))?;
        Ok(parsed.text)
    }
}
