// Cartesia Ink Whisper : provider streaming-only.
//
// Reference VoiceInk : LLMkit CartesiaStreamingClient.swift + CartesiaProvider.
// Pas de batch endpoint. La verification de cle se fait via /voices?limit=1
// (le seul endpoint REST authentifie disponible). transcribe() retourne une
// erreur explicite si le pipeline tente quand meme : la couche frontend
// filtre via `supports_batch=false` mais on garde le garde-fou.

use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;

use super::http::{batch_client, map_http_err};
use super::provider::{CloudTranscriptionProvider, TranscribeRequest};

pub struct CartesiaProvider;

const CARTESIA_VERSION: &str = "2026-03-01";

#[async_trait]
impl CloudTranscriptionProvider for CartesiaProvider {
    fn id(&self) -> &'static str {
        "cartesia"
    }

    async fn verify_api_key(&self, api_key: &str) -> Result<()> {
        let client = batch_client()?;
        let resp = client
            .get("https://api.cartesia.ai/voices?limit=1")
            .header("X-API-Key", api_key)
            .header("Cartesia-Version", CARTESIA_VERSION)
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
        _wav_path: &Path,
        _api_key: &str,
        _request: &TranscribeRequest,
    ) -> Result<String> {
        anyhow::bail!("Cartesia Ink Whisper est streaming uniquement (pas de batch)")
    }
}
