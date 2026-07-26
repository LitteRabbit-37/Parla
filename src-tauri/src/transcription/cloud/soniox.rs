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
use reqwest::multipart::Form;
use serde::Deserialize;
use serde_json::json;

use super::http::{batch_client, map_http_err, wav_part_from_path};
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
        let client = batch_client("https://api.soniox.com/v1/files")?;
        let resp = client
            .get("https://api.soniox.com/v1/files")
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
        let client = batch_client("https://api.soniox.com/v1/files")?;

        // 1. Upload du fichier
        let form = Form::new().part("file", wav_part_from_path(wav_path).await?);
        let file_id = client
            .post("https://api.soniox.com/v1/files")
            .bearer_auth(api_key)
            .multipart(form)
            .send()
            .await
            .context("POST /v1/files")?
            .error_for_status()?
            .json::<IdResponse>()
            .await
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

        let trans_id = client
            .post("https://api.soniox.com/v1/transcriptions")
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await
            .context("POST /v1/transcriptions")?
            .error_for_status()?
            .json::<IdResponse>()
            .await
            .context("parse /v1/transcriptions response")?
            .id;

        // 3. Poll du statut
        let start = std::time::Instant::now();
        loop {
            if start.elapsed().as_secs() > MAX_WAIT_SECS {
                anyhow::bail!("timeout transcription Soniox");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            let status: StatusResponse = client
                .get(format!(
                    "https://api.soniox.com/v1/transcriptions/{trans_id}"
                ))
                .bearer_auth(api_key)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            match status.status.as_str() {
                "completed" => break,
                "failed" => anyhow::bail!("transcription Soniox echouee"),
                _ => continue,
            }
        }

        // 4. Recuperation du transcript
        let resp = client
            .get(format!(
                "https://api.soniox.com/v1/transcriptions/{trans_id}/transcript"
            ))
            .bearer_auth(api_key)
            .send()
            .await?
            .error_for_status()?;
        let body = resp.bytes().await?;
        match serde_json::from_slice::<TranscriptJson>(&body) {
            Ok(TranscriptJson { text: Some(t) }) => Ok(t),
            _ => Ok(String::from_utf8_lossy(&body).trim().to_string()),
        }
    }
}
