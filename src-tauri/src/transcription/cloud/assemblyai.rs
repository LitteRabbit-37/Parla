// AssemblyAI Universal speech-to-text en batch.
//
// Reference VoiceInk : LLMkit AssemblyAIClient.swift.
// Protocole en 3 etapes :
//   1. Upload     : POST /v2/upload (octet-stream raw)             -> upload_url
//   2. Job        : POST /v2/transcript (JSON)                     -> transcript_id
//   3. Poll       : GET /v2/transcript/{id} chaque seconde         -> text quand status=completed
// Auth : header `Authorization: <apiKey>` (PAS de Bearer cote AssemblyAI).
//
// Le mapping `speech_models` est repris de LLMkit (revision 95b29c2 utilisee
// par VoiceInk 2.13) : universal-3-5-pro et universal-2, sans fallback. Le
// custom_vocabulary devient `keyterms_prompt`, normalise (max 50 chars /
// 6 mots, dedup case-insensitive, 1000 entrees pour Universal-3.5 Pro et
// 200 pour Universal-2). LLMkit n'envoie plus de `prompt`. On retourne le
// texte final ou bail en cas d'erreur.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::http::{batch_client, map_http_err};
use super::provider::{CloudTranscriptionProvider, TranscribeRequest};

pub struct AssemblyAiProvider;

const API_BASE: &str = "https://api.assemblyai.com";
const POLL_INTERVAL: Duration = Duration::from_secs(1);
const POLL_MAX_WAIT: Duration = Duration::from_secs(300);
/// LLMkit AssemblyAIClient.normalizedKeyterms : 1000 pour Universal-3.5 Pro,
/// 200 pour Universal-2.
const KEYTERMS_LIMIT_U35: usize = 1_000;
const KEYTERMS_LIMIT_U2: usize = 200;

#[derive(Debug, Deserialize)]
struct UploadResponse {
    upload_url: String,
}

#[derive(Debug, Deserialize)]
struct CreateResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct StatusResponse {
    status: String,
    text: Option<String>,
    error: Option<String>,
}

#[async_trait]
impl CloudTranscriptionProvider for AssemblyAiProvider {
    fn id(&self) -> &'static str {
        "assemblyai"
    }

    async fn verify_api_key(&self, api_key: &str) -> Result<()> {
        let client = batch_client()?;
        let resp = client
            .get(format!("{API_BASE}/v2/transcript"))
            .header("Authorization", api_key)
            .send()
            .await
            .map_err(map_http_err)?;
        if !resp.status().is_success() {
            anyhow::bail!("HTTP {} (cle API invalide ?)", resp.status());
        }
        Ok(())
    }

    async fn transcribe(
        &self,
        wav_path: &Path,
        api_key: &str,
        request: &TranscribeRequest,
    ) -> Result<String> {
        let client = batch_client()?;
        let bytes = tokio::fs::read(wav_path)
            .await
            .with_context(|| format!("lecture {}", wav_path.display()))?;

        // 1. Upload
        let upload_url = upload(&client, api_key, bytes).await?;

        // 2. Cree le job
        let speech_models = speech_models_for(&request.model);
        let primary = speech_models
            .first()
            .cloned()
            .unwrap_or_else(|| request.model.clone());
        let keyterms = normalize_keyterms(&request.custom_vocabulary, keyterms_limit(&primary));

        let mut payload = serde_json::Map::new();
        payload.insert("audio_url".into(), json!(upload_url));
        payload.insert(
            "speech_models".into(),
            json!(speech_models),
        );
        payload.insert("punctuate".into(), json!(true));
        payload.insert("format_text".into(), json!(true));

        match request.language.as_deref() {
            Some(lang) if !lang.is_empty() && lang != "auto" => {
                payload.insert("language_code".into(), json!(lang));
            }
            _ => {
                payload.insert("language_detection".into(), json!(true));
            }
        }

        if !keyterms.is_empty() {
            payload.insert("keyterms_prompt".into(), json!(keyterms));
        }

        let resp = client
            .post(format!("{API_BASE}/v2/transcript"))
            .header("Authorization", api_key)
            .json(&payload)
            .send()
            .await
            .map_err(map_http_err)?;
        let status = resp.status();
        let body = resp.bytes().await?;
        if !status.is_success() {
            anyhow::bail!("HTTP {status}: {}", String::from_utf8_lossy(&body));
        }
        let created: CreateResponse =
            serde_json::from_slice(&body).context("parse create response")?;

        // 3. Poll
        poll_transcript(&client, api_key, &created.id).await
    }
}

async fn upload(client: &reqwest::Client, api_key: &str, bytes: Vec<u8>) -> Result<String> {
    let resp = client
        .post(format!("{API_BASE}/v2/upload"))
        .header("Authorization", api_key)
        .header("Content-Type", "application/octet-stream")
        .body(bytes)
        .send()
        .await
        .map_err(map_http_err)?;
    let status = resp.status();
    let body = resp.bytes().await?;
    if !status.is_success() {
        anyhow::bail!(
            "HTTP {status} sur /v2/upload: {}",
            String::from_utf8_lossy(&body)
        );
    }
    let parsed: UploadResponse = serde_json::from_slice(&body).context("parse upload response")?;
    Ok(parsed.upload_url)
}

async fn poll_transcript(
    client: &reqwest::Client,
    api_key: &str,
    id: &str,
) -> Result<String> {
    let url = format!("{API_BASE}/v2/transcript/{id}");
    let start = std::time::Instant::now();
    loop {
        let resp = client
            .get(&url)
            .header("Authorization", api_key)
            .send()
            .await
            .map_err(map_http_err)?;
        let status = resp.status();
        let body = resp.bytes().await?;
        if !status.is_success() {
            anyhow::bail!("HTTP {status}: {}", String::from_utf8_lossy(&body));
        }
        let parsed: StatusResponse =
            serde_json::from_slice(&body).context("parse status response")?;
        match parsed.status.to_lowercase().as_str() {
            "completed" => {
                let text = parsed.text.unwrap_or_default();
                if text.trim().is_empty() {
                    anyhow::bail!("AssemblyAI a renvoye un texte vide");
                }
                return Ok(text);
            }
            "error" => {
                anyhow::bail!(
                    "AssemblyAI: {}",
                    parsed.error.unwrap_or_else(|| "transcription failed".into())
                );
            }
            _ => {}
        }
        if start.elapsed() > POLL_MAX_WAIT {
            anyhow::bail!("timeout: AssemblyAI > {}s", POLL_MAX_WAIT.as_secs());
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Resolution LLMkit AssemblyAIClient.swift `speechModels(for:)` (95b29c2).
/// Les identifiants des versions precedentes de Parla (universal-3-pro,
/// universal-streaming*) encore presents dans les reglages sont mappes sur
/// leur successeur pour ne pas casser une selection existante.
fn speech_models_for(model: &str) -> Vec<String> {
    match model {
        "universal-3-5-pro" | "universal-3-pro" | "u3-rt-pro" => vec!["universal-3-5-pro".into()],
        "universal-2"
        | "universal-streaming"
        | "universal-streaming-english"
        | "universal-streaming-multilingual"
        | "whisper-rt" => vec!["universal-2".into()],
        other => vec![other.to_string()],
    }
}

fn keyterms_limit(primary_model: &str) -> usize {
    if primary_model == "universal-2" {
        KEYTERMS_LIMIT_U2
    } else {
        KEYTERMS_LIMIT_U35
    }
}

fn normalize_keyterms(raw: &[String], limit: usize) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for term in raw {
        let trimmed = term.trim();
        if trimmed.is_empty() || trimmed.chars().count() > 50 {
            continue;
        }
        let word_count = trimmed.split_whitespace().count();
        if word_count > 6 {
            continue;
        }
        let key = trimmed.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        out.push(trimmed.to_string());
        if out.len() == limit {
            break;
        }
    }
    out
}

// Empeche les warnings dead_code si certaines branches ne sont jamais
// utilisees actuellement (ex: prompt n'est pas encore exposé en UI).
#[allow(dead_code)]
fn _force_use_serialize_derive(_: &impl Serialize) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyterms_dedup_and_limit() {
        let raw: Vec<String> = (0..1_200).map(|i| format!("term {i}")).collect();
        assert_eq!(
            normalize_keyterms(&raw, keyterms_limit("universal-3-5-pro")).len(),
            KEYTERMS_LIMIT_U35
        );
        assert_eq!(
            normalize_keyterms(&raw, keyterms_limit("universal-2")).len(),
            KEYTERMS_LIMIT_U2
        );
    }

    #[test]
    fn keyterms_skip_long_or_too_many_words() {
        let raw = vec![
            "ok term".to_string(),
            // 7 mots : VoiceInk autorise <= 6, on doit dropper celui-ci.
            "this phrase has way too many words to keep".to_string(),
            "x".repeat(51),
            "  trim me  ".to_string(),
        ];
        let n = normalize_keyterms(&raw, KEYTERMS_LIMIT_U35);
        assert!(n.contains(&"ok term".to_string()));
        assert!(n.contains(&"trim me".to_string()));
        assert!(!n
            .iter()
            .any(|t| t == "this phrase has way too many words to keep"));
        assert!(!n.iter().any(|t| t.len() == 51));
    }

    #[test]
    fn keyterms_dedup_case_insensitive() {
        let raw = vec!["Docker".into(), "docker".into(), "DOCKER".into()];
        assert_eq!(
            normalize_keyterms(&raw, KEYTERMS_LIMIT_U35),
            vec!["Docker".to_string()]
        );
    }

    #[test]
    fn speech_models_universal35_and_legacy_ids() {
        assert_eq!(speech_models_for("universal-3-5-pro"), vec!["universal-3-5-pro"]);
        // Anciens identifiants Parla < 0.6.1 encore en store.
        assert_eq!(speech_models_for("universal-3-pro"), vec!["universal-3-5-pro"]);
        assert_eq!(speech_models_for("universal-streaming"), vec!["universal-2"]);
        assert_eq!(speech_models_for("universal-2"), vec!["universal-2"]);
    }
}
