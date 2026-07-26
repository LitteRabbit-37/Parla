// Commandes Tauri exposees au frontend pour la capture audio.
//
// Reference VoiceInk : VoiceInk/Recorder.swift + VoiceInk/VoiceInkEngine.swift
// - VoiceInk nomme le WAV uuid.uuidString dans ~/Library/.../Recordings/
// - Nous utilisons AppLocalData/Recordings/{uuid}.wav

use std::path::PathBuf;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tracing::{info, warn};
use uuid::Uuid;

use crate::audio::feedback::{self, Cue};
use crate::audio::{
    list_input_devices, recorder::ChunkCallback, AudioDeviceInfo, AudioMeter, AudioRecorder,
    RecorderConfig, RecorderHandle,
};

/// Etat partage du recorder courant.
#[derive(Default)]
pub struct RecorderState(pub Mutex<Option<RecorderHandle>>);

#[derive(Debug, Serialize, Clone)]
pub struct RecordingStarted {
    pub recording_id: String,
    pub wav_path: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct RecordingStopped {
    pub wav_path: String,
}

#[tauri::command]
pub fn list_audio_devices() -> Vec<AudioDeviceInfo> {
    list_input_devices()
}

#[tauri::command]
pub fn start_recording(
    app: AppHandle,
    state: State<'_, RecorderState>,
    device_name: Option<String>,
) -> Result<RecordingStarted, String> {
    start_recording_core(&app, &state, device_name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn stop_recording(
    app: AppHandle,
    state: State<'_, RecorderState>,
    #[allow(non_snake_case)] runPipeline: Option<bool>,
) -> Result<RecordingStopped, String> {
    let stopped = stop_recording_core(&app, &state).map_err(|e| e.to_string())?;
    if runPipeline.unwrap_or(true) {
        crate::transcription::pipeline::run_after_recording(
            app.clone(),
            std::path::PathBuf::from(&stopped.wav_path),
        );
    }
    Ok(stopped)
}

#[tauri::command]
pub fn cancel_recording(app: AppHandle, state: State<'_, RecorderState>) -> Result<(), String> {
    cancel_recording_core(&app, &state).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_audio_meter(state: State<'_, RecorderState>) -> AudioMeter {
    state
        .0
        .lock()
        .as_ref()
        .map(|h| h.current_meter())
        .unwrap_or_default()
}

#[tauri::command]
pub fn is_recording(state: State<'_, RecorderState>) -> bool {
    state.0.lock().is_some()
}

// ============================================================================
// Helpers reutilisables (hotkey manager, etc.)
// ============================================================================

pub fn start_recording_core(
    app: &AppHandle,
    state: &RecorderState,
    device_name: Option<String>,
) -> anyhow::Result<RecordingStarted> {
    start_recording_core_with_chunk(app, state, device_name, None)
}

/// Variante permettant de passer un callback qui recoit chaque chunk audio
/// Int16 16 kHz mono (utilise pour le streaming cloud).
pub fn start_recording_core_with_chunk(
    app: &AppHandle,
    state: &RecorderState,
    device_name: Option<String>,
    on_chunk: Option<ChunkCallback>,
) -> anyhow::Result<RecordingStarted> {
    let mut guard = state.0.lock();
    if guard.is_some() {
        anyhow::bail!("un enregistrement est deja en cours");
    }

    // Start cue, played before the microphone capture begins so it never
    // leaks into the recording (VoiceInk SoundManager.playStartSound). On the
    // hotkey path the system mute is deferred ~250ms (audio/mute.rs) so the
    // speakers are still audible here. Fire-and-forget, no-op if disabled.
    feedback::play(app, Cue::Start);

    let dir = recordings_dir(app)?;
    let id = Uuid::new_v4().to_string();
    let wav_path = dir.join(format!("{id}.wav"));

    let config = RecorderConfig {
        device_name,
        output_path: wav_path.clone(),
    };

    let handle = AudioRecorder::start(config, on_chunk)?;
    *guard = Some(handle);
    drop(guard);

    info!(recording_id = %id, path = %wav_path.display(), "Enregistrement demarre");

    let started = RecordingStarted {
        recording_id: id,
        wav_path: wav_path.to_string_lossy().into_owned(),
    };
    let _ = app.emit("recording:started", started.clone());
    Ok(started)
}

pub fn stop_recording_core(
    app: &AppHandle,
    state: &RecorderState,
) -> anyhow::Result<RecordingStopped> {
    let mut guard = state.0.lock();
    let Some(handle) = guard.take() else {
        anyhow::bail!("aucun enregistrement en cours");
    };
    drop(guard);
    let path = handle.stop()?;
    info!(path = %path.display(), "Enregistrement termine");
    // The stop cue is NOT played here. Like VoiceInk (playStopSound at the
    // paste site), it fires at the end of the pipeline once the text is
    // inserted at the cursor - see transcription/pipeline.rs finalize_text.
    // That also avoids the system-mute resumption-delay clipping the cue.
    let stopped = RecordingStopped {
        wav_path: path.to_string_lossy().into_owned(),
    };
    let _ = app.emit("recording:stopped", stopped.clone());
    Ok(stopped)
}

pub fn cancel_recording_core(app: &AppHandle, state: &RecorderState) -> anyhow::Result<()> {
    let mut guard = state.0.lock();
    let Some(handle) = guard.take() else {
        return Ok(());
    };
    drop(guard);
    let path = handle.stop()?;
    if let Err(e) = std::fs::remove_file(&path) {
        warn!(
            "Impossible de supprimer l'enregistrement annule {}: {e}",
            path.display()
        );
    }
    info!(path = %path.display(), "Enregistrement annule");
    // Cancel / ESC cue (VoiceInk SoundManager.playEscSound). Only when there
    // was an active recording to cancel (early return above otherwise).
    feedback::play(app, Cue::Cancel);
    let _ = app.emit("recording:cancelled", ());
    Ok(())
}

fn recordings_dir(app: &AppHandle) -> anyhow::Result<PathBuf> {
    let base = app
        .path()
        .app_local_data_dir()
        .map_err(|e| anyhow::anyhow!("app_local_data_dir: {e}"))?;
    let dir = base.join("Recordings");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
