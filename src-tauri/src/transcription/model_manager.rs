// Gestion du catalogue local des modeles Whisper : listing, telechargement,
// suppression. Emet des evenements Tauri pour la progression UI.
//
// Reference VoiceInk : VoiceInk/Transcription/Core/Whisper/WhisperModelManager.swift
// - VoiceInk stocke les modeles dans `modelsDirectory` (UserDefaults) avec un
//   defaut dans Application Support. Ici on utilise AppLocalData/Models/.
// - Les modeles sont telecharges depuis HuggingFace ggerganov/whisper.cpp.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

use super::model::{find_model, WhisperModelInfo, WHISPER_FULL_LANGS, WHISPER_MODELS};

#[derive(Debug, Clone, Serialize)]
pub struct ModelState {
    pub id: String,
    pub display_name: String,
    pub size_bytes: u64,
    pub multilingual: bool,
    pub notes: String,
    pub downloaded: bool,
    /// Taille reelle sur disque si telecharge.
    pub on_disk_bytes: Option<u64>,
    pub path: Option<String>,
    /// true si c'est un modele importe par l'utilisateur (hors catalogue).
    pub imported: bool,
    /// Note de vitesse de 0 a 1 (alignee VoiceInk).
    pub speed: f32,
    /// Note de precision de 0 a 1 (idem).
    pub accuracy: f32,
    /// Codes ISO supportes par le modele (peut contenir "auto").
    pub language_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct DownloadProgress {
    id: String,
    downloaded: u64,
    total: u64,
}

#[derive(Debug, Clone, Serialize)]
struct DownloadComplete {
    id: String,
    path: String,
}

#[derive(Debug, Clone, Serialize)]
struct DownloadError {
    id: String,
    message: String,
}

pub struct ModelManager {
    app: AppHandle,
    cancel_flags: Mutex<std::collections::HashMap<String, Arc<std::sync::atomic::AtomicBool>>>,
}

impl ModelManager {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            cancel_flags: Mutex::new(Default::default()),
        }
    }

    /// Dossier ou sont stockes les .bin des modeles.
    pub fn models_dir(&self) -> Result<PathBuf> {
        let base = self
            .app
            .path()
            .app_local_data_dir()
            .map_err(|e| anyhow!("app_local_data_dir: {e}"))?;
        let dir = base.join("Models");
        fs::create_dir_all(&dir).ok();
        Ok(dir)
    }

    pub fn model_path(&self, model: &WhisperModelInfo) -> Result<PathBuf> {
        Ok(self.models_dir()?.join(format!("{}.bin", model.id)))
    }

    /// Repertoire des modeles importes par l'utilisateur (hors catalogue).
    pub fn imported_dir(&self) -> Result<PathBuf> {
        let dir = self.models_dir()?.join("imported");
        fs::create_dir_all(&dir).ok();
        Ok(dir)
    }

    /// Liste tous les modeles : catalogue + imports utilisateur.
    pub fn list(&self) -> Result<Vec<ModelState>> {
        let dir = self.models_dir()?;
        let mut out = Vec::with_capacity(WHISPER_MODELS.len());
        for m in WHISPER_MODELS {
            let p = dir.join(format!("{}.bin", m.id));
            let downloaded = p.exists();
            let on_disk_bytes = p.metadata().ok().map(|meta| meta.len());
            out.push(ModelState {
                id: m.id.to_string(),
                display_name: m.display_name.to_string(),
                size_bytes: m.size_bytes,
                multilingual: m.multilingual,
                notes: m.notes.to_string(),
                downloaded,
                on_disk_bytes,
                path: if downloaded {
                    Some(p.to_string_lossy().into_owned())
                } else {
                    None
                },
                imported: false,
                speed: m.speed,
                accuracy: m.accuracy,
                language_codes: m.language_codes.iter().map(|s| s.to_string()).collect(),
            });
        }
        // Ajoute les modeles importes.
        if let Ok(iter) = fs::read_dir(self.imported_dir()?) {
            for entry in iter.flatten() {
                let p = entry.path();
                if p.extension().and_then(|s| s.to_str()) != Some("bin") {
                    continue;
                }
                let name = p
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                if name.is_empty() {
                    continue;
                }
                let id = format!("imported:{name}");
                let size = p.metadata().ok().map(|m| m.len()).unwrap_or(0);
                out.push(ModelState {
                    id,
                    display_name: format!("{name} (importe)"),
                    size_bytes: size,
                    multilingual: true, // hypothese raisonnable pour un fichier fourni par l'utilisateur
                    notes: "Modele Whisper GGML importe par l'utilisateur".to_string(),
                    downloaded: true,
                    on_disk_bytes: Some(size),
                    path: Some(p.to_string_lossy().into_owned()),
                    imported: true,
                    // Pas d'info catalogue pour un import, on laisse a 0 pour cacher les ratings cote UI.
                    speed: 0.0,
                    accuracy: 0.0,
                    // Hypothese raisonnable : le fichier importe est multilingue.
                    language_codes: WHISPER_FULL_LANGS.iter().map(|s| s.to_string()).collect(),
                });
            }
        }
        Ok(out)
    }

    /// Importe un fichier .bin externe dans le repertoire imported/.
    /// Retourne l'id du modele (prefix `imported:`).
    pub fn import(&self, source_path: &Path) -> Result<String> {
        if source_path.extension().and_then(|s| s.to_str()) != Some("bin") {
            anyhow::bail!("le fichier doit avoir l'extension .bin");
        }
        if !source_path.exists() {
            anyhow::bail!("fichier introuvable: {}", source_path.display());
        }

        let stem = source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow!("nom de fichier invalide"))?;

        let imported_dir = self.imported_dir()?;
        let target = imported_dir.join(format!("{stem}.bin"));
        if target.exists() {
            anyhow::bail!("un modele portant ce nom existe deja: {}", target.display());
        }

        fs::copy(source_path, &target)
            .with_context(|| format!("copie vers {}", target.display()))?;

        info!(
            source = %source_path.display(),
            target = %target.display(),
            "Modele importe"
        );
        Ok(format!("imported:{stem}"))
    }

    /// Supprime un modele importe. Ne touche pas aux modeles du catalogue.
    pub fn delete_imported(&self, id: &str) -> Result<()> {
        let stem = id
            .strip_prefix("imported:")
            .ok_or_else(|| anyhow!("id invalide (doit commencer par imported:)"))?;
        let path = self.imported_dir()?.join(format!("{stem}.bin"));
        if path.exists() {
            fs::remove_file(&path)?;
            info!(id, "Modele importe supprime");
        }
        Ok(())
    }

    /// Telecharge un modele depuis HuggingFace avec emission d'evenements de
    /// progression `model:download:progress` (et complete / error).
    pub async fn download(&self, id: &str) -> Result<PathBuf> {
        // Reentrancy guard BEFORE any IO so a second call returns fast.
        // We insert the cancel flag up front and always remove it when
        // download_impl returns (success, cancel, or error).
        {
            let mut flags = self.cancel_flags.lock();
            if flags.contains_key(id) {
                return Err(anyhow!("telechargement deja en cours: {id}"));
            }
            flags.insert(
                id.to_string(),
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
            );
        }
        let result = self.download_impl(id).await;
        self.cancel_flags.lock().remove(id);
        if let Err(error) = &result {
            self.emit_error(id, crate::services::download::diagnostic(error));
        }
        result
    }

    async fn download_impl(&self, id: &str) -> Result<PathBuf> {
        let model = find_model(id).ok_or_else(|| anyhow!("modele inconnu: {id}"))?;
        let target = self.model_path(model)?;
        if target.exists() {
            info!(id, path = %target.display(), "Modele deja present");
            return Ok(target);
        }

        let cancel = self
            .cancel_flags
            .lock()
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("cancel flag missing for {id}"))?;

        let tmp = target.with_extension("bin.part");
        // Efface un eventuel tmp precedent.
        let _ = fs::remove_file(&tmp);

        let url = url::Url::parse(model.url)?;
        if matches!(
            crate::services::proxy::route_for_url(&url)?,
            crate::services::proxy::ProxyRoute::System
        ) {
            return self
                .download_system_worker(id, model.url, model.size_bytes, target, tmp, url, cancel)
                .await;
        }
        let host = url.host_str().unwrap_or("?");
        let apply_started = std::time::Instant::now();
        info!(id, host, "Model download proxy setup started");
        let (builder, route_diagnostic) = crate::services::proxy::apply_for_url_with_diagnostic(
            reqwest::Client::builder(),
            &url,
        )?;
        let client = builder.build()?;
        info!(
            id,
            host,
            elapsed_ms = apply_started.elapsed().as_millis() as u64,
            "Model download proxy setup completed"
        );

        let send_started = std::time::Instant::now();
        info!(id, host, "Model download request started");
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            client.get(model.url).send(),
        )
        .await;
        let resp = match response {
            Ok(Ok(response)) => {
                info!(
                    id,
                    host,
                    elapsed_ms = send_started.elapsed().as_millis() as u64,
                    success = true,
                    "Model download request completed"
                );
                response
            }
            Ok(Err(error)) => {
                let diagnostic = crate::services::download::diagnostic(&anyhow!(error.to_string()));
                warn!(
                    id,
                    host,
                    route_kind = route_diagnostic.kind,
                    route_scheme = route_diagnostic.scheme.as_deref().unwrap_or("?"),
                    route_host = route_diagnostic.host.as_deref().unwrap_or("?"),
                    route_port = route_diagnostic.port.unwrap_or_default(),
                    elapsed_ms = send_started.elapsed().as_millis() as u64,
                    is_connect = error.is_connect(),
                    is_timeout = error.is_timeout(),
                    is_request = error.is_request(),
                    is_body = error.is_body(),
                    is_status = error.is_status(),
                    error = %diagnostic,
                    "Model download request failed"
                );
                return Err(anyhow!(diagnostic));
            }
            Err(_) => {
                let diagnostic =
                    "model download request timed out while waiting for proxy or server response after 30 seconds";
                warn!(
                    id,
                    host,
                    route_kind = route_diagnostic.kind,
                    route_scheme = route_diagnostic.scheme.as_deref().unwrap_or("?"),
                    route_host = route_diagnostic.host.as_deref().unwrap_or("?"),
                    route_port = route_diagnostic.port.unwrap_or_default(),
                    elapsed_ms = send_started.elapsed().as_millis() as u64,
                    error = diagnostic,
                    "Model download request timed out"
                );
                return Err(anyhow!(diagnostic));
            }
        };
        if !resp.status().is_success() {
            let auth_schemes =
                if resp.status() == reqwest::StatusCode::PROXY_AUTHENTICATION_REQUIRED {
                    resp.headers()
                        .get_all(reqwest::header::PROXY_AUTHENTICATE)
                        .iter()
                        .filter_map(|value| value.to_str().ok())
                        .filter_map(|value| value.split_whitespace().next())
                        .collect::<Vec<_>>()
                        .join(",")
                } else {
                    String::new()
                };
            warn!(
                id,
                host,
                status = %resp.status(),
                proxy_auth_schemes = %auth_schemes,
                "Model download HTTP request returned non-success"
            );
            anyhow::bail!("HTTP {} depuis {}", resp.status(), model.url);
        }
        let total = resp.content_length().unwrap_or(model.size_bytes);

        let file = tokio::fs::File::create(&tmp)
            .await
            .with_context(|| format!("create {}", tmp.display()))?;
        let mut last_emit = std::time::Instant::now();
        let (mut file, downloaded) = write_stream_to_partial_file(
            resp.bytes_stream(),
            file,
            &tmp,
            &cancel,
            std::time::Duration::from_secs(30),
            |downloaded| {
                if last_emit.elapsed() >= std::time::Duration::from_millis(50) {
                    let _ = self.app.emit(
                        "model:download:progress",
                        DownloadProgress {
                            id: id.to_string(),
                            downloaded,
                            total,
                        },
                    );
                    last_emit = std::time::Instant::now();
                }
            },
        )
        .await?;
        file.flush().await?;
        drop(file);

        // Dernier emit a 100 %.
        let _ = self.app.emit(
            "model:download:progress",
            DownloadProgress {
                id: id.to_string(),
                downloaded,
                total,
            },
        );

        fs::rename(&tmp, &target)
            .with_context(|| format!("rename {} -> {}", tmp.display(), target.display()))?;

        info!(id, path = %target.display(), "Modele telecharge");

        let _ = self.app.emit(
            "model:download:complete",
            DownloadComplete {
                id: id.to_string(),
                path: target.to_string_lossy().into_owned(),
            },
        );

        Ok(target)
    }

    async fn download_system_worker(
        &self,
        id: &str,
        model_url: &str,
        model_size: u64,
        target: PathBuf,
        tmp: PathBuf,
        url: url::Url,
        cancel: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<PathBuf> {
        use std::sync::atomic::{AtomicBool, Ordering};

        static SYSTEM_WORKER_BUSY: AtomicBool = AtomicBool::new(false);
        if SYSTEM_WORKER_BUSY.swap(true, Ordering::Acquire) {
            anyhow::bail!("system proxy model download already blocked");
        }

        let abandoned = Arc::new(AtomicBool::new(false));
        let worker_abandoned = Arc::clone(&abandoned);
        let worker_cancel = Arc::clone(&cancel);
        let worker_app = self.app.clone();
        let worker_id = id.to_owned();
        let worker_url = model_url.to_owned();
        let host = url.host_str().unwrap_or("?").to_owned();
        let worker_host = host.clone();
        let worker_log_id = worker_id.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(16);

        std::thread::spawn(move || {
            info!(id = %worker_log_id, host = %worker_host, "System model download worker started");
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| anyhow!("system download runtime: {error}"))?;
                runtime.block_on(system_download_worker(
                    worker_app,
                    worker_id,
                    worker_url,
                    model_size,
                    target,
                    tmp,
                    url,
                    worker_cancel,
                    worker_abandoned,
                    sender.clone(),
                ))
            }))
            .unwrap_or_else(|_| Err(anyhow!("system download worker panicked")));
            let _ = sender.try_send(SystemDownloadMessage::Result(result));
            info!(id = %worker_log_id, host = %worker_host, "System model download worker exited");
            SYSTEM_WORKER_BUSY.store(false, Ordering::Release);
        });

        loop {
            match receiver.recv_timeout(std::time::Duration::from_secs(30)) {
                Ok(SystemDownloadMessage::Activity) => continue,
                Ok(SystemDownloadMessage::Result(result)) => return result,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    abandoned.store(true, Ordering::Release);
                    let diagnostic =
                        "model download request timed out while waiting for proxy or server response after 30 seconds";
                    warn!(id, host = %host, error = diagnostic, "Model download request timed out");
                    return Err(anyhow!(diagnostic));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(anyhow!("system download worker exited without a result"));
                }
            }
        }
    }

    /// Annule un telechargement en cours. Idempotent.
    pub fn cancel_download(&self, id: &str) {
        if let Some(flag) = self.cancel_flags.lock().get(id) {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            warn!(id, "Telechargement annule");
        }
    }

    /// Supprime un modele telecharge. Ne touche pas au catalogue.
    pub fn delete(&self, id: &str) -> Result<()> {
        let model = find_model(id).ok_or_else(|| anyhow!("modele inconnu: {id}"))?;
        let path = self.model_path(model)?;
        if path.exists() {
            fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
            info!(id, "Modele supprime");
        }
        Ok(())
    }

    /// Emet un event d'erreur pour un telechargement.
    pub fn emit_error(&self, id: &str, message: impl Into<String>) {
        let _ = self.app.emit(
            "model:download:error",
            DownloadError {
                id: id.to_string(),
                message: message.into(),
            },
        );
    }

    /// Raccourci pour construire un PathBuf si deja telecharge (catalogue
    /// predefini ou modele importe).
    pub fn path_if_present(&self, id: &str) -> Option<PathBuf> {
        if let Some(stem) = id.strip_prefix("imported:") {
            let p = self.imported_dir().ok()?.join(format!("{stem}.bin"));
            return p.exists().then_some(p);
        }
        let model = find_model(id)?;
        let path = self.model_path(model).ok()?;
        path.exists().then_some(path)
    }
}

enum SystemDownloadMessage {
    Activity,
    Result(Result<PathBuf>),
}

async fn write_stream_to_partial_file<S, B, E, F>(
    mut stream: S,
    mut file: tokio::fs::File,
    partial_path: &Path,
    cancel: &std::sync::atomic::AtomicBool,
    chunk_timeout: std::time::Duration,
    mut on_progress: F,
) -> Result<(tokio::fs::File, u64)>
where
    S: futures_util::Stream<Item = std::result::Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: std::error::Error + Send + Sync + 'static,
    F: FnMut(u64),
{
    let mut downloaded = 0;
    loop {
        let next_chunk = match tokio::time::timeout(chunk_timeout, stream.next()).await {
            Ok(next_chunk) => next_chunk,
            Err(_) => {
                drop(file);
                let _ = fs::remove_file(partial_path);
                return Err(anyhow!(
                    "model download body stalled while waiting for next chunk"
                ));
            }
        };
        let Some(chunk) = next_chunk else { break };
        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            drop(file);
            let _ = fs::remove_file(partial_path);
            return Err(anyhow!("telechargement annule"));
        }
        let bytes = chunk.context("chunk recv")?;
        file.write_all(bytes.as_ref()).await?;
        downloaded += bytes.as_ref().len() as u64;
        on_progress(downloaded);
    }
    Ok((file, downloaded))
}

async fn system_download_worker(
    app: AppHandle,
    id: String,
    _model_url: String,
    model_size: u64,
    target: PathBuf,
    tmp: PathBuf,
    url: url::Url,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    abandoned: Arc<std::sync::atomic::AtomicBool>,
    sender: std::sync::mpsc::SyncSender<SystemDownloadMessage>,
) -> Result<PathBuf> {
    let app_for_worker = app.clone();
    let id_for_worker = id.clone();
    let tmp_for_worker = tmp.clone();
    let cancel_for_worker = cancel.clone();
    let abandoned_for_worker = abandoned.clone();
    let downloaded = crate::services::winhttp_download::download(
        &url,
        model_size,
        move |chunk, downloaded, total| {
            if cancel_for_worker.load(std::sync::atomic::Ordering::SeqCst)
                || abandoned_for_worker.load(std::sync::atomic::Ordering::Acquire)
            {
                return false;
            }
            let mut file = match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&tmp_for_worker)
            {
                Ok(file) => file,
                Err(_) => return false,
            };
            use std::io::Write;
            if file.write_all(chunk).is_err() {
                return false;
            }
            let _ = sender.try_send(SystemDownloadMessage::Activity);
            let _ = app_for_worker.emit(
                "model:download:progress",
                DownloadProgress {
                    id: id_for_worker.clone(),
                    downloaded,
                    total,
                },
            );
            true
        },
    )?;
    if abandoned.load(std::sync::atomic::Ordering::Acquire) {
        let _ = fs::remove_file(&tmp);
        return Err(anyhow!("system download abandoned"));
    }
    let _ = app.emit(
        "model:download:progress",
        DownloadProgress {
            id: id.clone(),
            downloaded,
            total: model_size,
        },
    );
    fs::rename(&tmp, &target)
        .with_context(|| format!("rename {} -> {}", tmp.display(), target.display()))?;
    let _ = app.emit(
        "model:download:complete",
        DownloadComplete {
            id,
            path: target.to_string_lossy().into_owned(),
        },
    );
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::write_stream_to_partial_file;
    use futures_util::stream;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn cancellation_after_first_chunk_removes_partial_file() {
        let path = std::env::temp_dir().join(format!(
            "parla-model-manager-test-{}.bin.part",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let file = tokio::fs::File::create(&path).await.unwrap();
        let cancel = AtomicBool::new(false);
        let result = write_stream_to_partial_file(
            stream::iter([
                Ok::<_, std::io::Error>(b"first".to_vec()),
                Ok::<_, std::io::Error>(b"second".to_vec()),
            ]),
            file,
            &path,
            &cancel,
            std::time::Duration::from_secs(30),
            |downloaded| {
                if downloaded == 5 {
                    cancel.store(true, Ordering::SeqCst);
                }
            },
        )
        .await;

        assert!(result.is_err());
        assert!(!path.exists());
        assert!(!path.with_extension("bin").exists());
    }

    #[tokio::test]
    async fn stalled_body_returns_within_chunk_timeout_and_removes_partial_file() {
        use std::task::Poll;

        let path = std::env::temp_dir().join(format!(
            "parla-model-manager-stalled-test-{}.bin.part",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let file = tokio::fs::File::create(&path).await.unwrap();
        let cancel = AtomicBool::new(false);
        let result = write_stream_to_partial_file(
            stream::poll_fn(
                |_: &mut std::task::Context<'_>| -> Poll<Option<std::result::Result<Vec<u8>, std::io::Error>>> {
                    Poll::Pending
                },
            ),
            file,
            &path,
            &cancel,
            std::time::Duration::from_millis(10),
            |_| {},
        )
        .await;

        let error = result.unwrap_err().to_string();
        assert!(error.contains("body stalled"), "unexpected error: {error}");
        assert!(!path.exists());
    }
}
