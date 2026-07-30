// Google Gemini en batch (audio inline base64).
//
// Reference VoiceInk : LLMkit GeminiTranscriptionClient.swift.
// Endpoint : POST https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent
// Headers : x-goog-api-key: <key>, Content-Type: application/json
// Body JSON :
//   { contents: [ { parts: [
//       { text: "Please transcribe this audio file. Provide only the transcribed text." },
//       { inlineData: { mimeType: "audio/wav", data: "<base64>" } }
//   ] } ] }
// Reponse : candidates[0].content.parts[0].text (trim whitespace).
// Pas de parametre language ; auto-detect par le modele.

use std::path::Path;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Deserialize;
use serde_json::json;

use super::http::{http_status_error, BatchHttpClient, HttpRequest};
use super::provider::{CloudTranscriptionProvider, TranscribeRequest};

pub struct GeminiProvider;

const TRANSCRIPTION_PROMPT: &str =
    "Please transcribe this audio file. Provide only the transcribed text.";

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Vec<Candidate>,
}
#[derive(Debug, Deserialize)]
struct Candidate {
    content: Content,
}
#[derive(Debug, Deserialize)]
struct Content {
    parts: Vec<Part>,
}
#[derive(Debug, Deserialize)]
struct Part {
    text: Option<String>,
}

#[async_trait]
impl CloudTranscriptionProvider for GeminiProvider {
    fn id(&self) -> &'static str {
        "gemini"
    }

    async fn verify_api_key(&self, api_key: &str) -> Result<()> {
        let url = "https://generativelanguage.googleapis.com/v1beta/models";
        let client = BatchHttpClient::new(url)?;
        let request = HttpRequest::new("GET", url).header("x-goog-api-key", api_key);
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
        let b64 = B64.encode(&audio_bytes);

        let prompt = request
            .prompt
            .clone()
            .unwrap_or_else(|| TRANSCRIPTION_PROMPT.to_string());

        let body = json!({
            "contents": [ {
                "parts": [
                    { "text": prompt },
                    { "inlineData": { "mimeType": "audio/wav", "data": b64 } }
                ]
            } ]
        });

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            request.model
        );

        let client = BatchHttpClient::new(&url)?;
        let request = HttpRequest::new("POST", &url)
            .header("x-goog-api-key", api_key)
            .header("Content-Type", "application/json")
            .body(serde_json::to_vec(&body)?);
        let response = client.send(request.clone()).await?;
        if !(200..300).contains(&response.status) {
            return Err(http_status_error(response.status, &response.body, &request));
        }

        let parsed: GeminiResponse = serde_json::from_slice(&response.body)
            .map_err(|e| anyhow!("parse JSON Gemini: {e}"))?;
        let text = parsed
            .candidates
            .first()
            .and_then(|c| c.content.parts.first())
            .and_then(|p| p.text.clone())
            .ok_or_else(|| anyhow!("reponse Gemini sans texte"))?;
        Ok(text.trim().to_string())
    }
}
