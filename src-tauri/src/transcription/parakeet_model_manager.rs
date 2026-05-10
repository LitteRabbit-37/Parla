// Gestion du catalogue et des telechargements des modeles Parakeet.
//
// Reference VoiceInk : la lib FluidAudio gere tout (download + decode +
// inference) sur macOS. Parla porte ca sur Windows via `parakeet-rs` qui
// consomme un repertoire de fichiers ONNX produits par NVIDIA NeMo puis
// reuploades sur HuggingFace. On telecharge ces fichiers explicitement.
//
// Fichiers par modele (repo istupakov/parakeet-tdt-0.6b-vN-onnx) :
//   config.json
//   nemo128.onnx
//   encoder-model{.int8}?.onnx
//   encoder-model{.int8}?.onnx.data  (uniquement F16 + en fichier separe)
//   decoder_joint-model{.int8}?.onnx
//   vocab.txt

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;
use tracing::info;

#[derive(Debug, Clone, Copy)]
pub struct ParakeetVariant {
    /// Identifiant stable (utilise en UI + store).
    pub id: &'static str,
    pub display_name: &'static str,
    pub repo: &'static str,
    pub is_quantized: bool,
    pub multilingual: bool,
    /// Taille totale approximative des fichiers a telecharger (affichage).
    pub size_bytes: u64,
    /// Liste des fichiers attendus dans le repertoire du modele.
    pub files: &'static [&'static str],
    pub notes: &'static str,
    /// Note de vitesse de 0 a 1 (alignee VoiceInk FluidAudioModel).
    pub speed: f32,
    /// Note de precision de 0 a 1 (idem). Variante int8 perd ~1 point sur l'accuracy.
    pub accuracy: f32,
    /// Codes ISO supportes : v2 = anglais seul, v3 = 25 langues europeennes
    /// + auto. Reference VoiceInk LanguageDictionary.forProvider(.fluidAudio).
    pub language_codes: &'static [&'static str],
}

/// 25 langues europeennes supportees par Parakeet TDT v3 + "auto" en tete.
/// Reference VoiceInk LanguageDictionary.forProvider(.fluidAudio).
pub const PARAKEET_V3_LANGS: &[&str] = &[
    "auto", "bg", "cs", "da", "de", "el", "en", "es", "et", "fi", "fr", "hr",
    "hu", "it", "lt", "lv", "mt", "nl", "pl", "pt", "ro", "ru", "sk", "sl",
    "sv", "uk",
];

pub const PARAKEET_VARIANTS: &[ParakeetVariant] = &[
    ParakeetVariant {
        id: "parakeet-tdt-0.6b-v2",
        display_name: "Parakeet TDT 0.6B v2 (anglais, F16)",
        repo: "istupakov/parakeet-tdt-0.6b-v2-onnx",
        is_quantized: false,
        multilingual: false,
        size_bytes: 2_500_000_000,
        files: &[
            "config.json",
            "vocab.txt",
            "nemo128.onnx",
            "encoder-model.onnx",
            "encoder-model.onnx.data",
            "decoder_joint-model.onnx",
        ],
        notes: "Modele de reference. Anglais uniquement. ~2.5 GB a telecharger.",
        speed: 0.99,
        accuracy: 0.94,
        language_codes: &["en"],
    },
    ParakeetVariant {
        id: "parakeet-tdt-0.6b-v2-int8",
        display_name: "Parakeet TDT 0.6B v2 (anglais, int8)",
        repo: "istupakov/parakeet-tdt-0.6b-v2-onnx",
        is_quantized: true,
        multilingual: false,
        size_bytes: 680_000_000,
        files: &[
            "config.json",
            "vocab.txt",
            "nemo128.onnx",
            "encoder-model.int8.onnx",
            "decoder_joint-model.int8.onnx",
        ],
        notes: "Variante quantizee int8. ~680 MB. Anglais uniquement.",
        speed: 0.99,
        accuracy: 0.93,
        language_codes: &["en"],
    },
    ParakeetVariant {
        id: "parakeet-tdt-0.6b-v3",
        display_name: "Parakeet TDT 0.6B v3 (multilingue, F16)",
        repo: "istupakov/parakeet-tdt-0.6b-v3-onnx",
        is_quantized: false,
        multilingual: true,
        size_bytes: 2_500_000_000,
        files: &[
            "config.json",
            "vocab.txt",
            "nemo128.onnx",
            "encoder-model.onnx",
            "encoder-model.onnx.data",
            "decoder_joint-model.onnx",
        ],
        notes: "Multilingue (EN + 25 langues europeennes). ~2.5 GB.",
        speed: 0.99,
        accuracy: 0.94,
        language_codes: PARAKEET_V3_LANGS,
    },
    ParakeetVariant {
        id: "parakeet-tdt-0.6b-v3-int8",
        display_name: "Parakeet TDT 0.6B v3 (multilingue, int8)",
        repo: "istupakov/parakeet-tdt-0.6b-v3-onnx",
        is_quantized: true,
        multilingual: true,
        size_bytes: 680_000_000,
        files: &[
            "config.json",
            "vocab.txt",
            "nemo128.onnx",
            "encoder-model.int8.onnx",
            "decoder_joint-model.int8.onnx",
        ],
        notes: "Multilingue quantizee int8. ~680 MB.",
        speed: 0.99,
        accuracy: 0.93,
        language_codes: PARAKEET_V3_LANGS,
    },
];

pub fn find_variant(id: &str) -> Option<&'static ParakeetVariant> {
    PARAKEET_VARIANTS.iter().find(|v| v.id == id)
}

#[derive(Debug, Clone, Serialize)]
pub struct ParakeetModelState {
    pub id: String,
    pub display_name: String,
    pub multilingual: bool,
    pub is_quantized: bool,
    pub size_bytes: u64,
    pub notes: String,
    pub downloaded: bool,
    pub missing_files: Vec<String>,
    pub on_disk_bytes: Option<u64>,
    pub path: Option<String>,
    pub speed: f32,
    pub accuracy: f32,
    pub language_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct DownloadProgress {
    id: String,
    downloaded: u64,
    total: u64,
    current_file: String,
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


pub struct ParakeetModelManager {
    app: AppHandle,
    cancel_flags: Mutex<std::collections::HashMap<String, Arc<std::sync::atomic::AtomicBool>>>,
}

impl ParakeetModelManager {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            cancel_flags: Mutex::new(Default::default()),
        }
    }

    pub fn root_dir(&self) -> Result<PathBuf> {
        let base = self
            .app
            .path()
            .app_local_data_dir()
            .map_err(|e| anyhow!("app_local_data_dir: {e}"))?;
        let dir = base.join("ParakeetModels");
        fs::create_dir_all(&dir).ok();
        Ok(dir)
    }

    pub fn variant_dir(&self, v: &ParakeetVariant) -> Result<PathBuf> {
        Ok(self.root_dir()?.join(v.id))
    }

    pub fn path_for_id(&self, id: &str) -> Option<PathBuf> {
        let v = find_variant(id)?;
        let d = self.variant_dir(v).ok()?;
        if self.missing_files(v).is_empty() && d.exists() {
            Some(d)
        } else {
            None
        }
    }

    fn missing_files(&self, v: &ParakeetVariant) -> Vec<String> {
        let Ok(dir) = self.variant_dir(v) else {
            return v.files.iter().map(|s| s.to_string()).collect();
        };
        v.files
            .iter()
            .filter(|f| !dir.join(f).exists())
            .map(|s| s.to_string())
            .collect()
    }

    pub fn list(&self) -> Result<Vec<ParakeetModelState>> {
        let mut out = Vec::with_capacity(PARAKEET_VARIANTS.len());
        for v in PARAKEET_VARIANTS {
            let dir = self.variant_dir(v)?;
            let missing = self.missing_files(v);
            let downloaded = missing.is_empty();
            let on_disk_bytes = if downloaded {
                let mut sum = 0u64;
                for f in v.files {
                    if let Ok(meta) = dir.join(f).metadata() {
                        sum += meta.len();
                    }
                }
                Some(sum)
            } else {
                None
            };
            out.push(ParakeetModelState {
                id: v.id.into(),
                display_name: v.display_name.into(),
                multilingual: v.multilingual,
                is_quantized: v.is_quantized,
                size_bytes: v.size_bytes,
                notes: v.notes.into(),
                downloaded,
                missing_files: missing,
                on_disk_bytes,
                path: downloaded.then(|| dir.to_string_lossy().into_owned()),
                speed: v.speed,
                accuracy: v.accuracy,
                language_codes: v.language_codes.iter().map(|s| s.to_string()).collect(),
            });
        }
        Ok(out)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let v = find_variant(id).ok_or_else(|| anyhow!("variante inconnue: {id}"))?;
        let dir = self.variant_dir(v)?;
        if dir.exists() {
            fs::remove_dir_all(&dir)
                .with_context(|| format!("remove_dir_all {}", dir.display()))?;
            info!(id, "Modele Parakeet supprime");
        }
        Ok(())
    }

    pub fn cancel_download(&self, id: &str) {
        if let Some(flag) = self.cancel_flags.lock().get(id) {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    pub async fn download(&self, id: &str) -> Result<PathBuf> {
        // Reentrancy guard up front. If we bail here, no cancel flag was
        // inserted and no error event is emitted for this second call.
        {
            let mut flags = self.cancel_flags.lock();
            if flags.contains_key(id) {
                return Err(anyhow!("telechargement deja en cours: {id}"));
            }
            let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
            flags.insert(id.to_string(), cancel);
        }

        let result = self.download_impl(id).await;

        // Always clean up the cancel flag, regardless of success / error /
        // cancellation. Frontend needs an explicit error event so the UI
        // can clear the progress loader and re-enable the download button.
        self.cancel_flags.lock().remove(id);
        if let Err(e) = &result {
            let _ = self.app.emit(
                "parakeet_model:download:error",
                DownloadError {
                    id: id.to_string(),
                    message: e.to_string(),
                },
            );
        }
        result
    }

    async fn download_impl(&self, id: &str) -> Result<PathBuf> {
        let v = find_variant(id).ok_or_else(|| anyhow!("variante inconnue: {id}"))?;
        let dir = self.variant_dir(v)?;
        fs::create_dir_all(&dir).ok();

        let cancel = self
            .cancel_flags
            .lock()
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("cancel flag missing for {id}"))?;

        let client = reqwest::Client::new();

        // Deux passes : d'abord HEAD pour additionner les total bytes (pour
        // afficher une progression globale coherente), puis GET sequentiel.
        let mut total_global: u64 = 0;
        let mut missing: Vec<&str> = Vec::new();
        for f in v.files {
            let target = dir.join(f);
            if target.exists() {
                continue;
            }
            missing.push(f);
            let url = file_url(v.repo, f);
            if let Ok(resp) = client.head(&url).send().await {
                if let Some(len) = resp.content_length() {
                    total_global += len;
                }
            }
        }
        if total_global == 0 {
            // Tout est deja present ou head failed : fallback sur la taille
            // declaree dans le catalogue.
            total_global = v.size_bytes;
        }

        let mut downloaded_global: u64 = 0;
        for f in missing {
            if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(anyhow!("telechargement annule"));
            }
            let url = file_url(v.repo, f);
            let target = dir.join(f);
            let tmp = target.with_extension("part");
            let _ = fs::remove_file(&tmp);

            let resp = client
                .get(&url)
                .send()
                .await
                .with_context(|| format!("GET {url}"))?;
            if !resp.status().is_success() {
                anyhow::bail!("HTTP {} depuis {url}", resp.status());
            }

            let mut file = tokio::fs::File::create(&tmp)
                .await
                .with_context(|| format!("create {}", tmp.display()))?;
            let mut stream = resp.bytes_stream();
            let mut last_emit = std::time::Instant::now();
            while let Some(chunk) = stream.next().await {
                if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                    drop(file);
                    let _ = fs::remove_file(&tmp);
                    self.cancel_flags.lock().remove(id);
                    return Err(anyhow!("telechargement annule"));
                }
                let bytes = chunk.context("chunk recv")?;
                file.write_all(&bytes).await?;
                downloaded_global += bytes.len() as u64;
                if last_emit.elapsed() >= std::time::Duration::from_millis(50) {
                    let _ = self.app.emit(
                        "parakeet_model:download:progress",
                        DownloadProgress {
                            id: id.to_string(),
                            downloaded: downloaded_global,
                            total: total_global,
                            current_file: f.to_string(),
                        },
                    );
                    last_emit = std::time::Instant::now();
                }
            }
            file.flush().await?;
            drop(file);
            fs::rename(&tmp, &target)
                .with_context(|| format!("rename {} -> {}", tmp.display(), target.display()))?;
        }

        let _ = self.app.emit(
            "parakeet_model:download:complete",
            DownloadComplete {
                id: id.to_string(),
                path: dir.to_string_lossy().into_owned(),
            },
        );
        info!(id, path = %dir.display(), "Parakeet telecharge");
        Ok(dir)
    }
}

fn file_url(repo: &str, file: &str) -> String {
    format!("https://huggingface.co/{repo}/resolve/main/{file}")
}

pub struct ParakeetModelManagerState(pub Arc<ParakeetModelManager>);

impl ParakeetModelManagerState {
    pub fn new(app: AppHandle) -> Self {
        Self(Arc::new(ParakeetModelManager::new(app)))
    }
}
