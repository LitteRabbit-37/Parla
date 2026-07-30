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
use serde::Deserialize;
use serde_json::json;

use super::http::{
    http_status_error, read_wav_with_filename, BatchHttpClient, HttpRequest, MultipartEncoder,
};
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
        let client = BatchHttpClient::new("https://asr.api.speechmatics.com/v2/jobs")?;
        let request = HttpRequest::new("GET", "https://asr.api.speechmatics.com/v2/jobs")
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
        let client = BatchHttpClient::new("https://asr.api.speechmatics.com/v2/jobs")?;

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
        });

        let (wav, filename) = read_wav_with_filename(wav_path).await?;
        let multipart = MultipartEncoder::random()
            .field("config", serde_json::to_vec(&config_json)?)
            .file("data_file", filename, "audio/wav", wav);
        let submit_request = HttpRequest::new("POST", "https://asr.api.speechmatics.com/v2/jobs")
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", multipart.content_type())
            .body(multipart.try_encode()?);

        // 1. Soumettre le job
        let submit_response = client
            .send(submit_request.clone())
            .await
            .context("POST /v2/jobs")?;
        if !(200..300).contains(&submit_response.status) {
            return Err(http_status_error(
                submit_response.status,
                &submit_response.body,
                &submit_request,
            ));
        }
        let job_id = serde_json::from_slice::<JobCreated>(&submit_response.body)
            .context("parse /v2/jobs response")?
            .id;

        // 2. Poll du statut
        let start = std::time::Instant::now();
        loop {
            if start.elapsed().as_secs() > MAX_WAIT_SECS {
                anyhow::bail!("timeout job Speechmatics");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            let status_request = HttpRequest::new(
                "GET",
                format!("https://asr.api.speechmatics.com/v2/jobs/{job_id}"),
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
            let st: JobStatus = serde_json::from_slice(&status_response.body)?;
            match st.job.status.as_str() {
                "done" => break,
                "rejected" => anyhow::bail!("job Speechmatics rejete"),
                "deleted" => anyhow::bail!("job Speechmatics supprime"),
                _ => continue,
            }
        }

        // 3. Recuperation du transcript (texte brut)
        let transcript_request = HttpRequest::new(
            "GET",
            format!("https://asr.api.speechmatics.com/v2/jobs/{job_id}/transcript?format=txt"),
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
        let text = String::from_utf8(transcript_response.body)
            .map_err(|e| anyhow!("lecture transcript: {e}"))?;
        Ok(text.trim().to_string())
    }
}
