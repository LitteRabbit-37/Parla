// Catalogue et gestion des modeles GGUF pour llama.cpp embarque.
//
// Les modeles sont telecharges depuis HuggingFace et stockes dans
// AppLocalData/LlmModels/. Le format est .gguf (llama.cpp).
//
// Reference VoiceInk : VoiceInk ne bundle pas de LLM local sous macOS ;
// cette brique est specifique a Parla (Windows) et utilise le meme pattern
// que crate::transcription::model_manager.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tracing::info;

/// Entree du catalogue GGUF.
#[derive(Debug, Clone, Copy)]
pub struct GgufModelInfo {
    pub id: &'static str,
    pub display_name: &'static str,
    pub url: &'static str,
    pub size_bytes: u64,
    pub context_length: u32,
    pub notes: &'static str,
}

/// Catalogue des modeles recommandes pour l'enhancement LLM local.
/// Selection : petits modeles Instruct performants en Q4_K_M.
pub const GGUF_MODELS: &[GgufModelInfo] = &[
    GgufModelInfo {
        id: "qwen2.5-3b-instruct-q4",
        display_name: "Qwen 2.5 3B Instruct (Q4_K_M)",
        url: "https://huggingface.co/bartowski/Qwen2.5-3B-Instruct-GGUF/resolve/main/Qwen2.5-3B-Instruct-Q4_K_M.gguf",
        size_bytes: 2_018_000_000,
        context_length: 32768,
        notes: "Polyvalent, excellent rapport qualite / taille.",
    },
    GgufModelInfo {
        id: "llama-3.2-3b-instruct-q4",
        display_name: "Llama 3.2 3B Instruct (Q4_K_M)",
        url: "https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF/resolve/main/Llama-3.2-3B-Instruct-Q4_K_M.gguf",
        size_bytes: 2_020_000_000,
        context_length: 131072,
        notes: "Longue fenetre de contexte.",
    },
    GgufModelInfo {
        id: "gemma-2-2b-it-q4",
        display_name: "Gemma 2 2B Instruct (Q4_K_M)",
        url: "https://huggingface.co/bartowski/gemma-2-2b-it-GGUF/resolve/main/gemma-2-2b-it-Q4_K_M.gguf",
        size_bytes: 1_710_000_000,
        context_length: 8192,
        notes: "Modele leger, rapide en CPU.",
    },
    GgufModelInfo {
        id: "phi-3.5-mini-instruct-q4",
        display_name: "Phi 3.5 Mini Instruct (Q4_K_M)",
        url: "https://huggingface.co/bartowski/Phi-3.5-mini-instruct-GGUF/resolve/main/Phi-3.5-mini-instruct-Q4_K_M.gguf",
        size_bytes: 2_390_000_000,
        context_length: 131072,
        notes: "3.8B, fort en raisonnement et instructions.",
    },
];

pub fn find_gguf(id: &str) -> Option<&'static GgufModelInfo> {
    GGUF_MODELS.iter().find(|m| m.id == id)
}

#[derive(Debug, Clone, Serialize)]
pub struct GgufModelState {
    pub id: String,
    pub display_name: String,
    pub size_bytes: u64,
    pub context_length: u32,
    pub notes: String,
    pub downloaded: bool,
    pub on_disk_bytes: Option<u64>,
    pub path: Option<String>,
    pub imported: bool,
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

pub struct GgufModelManager {
    app: AppHandle,
    cancel_flags: Mutex<std::collections::HashMap<String, Arc<std::sync::atomic::AtomicBool>>>,
}

impl GgufModelManager {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            cancel_flags: Mutex::new(Default::default()),
        }
    }

    pub fn models_dir(&self) -> Result<PathBuf> {
        let base = self
            .app
            .path()
            .app_local_data_dir()
            .map_err(|e| anyhow!("app_local_data_dir: {e}"))?;
        let dir = base.join("LlmModels");
        fs::create_dir_all(&dir).ok();
        Ok(dir)
    }

    pub fn imported_dir(&self) -> Result<PathBuf> {
        let dir = self.models_dir()?.join("imported");
        fs::create_dir_all(&dir).ok();
        Ok(dir)
    }

    pub fn model_path(&self, m: &GgufModelInfo) -> Result<PathBuf> {
        Ok(self.models_dir()?.join(format!("{}.gguf", m.id)))
    }

    pub fn path_for_id(&self, id: &str) -> Option<PathBuf> {
        if let Some(rest) = id.strip_prefix("imported:") {
            let p = self.imported_dir().ok()?.join(format!("{rest}.gguf"));
            return p.exists().then_some(p);
        }
        if let Some(m) = find_gguf(id) {
            let p = self.model_path(m).ok()?;
            return p.exists().then_some(p);
        }
        None
    }

    pub fn list(&self) -> Result<Vec<GgufModelState>> {
        let dir = self.models_dir()?;
        let mut out = Vec::with_capacity(GGUF_MODELS.len());
        for m in GGUF_MODELS {
            let p = dir.join(format!("{}.gguf", m.id));
            let downloaded = p.exists();
            let on_disk_bytes = p.metadata().ok().map(|meta| meta.len());
            out.push(GgufModelState {
                id: m.id.to_string(),
                display_name: m.display_name.to_string(),
                size_bytes: m.size_bytes,
                context_length: m.context_length,
                notes: m.notes.to_string(),
                downloaded,
                on_disk_bytes,
                path: downloaded.then(|| p.to_string_lossy().into_owned()),
                imported: false,
            });
        }
        if let Ok(iter) = fs::read_dir(self.imported_dir()?) {
            for entry in iter.flatten() {
                let p = entry.path();
                if p.extension().and_then(|s| s.to_str()) != Some("gguf") {
                    continue;
                }
                let stem = match p.file_stem().and_then(|s| s.to_str()) {
                    Some(s) if !s.is_empty() => s.to_string(),
                    _ => continue,
                };
                let size = p.metadata().ok().map(|m| m.len()).unwrap_or(0);
                out.push(GgufModelState {
                    id: format!("imported:{stem}"),
                    display_name: format!("{stem} (importe)"),
                    size_bytes: size,
                    context_length: 0,
                    notes: "Modele GGUF importe par l'utilisateur".into(),
                    downloaded: true,
                    on_disk_bytes: Some(size),
                    path: Some(p.to_string_lossy().into_owned()),
                    imported: true,
                });
            }
        }
        Ok(out)
    }

    pub fn import(&self, source_path: &Path) -> Result<String> {
        if source_path.extension().and_then(|s| s.to_str()) != Some("gguf") {
            anyhow::bail!("le fichier doit avoir l'extension .gguf");
        }
        if !source_path.exists() {
            anyhow::bail!("fichier introuvable: {}", source_path.display());
        }
        let stem = source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow!("nom de fichier invalide"))?;
        let target = self.imported_dir()?.join(format!("{stem}.gguf"));
        if target.exists() {
            anyhow::bail!("un modele existe deja: {}", target.display());
        }
        fs::copy(source_path, &target)
            .with_context(|| format!("copie vers {}", target.display()))?;
        info!(
            source = %source_path.display(),
            target = %target.display(),
            "GGUF importe"
        );
        Ok(format!("imported:{stem}"))
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        if let Some(rest) = id.strip_prefix("imported:") {
            let p = self.imported_dir()?.join(format!("{rest}.gguf"));
            if p.exists() {
                fs::remove_file(&p)?;
            }
            return Ok(());
        }
        if let Some(m) = find_gguf(id) {
            let p = self.model_path(m)?;
            if p.exists() {
                fs::remove_file(&p)?;
            }
            return Ok(());
        }
        Err(anyhow!("id inconnu: {id}"))
    }

    pub fn cancel_download(&self, id: &str) {
        if let Some(flag) = self.cancel_flags.lock().get(id) {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    pub async fn download(&self, id: &str) -> Result<PathBuf> {
        // Reentrancy guard. Prevents double-click bugs from racing two
        // downloads on the same .part file.
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
        if let Err(e) = &result {
            let _ = self.app.emit(
                "llm_model:download:error",
                DownloadError {
                    id: id.to_string(),
                    message: crate::services::download::diagnostic(e),
                },
            );
        }
        result
    }

    async fn download_impl(&self, id: &str) -> Result<PathBuf> {
        let m = find_gguf(id).ok_or_else(|| anyhow!("modele inconnu: {id}"))?;
        let target = self.model_path(m)?;
        if target.exists() {
            return Ok(target);
        }
        let cancel = self
            .cancel_flags
            .lock()
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("cancel flag missing for {id}"))?;

        let url = url::Url::parse(m.url)?;
        let app = self.app.clone();
        let id_owned = id.to_owned();
        crate::services::download::download_to_file(
            &url,
            &target,
            m.size_bytes,
            Some(cancel),
            move |downloaded, total| {
                let _ = app.emit(
                    "llm_model:download:progress",
                    DownloadProgress {
                        id: id_owned.clone(),
                        downloaded,
                        total,
                    },
                );
            },
        )
        .await?;

        let _ = self.app.emit(
            "llm_model:download:complete",
            DownloadComplete {
                id: id.to_string(),
                path: target.to_string_lossy().into_owned(),
            },
        );
        info!(id, path = %target.display(), "GGUF telecharge");
        Ok(target)
    }
}

pub struct GgufModelManagerState(pub Arc<GgufModelManager>);

impl GgufModelManagerState {
    pub fn new(app: AppHandle) -> Self {
        Self(Arc::new(GgufModelManager::new(app)))
    }
}
