// Deepgram nova-3 / nova-3-medical en batch.
//
// Reference VoiceInk : LLMkit DeepgramClient.swift.
// Endpoint : POST https://api.deepgram.com/v1/listen?model=...&smart_format=true
//   &punctuate=true&paragraphs=true[&language=...]
// Headers : Authorization: Token <key>, Content-Type: audio/wav
// Body : raw audio bytes.
// Reponse : results.channels[0].alternatives[0].transcript.

use std::path::Path;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::Deserialize;

use super::http::{http_status_error, BatchHttpClient, HttpRequest};
use super::provider::{CloudTranscriptionProvider, TranscribeRequest};

pub struct DeepgramProvider;

#[derive(Debug, Deserialize)]
struct DeepgramResponse {
    results: Results,
}
#[derive(Debug, Deserialize)]
struct Results {
    channels: Vec<Channel>,
}
#[derive(Debug, Deserialize)]
struct Channel {
    alternatives: Vec<Alternative>,
}
#[derive(Debug, Deserialize)]
struct Alternative {
    transcript: String,
}

#[async_trait]
impl CloudTranscriptionProvider for DeepgramProvider {
    fn id(&self) -> &'static str {
        "deepgram"
    }

    async fn verify_api_key(&self, api_key: &str) -> Result<()> {
        let client = BatchHttpClient::new("https://api.deepgram.com/v1/projects")?;
        let request = HttpRequest::new("GET", "https://api.deepgram.com/v1/projects")
            .header("Authorization", format!("Token {api_key}"));
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
        let audio_bytes = tokio::fs::read(wav_path).await?;

        let mut url = format!(
            "https://api.deepgram.com/v1/listen?model={}&smart_format=true&punctuate=true&paragraphs=true",
            urlencoding::encode(&request.model)
        );
        if let Some(lang) = request.language.as_deref() {
            if !lang.is_empty() && lang != "auto" {
                url.push_str(&format!("&language={}", urlencoding::encode(lang)));
            }
        }
        for term in &request.custom_vocabulary {
            url.push_str(&format!("&keyterm={}", urlencoding::encode(term)));
        }

        let client = BatchHttpClient::new(&url)?;
        let request = HttpRequest::new("POST", &url)
            .header("Authorization", format!("Token {api_key}"))
            .header("Content-Type", "audio/wav")
            .body(audio_bytes);
        let response = client.send(request.clone()).await?;
        if !(200..300).contains(&response.status) {
            return Err(http_status_error(response.status, &response.body, &request));
        }
        let parsed: DeepgramResponse = serde_json::from_slice(&response.body)
            .map_err(|e| anyhow!("parse JSON Deepgram: {e}"))?;
        let transcript = parsed
            .results
            .channels
            .first()
            .and_then(|c| c.alternatives.first())
            .map(|a| a.transcript.clone())
            .unwrap_or_default();
        Ok(transcript)
    }
}
