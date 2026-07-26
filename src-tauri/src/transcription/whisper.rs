// Wrapper whisper-rs : chargement du modele + transcription d'un WAV 16 kHz mono.
//
// Reference VoiceInk : VoiceInk/Transcription/Core/Whisper/LibWhisper.swift
// Parametres alignes : translate=false, single_segment=false, print_*=false,
// suppress_blank=true, language=auto par defaut.
// La VAD Silero est appliquee en amont via WhisperVadContext (cf vad.rs).

use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use tracing::{debug, info};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Parametres d'une session de transcription (modeles utiles pour Phase 1c).
#[derive(Debug, Clone)]
pub struct WhisperParams {
    /// Code langue ISO 639-1 (ex "fr", "en"). `None` = auto-detect.
    pub language: Option<String>,
    /// Prompt initial pour guider le decodage (jargon, noms propres...).
    pub initial_prompt: Option<String>,
    /// Nombre de threads CPU. 0 = auto (nombre de cores physiques).
    pub n_threads: usize,
}

impl Default for WhisperParams {
    fn default() -> Self {
        Self {
            language: None,
            initial_prompt: None,
            n_threads: 0,
        }
    }
}

/// Contexte Whisper charge : encapsule le modele en memoire, reutilisable entre
/// transcriptions. Thread-safe derriere un Mutex.
pub struct WhisperEngine {
    inner: Arc<Mutex<Option<WhisperContext>>>,
    loaded_path: Arc<Mutex<Option<String>>>,
}

impl Default for WhisperEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl WhisperEngine {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            loaded_path: Arc::new(Mutex::new(None)),
        }
    }

    /// Libere le `WhisperContext` si un modele est charge. Fait chuter la
    /// RAM de l'equivalent du .bin (150 Mo pour base jusqu'a 3 Go pour
    /// large-v3). Utilise apres chaque pipeline pour ne pas garder le
    /// modele resident entre deux dictees (parite VoiceInk
    /// WhisperModelManager.unloadModel / cleanupResources).
    pub fn unload(&self) {
        let had_model = self.inner.lock().take().is_some();
        *self.loaded_path.lock() = None;
        if had_model {
            info!("Modele Whisper decharge");
        }
    }

    /// Charge ou recharge un modele si necessaire.
    pub fn load(&self, model_path: &Path) -> Result<()> {
        let path_str = model_path.to_string_lossy().to_string();
        {
            let guard = self.loaded_path.lock();
            if guard.as_deref() == Some(path_str.as_str()) {
                return Ok(());
            }
        }

        info!(path = %path_str, "Chargement du modele Whisper");
        let ctx_params = WhisperContextParameters::default();
        let ctx = WhisperContext::new_with_params(&path_str, ctx_params)
            .map_err(|e| anyhow!("whisper load: {e}"))?;

        *self.inner.lock() = Some(ctx);
        *self.loaded_path.lock() = Some(path_str);
        Ok(())
    }

    /// Transcrit un fichier WAV 16 kHz mono Int16 et retourne le texte.
    /// Le texte n'est pas post-traite (filter, word-replace, enhance) : ces
    /// etapes sont faites dans le pipeline superieur, comme VoiceInk.
    pub fn transcribe_wav(&self, wav_path: &Path, params: &WhisperParams) -> Result<String> {
        let samples = read_wav_as_f32(wav_path)
            .with_context(|| format!("lecture WAV {}", wav_path.display()))?;
        self.transcribe_samples(&samples, params)
    }

    /// Transcrit un buffer Float32 mono 16 kHz deja decode (utile pour VAD).
    pub fn transcribe_samples(&self, samples: &[f32], params: &WhisperParams) -> Result<String> {
        let ctx_guard = self.inner.lock();
        let ctx = ctx_guard
            .as_ref()
            .ok_or_else(|| anyhow!("aucun modele Whisper charge"))?;
        let mut state = ctx
            .create_state()
            .map_err(|e| anyhow!("whisper state: {e}"))?;

        let mut whisper_params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

        if let Some(lang) = params.language.as_deref() {
            whisper_params.set_language(Some(lang));
        } else {
            whisper_params.set_language(Some("auto"));
        }
        whisper_params.set_translate(false);
        whisper_params.set_single_segment(false);
        whisper_params.set_print_progress(false);
        whisper_params.set_print_timestamps(false);
        whisper_params.set_print_special(false);
        whisper_params.set_print_realtime(false);
        whisper_params.set_suppress_blank(true);

        if let Some(prompt) = params.initial_prompt.as_deref() {
            whisper_params.set_initial_prompt(prompt);
        }

        let n_threads = if params.n_threads == 0 {
            num_cpus_physical()
        } else {
            params.n_threads as i32
        };
        whisper_params.set_n_threads(n_threads);

        // VAD : gere en amont via WhisperVadContext (cf transcription/vad.rs).
        // Ici on passe les samples bruts ou deja filtres par la VAD externe.
        debug!(
            n_samples = samples.len(),
            n_threads, "Lancement whisper_full"
        );

        state
            .full(whisper_params, samples)
            .map_err(|e| anyhow!("whisper_full: {e}"))?;

        // whisper-rs 0.16 : iterateur de segments. Le Display de WhisperSegment
        // renvoie le texte (avec remplacement des octets UTF-8 invalides).
        let mut text = String::new();
        for segment in state.as_iter() {
            text.push_str(&format!("{segment}"));
        }
        Ok(text.trim().to_string())
    }
}

pub fn read_wav_as_f32(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            let max = (1i64 << (bits - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max).map_err(anyhow::Error::from))
                .collect::<Result<Vec<_>>>()?
        }
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| s.map_err(anyhow::Error::from))
            .collect::<Result<Vec<_>>>()?,
    };

    // Si le WAV est stereo, on mixdown en moyenne.
    let samples = if spec.channels > 1 {
        let ch = spec.channels as usize;
        samples
            .chunks_exact(ch)
            .map(|f| f.iter().sum::<f32>() / ch as f32)
            .collect()
    } else {
        samples
    };

    Ok(samples)
}

fn num_cpus_physical() -> i32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4)
}
