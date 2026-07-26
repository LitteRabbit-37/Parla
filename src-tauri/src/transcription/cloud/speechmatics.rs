// Speechmatics enhanced en batch (multi-step async job).
//
// Reference VoiceInk : LLMkit SpeechmaticsClient.swift.
// Sequence :
//   1. POST /v2/jobs (multipart : config JSON + data_file)
//      config = { type: "transcription", transcription_config: { language, operating_point: "enhanced",
//                                                                additional_vocab? } }
//   2. GET /v2/jobs/{id} poll toutes les 1s jusqu'a job.status == "done"
//   3. GET /v2/jobs/{id}/transcript?format=txt -> texte brut UTF-8
//
// Tous les appels : Authorization: Bearer <apiKey>.
// Mapping langue : nil/empty/"auto" -> "auto", "zh" -> "cmn", sinon pass-through.

use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use reqwest::multipart::Form;
use serde::Deserialize;
use serde_json::json;

use super::http::{batch_client, map_http_err, wav_part_from_path};
use super::provider::{CloudTranscriptionProvider, TranscribeRequest};

pub struct SpeechmaticsProvider;

const MAX_WAIT_SECS: u64 = 300;

#[derive(Debug, Deserialize)]
struct JobCreated {
    id: String,
}

#[derive(Debug, Deserialize)]
struct JobStatus {
    job: JobStatusInner,
}
#[derive(Debug, Deserialize)]
struct JobStatusInner {
    status: String,
}

fn map_language(language: Option<&str>) -> &str {
    match language {
        None => "auto",
        Some(l) if l.is_empty() || l == "auto" => "auto",
        Some("zh") => "cmn",
        Some(l) => l,
    }
}

fn map_language_owned(language: Option<&str>) -> String {
    map_language(language).to_string()
}

#[async_trait]
impl CloudTranscriptionProvider for SpeechmaticsProvider {
    fn id(&self) -> &'static str {
        "speechmatics"
    }

    async fn verify_api_key(&self, api_key: &str) -> Result<()> {
        let client = batch_client("https://asr.api.speechmatics.com/v2/jobs")?;
        let resp = client
            .get("https://asr.api.speechmatics.com/v2/jobs")
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
        let client = batch_client("https://asr.api.speechmatics.com/v2/jobs")?;

        let lang = map_language_owned(request.language.as_deref());
        let mut transcription_config = serde_json::Map::new();
        transcription_config.insert("language".into(), json!(lang));
        transcription_config.insert("operating_point".into(), json!("enhanced"));
        if !request.custom_vocabulary.is_empty() {
            let vocab: Vec<_> = request
                .custom_vocabulary
                .iter()
                .map(|t| json!({ "content": t }))
                .collect();
            transcription_config.insert("additional_vocab".into(), json!(vocab));
        }

        let config_json = json!({
            "type": "transcription",
            "transcription_config": transcription_config,
        })
        .to_string();

        let form = Form::new()
            .text("config", config_json)
            .part("data_file", wav_part_from_path(wav_path).await?);

        // 1. Soumettre le job
        let job_id = client
            .post("https://asr.api.speechmatics.com/v2/jobs")
            .bearer_auth(api_key)
            .multipart(form)
            .send()
            .await
            .context("POST /v2/jobs")?
            .error_for_status()?
            .json::<JobCreated>()
            .await
            .context("parse /v2/jobs response")?
            .id;

        // 2. Poll du statut
        let start = std::time::Instant::now();
        loop {
            if start.elapsed().as_secs() > MAX_WAIT_SECS {
                anyhow::bail!("timeout job Speechmatics");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            let st: JobStatus = client
                .get(format!("https://asr.api.speechmatics.com/v2/jobs/{job_id}"))
                .bearer_auth(api_key)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            match st.job.status.as_str() {
                "done" => break,
                "rejected" => anyhow::bail!("job Speechmatics rejete"),
                "deleted" => anyhow::bail!("job Speechmatics supprime"),
                _ => continue,
            }
        }

        // 3. Recuperation du transcript (texte brut)
        let resp = client
            .get(format!(
                "https://asr.api.speechmatics.com/v2/jobs/{job_id}/transcript?format=txt"
            ))
            .bearer_auth(api_key)
            .send()
            .await?
            .error_for_status()?;
        let text = resp
            .text()
            .await
            .map_err(|e| anyhow!("lecture transcript: {e}"))?;
        Ok(text.trim().to_string())
    }
}
