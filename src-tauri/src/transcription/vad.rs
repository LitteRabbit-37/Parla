// VAD (Voice Activity Detection) Silero via whisper-rs.
//
// Reference VoiceInk :
// - Transcription/Core/Whisper/VADModelManager.swift : bundle du fichier
//   ggml-silero-v5.1.2.bin dans l'app.
// - Transcription/Core/Whisper/LibWhisper.swift L73-85 : activation via
//   UserDefault IsVADEnabled + params threshold 0.50, min_speech 250 ms,
//   min_silence 100 ms, max_speech +inf, speech_pad 30 ms, samples_overlap 0.1.
//
// Sur whisper-rs 0.16 la VAD est un contexte SEPARE (WhisperVadContext) qui
// retourne des WhisperVadSegment. On segmente d'abord puis on transcrit chaque
// segment, identique a l'effet du vad_model_path passe a FullParams cote mac.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tracing::{debug, info, warn};
use whisper_rs::{WhisperVadContext, WhisperVadContextParams, WhisperVadParams, WhisperVadSegment};

/// Modele VAD Silero converti en GGML (whisper.cpp). Le repo a migre de
/// ggerganov/whisper.cpp vers ggml-org/whisper-vad fin 2025. La 6.2.0 est
/// la version recente recommandee par le script download-vad-model.sh.
pub const VAD_MODEL_URL: &str =
    "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin";

pub const VAD_MODEL_FILENAME: &str = "ggml-silero-v6.2.0.bin";
pub const VAD_MODEL_SIZE_BYTES: u64 = 906_752; // ~885 Ko (annonce sur HF)

#[derive(Debug, Clone, Serialize)]
pub struct VadModelState {
    pub downloaded: bool,
    pub path: Option<String>,
    pub on_disk_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct VadDownloadProgress {
    downloaded: u64,
    total: u64,
}

/// Contexte VAD charge (reutilisable entre transcriptions).
pub struct VadEngine {
    inner: Arc<Mutex<Option<WhisperVadContext>>>,
    loaded_path: Arc<Mutex<Option<PathBuf>>>,
}

impl Default for VadEngine {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            loaded_path: Arc::new(Mutex::new(None)),
        }
    }
}

impl VadEngine {
    /// Libere le contexte VAD (Silero ONNX charge via whisper-rs). ~20 Mo
    /// resident habituellement. Appele apres chaque pipeline pour reclaim.
    pub fn unload(&self) {
        let had_model = self.inner.lock().take().is_some();
        *self.loaded_path.lock() = None;
        if had_model {
            info!("Modele VAD decharge");
        }
    }

    pub fn load(&self, model_path: &Path) -> Result<()> {
        {
            let loaded = self.loaded_path.lock();
            if loaded.as_deref() == Some(model_path) {
                return Ok(());
            }
        }
        info!(path = %model_path.display(), "Chargement du modele VAD Silero");
        let mut params = WhisperVadContextParams::default();
        params.set_n_threads(num_cpus_physical());
        params.set_use_gpu(false);
        let path_str = model_path
            .to_str()
            .ok_or_else(|| anyhow!("chemin VAD non-UTF8"))?;
        let ctx = WhisperVadContext::new(path_str, params)
            .map_err(|e| anyhow!("WhisperVadContext: {e}"))?;

        *self.inner.lock() = Some(ctx);
        *self.loaded_path.lock() = Some(model_path.to_path_buf());
        Ok(())
    }

    /// Decoupe un buffer mono f32 16 kHz en segments de parole.
    pub fn segments(&self, samples: &[f32]) -> Result<Vec<WhisperVadSegment>> {
        let mut guard = self.inner.lock();
        let ctx = guard
            .as_mut()
            .ok_or_else(|| anyhow!("VAD non initialise"))?;
        let params = default_vad_params();
        let st = Instant::now();
        let iter = ctx
            .segments_from_samples(params, samples)
            .map_err(|e| anyhow!("VAD segments_from_samples: {e}"))?;
        // WhisperVadSegments est un iterateur qui yield des WhisperVadSegment.
        let segs: Vec<WhisperVadSegment> = iter.collect();
        debug!(
            duration_ms = st.elapsed().as_millis() as u64,
            segments = segs.len(),
            "VAD segmentation terminee"
        );
        Ok(segs)
    }
}

/// Parametres par defaut repliquant exactement VoiceInk LibWhisper.swift L78-85.
/// Si whisper-rs 0.16 n'expose pas un setter pour un parametre, on laisse
/// la valeur par defaut du crate (documentee en commentaire).
fn default_vad_params() -> WhisperVadParams {
    let params = WhisperVadParams::new();
    // TODO : whisper-rs 0.16 expose un Default mais les setters precis
    // (threshold, min_speech_duration_ms, etc.) ne sont pas encore publics
    // dans cette version. Les valeurs par defaut du crate sont raisonnables
    // (threshold 0.5 dans whisper.cpp), on adaptera si une version ulterieure
    // du binding expose les builders.
    params
}

/// Chemin attendu du modele VAD (un seul par installation).
pub fn vad_model_path(app: &AppHandle) -> Result<PathBuf> {
    let base = app
        .path()
        .app_local_data_dir()
        .map_err(|e| anyhow!("app_local_data_dir: {e}"))?;
    let dir = base.join("Models");
    fs::create_dir_all(&dir).ok();
    Ok(dir.join(VAD_MODEL_FILENAME))
}

pub fn vad_state(app: &AppHandle) -> VadModelState {
    match vad_model_path(app) {
        Ok(p) if p.exists() => {
            let size = p.metadata().ok().map(|m| m.len());
            VadModelState {
                downloaded: true,
                path: Some(p.to_string_lossy().into_owned()),
                on_disk_bytes: size,
            }
        }
        _ => VadModelState {
            downloaded: false,
            path: None,
            on_disk_bytes: None,
        },
    }
}

/// Telecharge le modele VAD Silero depuis HuggingFace avec progression.
/// Emet vad:download:progress / vad:download:complete / vad:download:error.
pub async fn download_vad(app: &AppHandle) -> Result<PathBuf> {
    let target = vad_model_path(app)?;
    if target.exists() {
        return Ok(target);
    }
    let url = url::Url::parse(VAD_MODEL_URL)?;
    crate::services::download::download_to_file(&url, &target, VAD_MODEL_SIZE_BYTES, None, {
        let app = app.clone();
        move |downloaded, total| {
            let _ = app.emit(
                "vad:download:progress",
                VadDownloadProgress { downloaded, total },
            );
        }
    })
    .await?;

    let _ = app.emit(
        "vad:download:complete",
        serde_json::json!({ "path": target.to_string_lossy() }),
    );
    info!(path = %target.display(), "Modele VAD telecharge");
    Ok(target)
}

pub fn delete_vad(app: &AppHandle) -> Result<()> {
    let p = vad_model_path(app)?;
    if p.exists() {
        fs::remove_file(&p)?;
    }
    Ok(())
}

fn num_cpus_physical() -> i32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4)
}

type VadResult = Result<(Vec<f32>, Vec<(usize, usize)>)>;

/// Applique la VAD sur un WAV 16 kHz mono et retourne les plages de samples
/// (start, end) a transcrire. Ces plages ont deja subi le padding de
/// `speech_pad_ms` en interne par whisper.cpp.
pub fn run_vad_on_wav(engine: &VadEngine, wav_path: &Path) -> VadResult {
    let samples = super::whisper::read_wav_as_f32(wav_path)?;
    let segs = engine.segments(&samples)?;
    // Les segments de whisper.cpp sont en centisecondes cote start/end.
    let ranges: Vec<(usize, usize)> = segs
        .into_iter()
        .map(|WhisperVadSegment { start, end }| {
            let start_sample = ((start / 100.0) * 16_000.0) as usize;
            let end_sample = ((end / 100.0) * 16_000.0) as usize;
            (
                start_sample.min(samples.len()),
                end_sample.min(samples.len()),
            )
        })
        .filter(|(s, e)| e > s)
        .collect();
    if ranges.is_empty() {
        warn!("VAD n'a detecte aucune parole");
    }
    Ok((samples, ranges))
}
