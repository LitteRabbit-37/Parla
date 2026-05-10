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
                    language_codes: WHISPER_FULL_LANGS
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
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
            flags.insert(id.to_string(), Arc::new(std::sync::atomic::AtomicBool::new(false)));
        }
        let result = self.download_impl(id).await;
        self.cancel_flags.lock().remove(id);
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

        let client = reqwest::Client::new();
        let resp = client
            .get(model.url)
            .send()
            .await
            .with_context(|| format!("GET {}", model.url))?;
        if !resp.status().is_success() {
            anyhow::bail!("HTTP {} depuis {}", resp.status(), model.url);
        }
        let total = resp.content_length().unwrap_or(model.size_bytes);

        let mut file = tokio::fs::File::create(&tmp)
            .await
            .with_context(|| format!("create {}", tmp.display()))?;
        let mut stream = resp.bytes_stream();
        let mut downloaded: u64 = 0;
        let mut last_emit = std::time::Instant::now();

        while let Some(chunk) = stream.next().await {
            if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                // Cleanup du fichier partiel. Cancel flag is removed by
                // the outer download() wrapper on return.
                drop(file);
                let _ = fs::remove_file(&tmp);
                return Err(anyhow!("telechargement annule"));
            }
            let bytes = chunk.context("chunk recv")?;
            file.write_all(&bytes).await?;
            downloaded += bytes.len() as u64;

            // Throttle les emits a ~20 Hz pour eviter de saturer le bridge IPC.
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
        }
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
            fs::remove_file(&path)
                .with_context(|| format!("remove {}", path.display()))?;
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
