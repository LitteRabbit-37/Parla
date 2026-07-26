// xAI Grok speech-to-text en batch.
//
// Reference VoiceInk : LLMkit XAIClient.swift.
// Endpoint : POST https://api.x.ai/v1/stt
// Auth     : Authorization: Bearer <apiKey>
// Body     : multipart/form-data, le champ `file` doit etre LE DERNIER
//            (contrainte xAI). `language` optionnel, "auto" = on n'envoie
//            rien. `format=true` active l'inverse text normalization (ITN)
//            uniquement quand une langue est explicite.
// Reponse  : JSON `{"text": "..."}`.
// Verify   : GET /v1/api-key avec Bearer ; 200 = OK.

use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;

use super::http::{
    http_status_error, read_wav_with_filename, BatchHttpClient, HttpRequest, MultipartEncoder,
};
use super::provider::{CloudTranscriptionProvider, TranscribeRequest};

pub struct XaiProvider;

#[derive(Debug, Deserialize)]
struct XaiResponse {
    text: Option<String>,
}

#[async_trait]
impl CloudTranscriptionProvider for XaiProvider {
    fn id(&self) -> &'static str {
        "xai"
    }

    async fn verify_api_key(&self, api_key: &str) -> Result<()> {
        let client = BatchHttpClient::new("https://api.x.ai/v1/api-key")?;
        let request = HttpRequest::new("GET", "https://api.x.ai/v1/api-key")
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
        // Ordre des champs important pour xAI : `language` et `format` AVANT
        // `file`, ce dernier doit etre en dernier (cf. LLMkit XAIClient L37).
        let mut multipart = MultipartEncoder::random();

        let lang_provided = match request.language.as_deref() {
            Some(lang) if !lang.is_empty() && lang != "auto" => {
                multipart = multipart.field("language", lang.as_bytes().to_vec());
                true
            }
            _ => false,
        };
        if lang_provided {
            // ITN n'a de sens que quand on connait la langue.
            multipart = multipart.field("format", b"true".to_vec());
        }

        let (wav, filename) = read_wav_with_filename(wav_path).await?;
        multipart = multipart.file("file", filename, "audio/wav", wav);

        let client = BatchHttpClient::new("https://api.x.ai/v1/stt")?;
        let request = HttpRequest::new("POST", "https://api.x.ai/v1/stt")
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Accept", "application/json")
            .header("Content-Type", multipart.content_type())
            .body(multipart.try_encode()?);
        let response = client.send(request.clone()).await?;
        if !(200..300).contains(&response.status) {
            return Err(http_status_error(response.status, &response.body, &request));
        }

        match serde_json::from_slice::<XaiResponse>(&response.body) {
            Ok(r) => Ok(r.text.unwrap_or_default()),
            Err(_) => Ok(String::from_utf8_lossy(&response.body).trim().to_string()),
        }
    }
}
