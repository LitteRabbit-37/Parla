// Helpers HTTP partages par les clients batch.
//
// batch_client() : client reqwest avec timeouts explicites pour eviter les
// hangs indefinis en cas de panne reseau ou d'API bloquee.
// map_http_err()  : mapping anyhow qui discrimine timeout / connect error
// (pattern repris de enhancement/providers/openai_compat.rs).

use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use reqwest::multipart::Part;
use url::Url;

/// Timeout total pour une requete batch (upload + traitement cote provider).
/// 120s est large : Whisper large sur 30min d'audio prend ~30-60s cote cloud.
pub const BATCH_TIMEOUT: Duration = Duration::from_secs(120);

/// Timeout pour l'etablissement de la connexion TCP/TLS.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Client reqwest pre-configure pour les providers batch cloud.
/// A utiliser partout a la place de `reqwest::Client::new()`.
pub fn batch_client(endpoint: &str) -> Result<reqwest::Client> {
    let url = Url::parse(endpoint).map_err(|e| anyhow!("http endpoint: {e}"))?;
    crate::services::proxy::apply_for_url(reqwest::Client::builder(), &url)?
        .timeout(BATCH_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|e| anyhow!("http client: {e}"))
}

/// Mappe une erreur reqwest en anyhow avec un prefixe discriminant.
/// Le pipeline peut ensuite detecter "timeout" / "network_error" via le
/// texte de l'erreur pour decider retry/backoff.
pub fn map_http_err(e: reqwest::Error) -> anyhow::Error {
    if e.is_timeout() {
        return anyhow!("timeout: {e}");
    }
    if e.is_connect() {
        return anyhow!("network_error: {e}");
    }
    anyhow!("http: {e}")
}

/// Lit le WAV du disque et extrait le nom de fichier avec fallback.
pub async fn read_wav_with_filename(wav_path: &Path) -> Result<(Vec<u8>, String)> {
    let bytes = tokio::fs::read(wav_path)
        .await
        .with_context(|| format!("lecture {}", wav_path.display()))?;
    let name = wav_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.wav")
        .to_string();
    Ok((bytes, name))
}

/// Construit un Part multipart pour un WAV (mime audio/wav).
pub fn wav_part(bytes: Vec<u8>, filename: String) -> Result<Part> {
    Part::bytes(bytes)
        .file_name(filename)
        .mime_str("audio/wav")
        .map_err(Into::into)
}

/// Lit le WAV et retourne directement un Part pret pour multipart.
pub async fn wav_part_from_path(wav_path: &Path) -> Result<Part> {
    let (bytes, name) = read_wav_with_filename(wav_path).await?;
    wav_part(bytes, name)
}
