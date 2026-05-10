// Pipeline post-enregistrement : transcription + paste au curseur.
//
// Reference VoiceInk : VoiceInk/Transcription/Core/TranscriptionPipeline.swift
// Ordre exact cote VoiceInk (L39-197) : transcribe -> filter -> format ->
// word-replace -> prompt-detect -> AI enhance -> save -> paste -> dismiss.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_store::StoreExt;
use tokio::task;
use tracing::{info, warn};

use crate::db::{transcription as history_repo, word_replacement as word_repo, Database};
use crate::enhancement::prompt_detection;
use crate::enhancement::prompts as prompt_store;
use crate::enhancement::service as llm_service;
use crate::mini_recorder;
use crate::paste::paste_at_cursor;
use crate::services::api_keys;
use crate::text_processing::{
    filler_words, filter as output_filter, formatter as text_formatter,
    word_replacement as word_replacer,
};
use crate::transcription::{
    cloud::{
        catalog::CLOUD_MODELS,
        provider::TranscribeRequest,
        streaming::{StreamingConfig, StreamingHandle, StreamingRegistry},
        CloudRegistry,
    },
    model_manager::ModelManager,
    vad,
    whisper::{self as whisper_core, WhisperParams},
};

/// Nom du store Tauri ou est persiste l'ID du modele selectionne.
const STORE_FILE: &str = "parla.settings.json";
const SELECTED_MODEL_KEY: &str = "selected_whisper_model";
const LANGUAGE_KEY: &str = "whisper_language";
const RESTORE_CLIPBOARD_KEY: &str = "restore_clipboard_after_paste";
const APPEND_TRAILING_SPACE_KEY: &str = "append_trailing_space";
const TEXT_FORMATTING_KEY: &str = "text_formatting_enabled";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PipelineState {
    Transcribing,
    Enhancing,
    Pasting,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct PipelineEvent {
    pub state: PipelineState,
    pub message: Option<String>,
    pub text: Option<String>,
    pub duration_ms: Option<u64>,
}

/// State qui tient l'id de la ligne history en cours pour la session
/// active (non-streaming ou streaming). Set au record start, lu a
/// chaque etape du pipeline.
#[derive(Default)]
pub struct HistorySessionState(pub parking_lot::Mutex<Option<String>>);

fn current_history_id(app: &AppHandle) -> Option<String> {
    app.try_state::<HistorySessionState>()
        .and_then(|s| s.0.lock().clone())
}

fn clear_history_id(app: &AppHandle) {
    if let Some(s) = app.try_state::<HistorySessionState>() {
        *s.0.lock() = None;
    }
}

/// Lit la duree du WAV pour le stocker dans la ligne history.
fn wav_duration_secs(wav_path: &Path) -> Option<f64> {
    use hound::WavReader;
    let reader = WavReader::open(wav_path).ok()?;
    let spec = reader.spec();
    let samples = reader.duration() as f64;
    Some(samples / spec.sample_rate as f64)
}

pub fn insert_pending_streaming_row(app: &AppHandle, provider: &str, model: &str) {
    let Some(db) = app.try_state::<Database>() else {
        return;
    };
    let conn = db.0.lock();
    match history_repo::insert_pending(&conn, None, None) {
        Ok(id) => {
            drop(conn);
            if let Some(state) = app.try_state::<HistorySessionState>() {
                *state.0.lock() = Some(id.clone());
            }
            // Pre-remplit transcription_model_name pour la ligne streaming
            // (sera ecrasee par mark_transcribed au finalize).
            let conn = db.0.lock();
            let label = format!("{provider} / {model} (streaming)");
            let _ = conn.execute(
                "UPDATE transcriptions SET transcription_model_name = ?1 WHERE id = ?2",
                rusqlite::params![label, id],
            );
            drop(conn);
            let _ = app.emit("history:created", &id);
        }
        Err(e) => warn!("history insert_pending streaming: {e}"),
    }
}

fn insert_pending_row(app: &AppHandle, wav_path: Option<&Path>) {
    let Some(db) = app.try_state::<Database>() else {
        return;
    };
    let audio_name = wav_path
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .map(String::from);
    let duration_sec = wav_path.and_then(wav_duration_secs);
    let conn = db.0.lock();
    match history_repo::insert_pending(&conn, audio_name.as_deref(), duration_sec) {
        Ok(id) => {
            drop(conn);
            if let Some(state) = app.try_state::<HistorySessionState>() {
                *state.0.lock() = Some(id.clone());
            }
            let _ = app.emit("history:created", &id);
        }
        Err(e) => warn!("history insert_pending: {e}"),
    }
}

fn mark_failed(app: &AppHandle, err: &str) {
    let Some(id) = current_history_id(app) else {
        return;
    };
    if let Some(db) = app.try_state::<Database>() {
        if let Err(e) = history_repo::mark_failed(&db.0.lock(), &id, err) {
            warn!("history mark_failed: {e}");
        }
    }
    let _ = app.emit("history:updated", &id);
}

pub fn get_selected_model(app: &AppHandle) -> Option<String> {
    let store = app.store(STORE_FILE).ok()?;
    store
        .get(SELECTED_MODEL_KEY)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
}

pub fn set_selected_model(app: &AppHandle, id: Option<&str>) -> Result<()> {
    let store = app
        .store(STORE_FILE)
        .map_err(|e| anyhow!("store open: {e}"))?;
    match id {
        Some(id) => store.set(SELECTED_MODEL_KEY, serde_json::Value::String(id.to_string())),
        None => {
            store.delete(SELECTED_MODEL_KEY);
        }
    }
    store.save().map_err(|e| anyhow!("store save: {e}"))?;
    Ok(())
}

pub fn get_language(app: &AppHandle) -> Option<String> {
    let store = app.store(STORE_FILE).ok()?;
    let raw = store
        .get(LANGUAGE_KEY)
        .and_then(|v| v.as_str().map(|s| s.to_string()))?;
    // "auto" est notre valeur explicite pour auto-detect ; le pipeline whisper
    // (et les providers cloud) attendent None / null dans ce cas.
    if raw == "auto" || raw.is_empty() {
        None
    } else {
        Some(raw)
    }
}

pub fn get_restore_clipboard(app: &AppHandle) -> bool {
    let Some(store) = app.store(STORE_FILE).ok() else {
        return true;
    };
    store
        .get(RESTORE_CLIPBOARD_KEY)
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

pub fn get_append_trailing_space(app: &AppHandle) -> bool {
    let Some(store) = app.store(STORE_FILE).ok() else {
        return false;
    };
    store
        .get(APPEND_TRAILING_SPACE_KEY)
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

pub fn get_text_formatting_enabled(app: &AppHandle) -> bool {
    let Some(store) = app.store(STORE_FILE).ok() else {
        return false;
    };
    store
        .get(TEXT_FORMATTING_KEY)
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn is_vad_enabled(app: &AppHandle) -> bool {
    let Some(store) = app.store(STORE_FILE).ok() else {
        return false;
    };
    store
        .get("vad_enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

pub fn supports_streaming(provider_id: &str, model_id: &str) -> bool {
    CLOUD_MODELS
        .iter()
        .any(|m| m.provider_id == provider_id && m.model_id == model_id && m.supports_streaming)
}

/// Determine si la source actuelle doit utiliser le streaming cloud.
/// Renvoie Some((provider, model)) si oui, None sinon.
pub fn active_streaming_target(app: &AppHandle) -> Option<(String, String)> {
    let store = app.store(STORE_FILE).ok()?;
    let kind = store.get("transcription_source_kind")?.as_str()?.to_string();
    if kind != "cloud" {
        return None;
    }
    let provider = store
        .get("selected_cloud_provider")?
        .as_str()?
        .to_string();
    let model = store.get("selected_cloud_model")?.as_str()?.to_string();
    if supports_streaming(&provider, &model) {
        Some((provider, model))
    } else {
        None
    }
}

/// Lance une session streaming cloud via le registry et retourne le handle.
pub async fn start_streaming_session(
    app: AppHandle,
    provider: String,
    model: String,
    language: Option<String>,
) -> Result<StreamingHandle> {
    let registry: Arc<StreamingRegistry> = app
        .state::<crate::commands::streaming::StreamingRegistryState>()
        .0
        .clone();

    let key = api_keys::get_api_key(&provider)?
        .ok_or_else(|| anyhow!("PARLA_ERR:apiKey:{provider}"))?;

    // Injection du dictionnaire enabled comme en batch.
    let mut custom_vocab = Vec::new();
    if let Some(db) = app.try_state::<Database>() {
        if let Ok(rules) = word_repo::list_enabled(&db.0.lock()) {
            for r in rules {
                for v in r.original_text.split(',') {
                    let t = v.trim();
                    if !t.is_empty() {
                        custom_vocab.push(t.to_string());
                    }
                }
            }
        }
    }

    let config = StreamingConfig {
        model,
        language,
        custom_vocabulary: custom_vocab,
    };

    crate::transcription::cloud::streaming::start_streaming(app, registry, provider, key, config)
        .await
}

/// Finalise une session streaming et applique le post-traitement + paste.
pub async fn finalize_streaming_session(
    app: AppHandle,
    handle: StreamingHandle,
) -> Result<()> {
    let start = std::time::Instant::now();
    let text = handle.finalize().await?;
    let duration_ms = start.elapsed().as_millis() as u64;
    info!(chars = text.len(), duration_ms, "Streaming cloud finalise");
    mark_transcribed_in_history(
        &app,
        &text,
        duration_ms,
        "streaming",
        get_language(&app).as_deref(),
    );
    let result = finalize_text(&app, text, duration_ms).await;
    // Fin de la session Power Mode (idem branche batch).
    crate::power_mode::session::end_session(&app);
    let _ = app.emit("power_mode:active", serde_json::Value::Null);
    clear_history_id(&app);
    result
}

/// Declenche le pipeline complet apres la fin d'un enregistrement : transcribe
/// puis paste. Emet des evenements pipeline:state vers le frontend.
pub fn run_after_recording(app: AppHandle, wav_path: PathBuf) {
    insert_pending_row(&app, Some(&wav_path));
    let app_bg = app.clone();
    // tauri::async_runtime::spawn fonctionne depuis n'importe quel thread
    // (le hotkey-dispatch est un std::thread sans runtime Tokio attache).
    tauri::async_runtime::spawn(async move {
        let result = run_pipeline(app_bg.clone(), wav_path).await;
        if let Err(e) = result {
            warn!("Pipeline echec: {e}");
            mark_failed(&app_bg, &e.to_string());
            let _ = app_bg.emit(
                "pipeline:state",
                PipelineEvent {
                    state: PipelineState::Failed,
                    message: Some(e.to_string()),
                    text: None,
                    duration_ms: None,
                },
            );
        }
        // Libere les modeles d'inference maintenant que le pipeline est
        // termine, succes ou echec. Evite de garder en permanence whisper
        // (~150 Mo a 3 Go), parakeet (~600 Mo), VAD (~20 Mo) et le runtime
        // llama.cpp (taille du GGUF selectionne). Reference VoiceInk
        // VoiceInkEngine.cleanupResources -> WhisperModelManager
        // .releaseResources -> whisper_free. La prochaine dictee
        // rechargera a la volee.
        unload_inference_models(&app_bg);
        // VoiceInk dismiss la fenetre immediatement apres le paste done
        // (RecorderUIManager.dismissMiniRecorder, pas de delay visuel).
        // On garde 200 ms de buffer pour absorber les derniers events.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        mini_recorder::close(&app_bg);
        // Fin de la session Power Mode : restauration de la baseline
        // si auto_restore est active (reference VoiceInk
        // PowerModeSessionManager.endSession).
        crate::power_mode::session::end_session(&app_bg);
        let _ = app_bg.emit("power_mode:active", serde_json::Value::Null);
        clear_history_id(&app_bg);
    });
}

/// Decharge tous les modeles d'inference resident en memoire (whisper,
/// parakeet, VAD, llama.cpp). Appele en fin de pipeline pour que la RAM
/// idle de Parla redescende a ~200 Mo (Tauri + WebView2 + SQLite ouvert)
/// au lieu de ~800 Mo ou plus selon les modeles utilises. Chaque engine
/// gere son propre Mutex, donc cette fonction est sans erreur et sans
/// await : on tombe juste le Arc<Option<T>> a None.
fn unload_inference_models(app: &AppHandle) {
    if let Some(st) = app.try_state::<crate::commands::transcription::WhisperEngineState>() {
        st.0.unload();
    }
    if let Some(st) = app.try_state::<crate::commands::vad::VadEngineState>() {
        st.0.unload();
    }
    if let Some(st) = app.try_state::<crate::transcription::parakeet::ParakeetEngineState>() {
        st.0.unload();
    }
    if let Some(rt) = crate::enhancement::service::llama_runtime() {
        rt.unload();
    }
}

#[derive(Debug, Clone)]
enum Source {
    Local { model_id: String },
    Cloud { provider: String, model: String },
    Parakeet { model_id: String },
}

fn resolve_source(app: &AppHandle) -> Source {
    let store = match app.store(STORE_FILE) {
        Ok(s) => s,
        Err(_) => {
            return Source::Local {
                model_id: get_selected_model(app).unwrap_or_default(),
            };
        }
    };
    let kind = store
        .get("transcription_source_kind")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "local".into());
    match kind.as_str() {
        "cloud" => {
            let provider = store
                .get("selected_cloud_provider")
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_default();
            let model = store
                .get("selected_cloud_model")
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_default();
            Source::Cloud { provider, model }
        }
        "parakeet" => Source::Parakeet {
            model_id: store
                .get("selected_parakeet_model")
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_default(),
        },
        _ => Source::Local {
            model_id: get_selected_model(app).unwrap_or_default(),
        },
    }
}

async fn transcribe_cloud(
    app: &AppHandle,
    wav_path: &Path,
    provider_id: &str,
    model: &str,
    language: Option<String>,
) -> Result<(String, u64)> {
    let registry: std::sync::Arc<CloudRegistry> = app
        .state::<crate::commands::cloud::CloudRegistryState>()
        .0
        .clone();
    let provider = registry
        .find(provider_id)
        .ok_or_else(|| anyhow!("provider cloud inconnu: {provider_id}"))?;
    let key = api_keys::get_api_key(provider_id)
        .map_err(|e| anyhow!("keyring: {e}"))?
        .ok_or_else(|| anyhow!("PARLA_ERR:apiKey:{provider_id}"))?;

    let mut req = TranscribeRequest::new(model);
    req.language = language;

    // Injection du dictionnaire enabled (VoiceInk Soniox/Deepgram/Speechmatics).
    if let Some(db) = app.try_state::<Database>() {
        if let Ok(rules) = word_repo::list_enabled(&db.0.lock()) {
            for r in rules {
                for v in r.original_text.split(',') {
                    let t = v.trim();
                    if !t.is_empty() {
                        req.custom_vocabulary.push(t.to_string());
                    }
                }
            }
        }
    }

    let start = std::time::Instant::now();
    let text = provider.transcribe(wav_path, &key, &req).await?;
    Ok((text, start.elapsed().as_millis() as u64))
}

async fn run_pipeline(app: AppHandle, wav_path: PathBuf) -> Result<()> {
    let source = resolve_source(&app);
    let language = get_language(&app);
    let params = WhisperParams {
        language: language.clone(),
        ..Default::default()
    };

    let source_label = match &source {
        Source::Local { model_id } => format!("local / {model_id}"),
        Source::Cloud { provider, model } => format!("{provider} / {model}"),
        Source::Parakeet { model_id } => format!("parakeet / {model_id}"),
    };
    let _ = app.emit(
        "pipeline:state",
        PipelineEvent {
            state: PipelineState::Transcribing,
            message: Some(format!("Transcription {source_label}")),
            text: None,
            duration_ms: None,
        },
    );

    // Branche cloud : pas de VAD, pas de WhisperEngine. Upload direct au provider.
    if let Source::Cloud { provider, model } = &source {
        if provider.is_empty() || model.is_empty() {
            anyhow::bail!("PARLA_ERR:cloudUnconfigured");
        }
        let (text, duration_ms) = transcribe_cloud(
            &app,
            &wav_path,
            provider,
            model,
            language.clone(),
        )
        .await?;
        info!(chars = text.len(), duration_ms, provider, model, "Transcription cloud terminee");
        mark_transcribed_in_history(
            &app,
            &text,
            duration_ms,
            &format!("{provider} / {model}"),
            language.as_deref(),
        );
        return finalize_text(&app, text, duration_ms).await;
    }

    // Branche Parakeet (local via parakeet-rs).
    if let Source::Parakeet { model_id } = &source {
        if model_id.is_empty() {
            anyhow::bail!("PARLA_ERR:noParakeet");
        }
        let mgr = app
            .state::<crate::commands::parakeet::ParakeetModelManagerState>()
            .0
            .clone();
        let model_dir = mgr
            .path_for_id(model_id)
            .ok_or_else(|| anyhow!("PARLA_ERR:parakeetIncomplete:{model_id}"))?;
        let engine_state = app
            .state::<crate::transcription::parakeet::ParakeetEngineState>()
            .0
            .clone();
        let wav_path_clone = wav_path.clone();
        let language_clone = language.clone();
        let start = std::time::Instant::now();
        let text = task::spawn_blocking(move || -> Result<String> {
            engine_state.ensure_loaded(&model_dir)?;
            let samples = whisper_core::read_wav_as_f32(&wav_path_clone)?;
            engine_state.transcribe_samples(&samples, language_clone.as_deref())
        })
        .await
        .map_err(|e| anyhow!("task join: {e}"))??;
        let duration_ms = start.elapsed().as_millis() as u64;
        info!(
            chars = text.len(),
            duration_ms,
            model = model_id,
            "Transcription Parakeet terminee"
        );
        mark_transcribed_in_history(
            &app,
            &text,
            duration_ms,
            &format!("parakeet / {model_id}"),
            language.as_deref(),
        );
        return finalize_text(&app, text, duration_ms).await;
    }

    // Branche locale Whisper (comme avant).
    let Source::Local { model_id } = source.clone() else {
        unreachable!();
    };
    if model_id.is_empty() {
        anyhow::bail!("PARLA_ERR:noWhisper");
    }

    let engine = app
        .state::<crate::commands::transcription::WhisperEngineState>()
        .0
        .clone();
    let models: Arc<ModelManager> = app
        .state::<crate::commands::models::ModelManagerState>()
        .0
        .clone();

    let model_path = models
        .path_if_present(&model_id)
        .ok_or_else(|| anyhow!("PARLA_ERR:modelNotDownloaded:{model_id}"))?;

    // VAD optionnelle (matching VoiceInk IsVADEnabled). Si actif et le modele
    // Silero est present, on segmente d'abord puis on transcrit uniquement les
    // samples de parole.
    let vad_enabled = is_vad_enabled(&app);
    let vad_state = vad::vad_state(&app);
    let use_vad = vad_enabled && vad_state.downloaded;
    let vad_engine_opt = if use_vad {
        Some(
            app.state::<crate::commands::vad::VadEngineState>()
                .0
                .clone(),
        )
    } else {
        None
    };
    let vad_model_path = vad_state.path.as_ref().map(PathBuf::from);

    let wav_path_clone = wav_path.clone();
    let start = std::time::Instant::now();
    let text = task::spawn_blocking(move || -> Result<String> {
        engine.load(&model_path)?;
        if let (Some(vad_engine), Some(vad_path)) = (vad_engine_opt, vad_model_path) {
            vad_engine.load(&vad_path)?;
            let (samples, ranges) = vad::run_vad_on_wav(&vad_engine, &wav_path_clone)?;
            if ranges.is_empty() {
                return Ok(String::new());
            }
            // Concatene les plages de parole en un seul buffer et transcrit.
            let total: usize = ranges.iter().map(|(s, e)| e - s).sum();
            let mut speech: Vec<f32> = Vec::with_capacity(total);
            for (s, e) in &ranges {
                speech.extend_from_slice(&samples[*s..*e]);
            }
            engine.transcribe_samples(&speech, &params)
        } else {
            // Path non-VAD : lit le WAV et transcrit tout.
            let samples = whisper_core::read_wav_as_f32(&wav_path_clone)?;
            engine.transcribe_samples(&samples, &params)
        }
    })
    .await
    .map_err(|e| anyhow!("task join: {e}"))??;

    let duration_ms = start.elapsed().as_millis() as u64;
    info!(chars = text.len(), duration_ms, "Transcription locale terminee");
    mark_transcribed_in_history(
        &app,
        &text,
        duration_ms,
        &format!("local / {model_id}"),
        language.as_deref(),
    );
    finalize_text(&app, text, duration_ms).await
}

fn mark_transcribed_in_history(
    app: &AppHandle,
    text: &str,
    duration_ms: u64,
    model_label: &str,
    language: Option<&str>,
) {
    let Some(id) = current_history_id(app) else {
        return;
    };
    let Some(db) = app.try_state::<Database>() else {
        return;
    };
    let session = crate::power_mode::session::current(app);
    let fields = history_repo::TranscribeFields {
        text,
        transcription_duration_sec: duration_ms as f64 / 1000.0,
        transcription_model_name: model_label,
        language,
        power_mode_name: session.as_ref().map(|s| s.config_name.as_str()),
        power_mode_emoji: session.as_ref().map(|s| s.emoji.as_str()),
    };
    if let Err(e) = history_repo::mark_transcribed(&db.0.lock(), &id, &fields) {
        warn!("history mark_transcribed: {e}");
    }
    let _ = app.emit("history:updated", &id);
}

fn mark_enhanced_in_history(
    app: &AppHandle,
    enhanced: &str,
    enhance_ms: u64,
) {
    let Some(id) = current_history_id(app) else {
        return;
    };
    let Some(db) = app.try_state::<Database>() else {
        return;
    };
    // Ces 2 champs sont renseignes via le selection LLM actuel au moment
    // de l'enhance (l'info Prompt active est lue separement).
    let selection = llm_service::get_selection(app);
    let model_label = selection
        .map(|s| format!("{} / {}", s.provider_id, s.model))
        .unwrap_or_default();
    let prompt_name = match crate::enhancement::prompts::get_active_prompt(app) {
        Ok(p) => Some(p.title),
        Err(_) => None,
    };
    let fields = history_repo::EnhanceFields {
        enhanced_text: enhanced,
        enhancement_duration_sec: enhance_ms as f64 / 1000.0,
        ai_enhancement_model_name: &model_label,
        prompt_name: prompt_name.as_deref(),
        ai_request_system_message: None,
        ai_request_user_message: None,
    };
    if let Err(e) = history_repo::mark_enhanced(&db.0.lock(), &id, &fields) {
        warn!("history mark_enhanced: {e}");
    }
    let _ = app.emit("history:updated", &id);
}

/// Post-traitement + paste commun aux branches locale et cloud.
/// Ordre VoiceInk TranscriptionPipeline.swift L67-100 :
///   filter -> trim -> formatter (si active) -> word_replacement -> paste.
async fn finalize_text(app: &AppHandle, text: String, duration_ms: u64) -> Result<()> {
    let fillers = if filler_words::is_enabled(app) {
        filler_words::current_list(app)
    } else {
        Vec::new()
    };
    let mut text = output_filter::filter(&text, &fillers);

    if get_text_formatting_enabled(app) {
        text = text_formatter::format(&text);
    }

    if let Some(db) = app.try_state::<Database>() {
        match word_repo::list_enabled(&db.0.lock()) {
            Ok(rules) if !rules.is_empty() => {
                text = word_replacer::apply(&text, &rules);
            }
            Ok(_) => {}
            Err(e) => warn!("Lecture word_replacements: {e}"),
        }
    }

    if text.trim().is_empty() {
        warn!("Transcription vide, on saute le paste");
        let _ = app.emit(
            "pipeline:state",
            PipelineEvent {
                state: PipelineState::Done,
                message: Some("Aucun texte detecte".into()),
                text: Some(String::new()),
                duration_ms: Some(duration_ms),
            },
        );
        return Ok(());
    }

    // prompt-detect : cherche un trigger_word dans le texte qui activerait
    // un prompt specifique (VoiceInk PromptDetectionService). Si match, on
    // force l'enhancement ON avec ce prompt et on strip le trigger du texte.
    let detection = match prompt_store::load_cached(app) {
        Ok(all) => prompt_detection::detect_and_strip(&all, &text),
        Err(e) => {
            warn!("prompt-detect: load prompts echec: {e}");
            None
        }
    };

    let prompt_override = if let Some(ref d) = detection {
        info!(
            trigger = %d.trigger_word,
            prompt_id = %d.prompt_id,
            "trigger_word detecte, enhancement force avec prompt specifique"
        );
        text = d.processed_text.clone();
        prompt_store::load_cached(app)
            .ok()
            .and_then(|all| all.into_iter().find(|p| p.id == d.prompt_id))
    } else {
        None
    };

    // Enhancement LLM optionnel (VoiceInk TranscriptionPipeline.swift L119-150).
    // Si active et configure, remplace le texte avant paste. En cas d'echec,
    // on preserve le texte brut (fallback VoiceInk).
    let should_enhance = if prompt_override.is_some() {
        // Trigger detecte : VoiceInk force l'enhancement ON meme si le
        // toggle global est off. On exige juste un provider configure.
        llm_service::has_provider_configured(app)
    } else {
        llm_service::is_configured(app) && !llm_service::should_skip_short(app, &text)
    };

    if should_enhance {
        let _ = app.emit(
            "pipeline:state",
            PipelineEvent {
                state: PipelineState::Enhancing,
                message: Some("Enhancement LLM".into()),
                text: Some(text.clone()),
                duration_ms: Some(duration_ms),
            },
        );
        match llm_service::enhance_with_override(app.clone(), text.clone(), prompt_override).await {
            Ok(Some((enhanced, enhance_ms))) => {
                if !enhanced.trim().is_empty() {
                    mark_enhanced_in_history(app, &enhanced, enhance_ms);
                    text = enhanced;
                }
            }
            Ok(None) => {}
            Err(e) => warn!("Enhancement LLM echec, fallback texte brut: {e}"),
        }
    }

    let _ = app.emit(
        "pipeline:state",
        PipelineEvent {
            state: PipelineState::Pasting,
            message: None,
            text: Some(text.clone()),
            duration_ms: Some(duration_ms),
        },
    );

    let append_space = get_append_trailing_space(app);
    let restore = get_restore_clipboard(app);
    let final_text = if append_space {
        format!("{text} ")
    } else {
        text.clone()
    };

    let final_text_clone = final_text.clone();
    task::spawn_blocking(move || paste_at_cursor(&final_text_clone, restore, None))
        .await
        .map_err(|e| anyhow!("task join: {e}"))??;

    let _ = app.emit(
        "pipeline:state",
        PipelineEvent {
            state: PipelineState::Done,
            message: None,
            text: Some(text),
            duration_ms: Some(duration_ms),
        },
    );
    Ok(())
}
