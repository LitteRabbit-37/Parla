// Soniox stt-async-v4 en batch (multi-step).
//
// Reference VoiceInk : LLMkit SonioxClient.swift.
// Sequence :
//   1. POST /v1/files (multipart file) -> { id }
//   2. POST /v1/transcriptions (JSON : file_id, model, enable_speaker_diarization=false,
//      language_hints? + language_hints_strict=true + enable_language_identification=true,
//      ou juste enable_language_identification=true si pas de langue,
//      context.terms[] si custom_vocabulary) -> { id }
//   3. GET /v1/transcriptions/{id} poll toutes les 1s jusqu'a status == "completed"
//   4. GET /v1/transcriptions/{id}/transcript -> { text } ou texte brut
//
// Tous les appels : Authorization: Bearer <apiKey>.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::http::{
    http_status_error, read_wav_with_filename, BatchHttpClient, HttpRequest, MultipartEncoder,
};
use super::provider::{CloudTranscriptionProvider, TranscribeRequest};

pub struct SonioxProvider;

const MAX_WAIT_SECS: u64 = 300;

#[derive(Debug, Deserialize)]
struct IdResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct StatusResponse {
    status: String,
}

#[derive(Debug, Deserialize)]
struct TranscriptJson {
    text: Option<String>,
}

#[async_trait]
impl CloudTranscriptionProvider for SonioxProvider {
    fn id(&self) -> &'static str {
        "soniox"
    }

    async fn verify_api_key(&self, api_key: &str) -> Result<()> {
        let client = BatchHttpClient::new("https://api.soniox.com/v1/files")?;
        let request = HttpRequest::new("GET", "https://api.soniox.com/v1/files")
            .header("Authorization", format!("Bearer {api_key}"));
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
        let client = BatchHttpClient::new("https://api.soniox.com/v1/files")?;

        // 1. Upload du fichier
        let (wav, filename) = read_wav_with_filename(wav_path).await?;
        let multipart = MultipartEncoder::random().file("file", filename, "audio/wav", wav);
        let upload_request = HttpRequest::new("POST", "https://api.soniox.com/v1/files")
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", multipart.content_type())
            .body(multipart.try_encode()?);
        let upload_response = client
            .send(upload_request.clone())
            .await
            .context("POST /v1/files")?;
        if !(200..300).contains(&upload_response.status) {
            return Err(http_status_error(
                upload_response.status,
                &upload_response.body,
                &upload_request,
            ));
        }
        let file_id = serde_json::from_slice::<IdResponse>(&upload_response.body)
            .context("parse /v1/files response")?
            .id;

        // 2. Creation de la transcription
        let mut body = serde_json::Map::new();
        body.insert("file_id".into(), json!(file_id));
        body.insert("model".into(), json!(request.model));
        body.insert("enable_speaker_diarization".into(), json!(false));

        match request.language.as_deref() {
            Some(lang) if !lang.is_empty() && lang != "auto" => {
                body.insert("language_hints".into(), json!([lang]));
                body.insert("language_hints_strict".into(), json!(true));
                body.insert("enable_language_identification".into(), json!(true));
            }
            _ => {
                body.insert("enable_language_identification".into(), json!(true));
            }
        }

        if !request.custom_vocabulary.is_empty() {
            body.insert(
                "context".into(),
                json!({ "terms": request.custom_vocabulary }),
            );
        }

        let json_body = serde_json::to_vec(&body)?;
        let transcription_request =
            HttpRequest::new("POST", "https://api.soniox.com/v1/transcriptions")
                .header("Authorization", format!("Bearer {api_key}"))
                .header("Content-Type", "application/json")
                .body(json_body);
        let transcription_response = client
            .send(transcription_request.clone())
            .await
            .context("POST /v1/transcriptions")?;
        if !(200..300).contains(&transcription_response.status) {
            return Err(http_status_error(
                transcription_response.status,
                &transcription_response.body,
                &transcription_request,
            ));
        }
        let trans_id = serde_json::from_slice::<IdResponse>(&transcription_response.body)
            .context("parse /v1/transcriptions response")?
            .id;

        // 3. Poll du statut
        let start = std::time::Instant::now();
        loop {
            if start.elapsed().as_secs() > MAX_WAIT_SECS {
                anyhow::bail!("timeout transcription Soniox");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            let status_request = HttpRequest::new(
                "GET",
                format!("https://api.soniox.com/v1/transcriptions/{trans_id}"),
            )
            .header("Authorization", format!("Bearer {api_key}"));
            let status_response = client.send(status_request.clone()).await?;
            if !(200..300).contains(&status_response.status) {
                return Err(http_status_error(
                    status_response.status,
                    &status_response.body,
                    &status_request,
                ));
            }
            let status: StatusResponse = serde_json::from_slice(&status_response.body)?;
            match status.status.as_str() {
                "completed" => break,
                "failed" => anyhow::bail!("transcription Soniox echouee"),
                _ => continue,
            }
        }

        // 4. Recuperation du transcript
        let transcript_request = HttpRequest::new(
            "GET",
            format!("https://api.soniox.com/v1/transcriptions/{trans_id}/transcript"),
        )
        .header("Authorization", format!("Bearer {api_key}"));
        let transcript_response = client.send(transcript_request.clone()).await?;
        if !(200..300).contains(&transcript_response.status) {
            return Err(http_status_error(
                transcript_response.status,
                &transcript_response.body,
                &transcript_request,
            ));
        }
        let body = transcript_response.body;
        match serde_json::from_slice::<TranscriptJson>(&body) {
            Ok(TranscriptJson { text: Some(t) }) => Ok(t),
            _ => Ok(String::from_utf8_lossy(&body).trim().to_string()),
        }
    }
}
