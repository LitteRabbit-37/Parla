// Recording feedback cues (start / stop / cancel).
//
// Reference VoiceInk : SoundManager.swift. Same three cues, same reduced
// volumes (0.4 / 0.4 / 0.3). VoiceInk uses AVAudioPlayer ; we use rodio on
// top of the cpal output device. The sounds are embedded in the binary so
// there is no resource path to resolve at runtime.
//
// Trigger sites (see commands/recording.rs) :
// - Start  : top of start_recording_core_with_chunk, before capture starts,
//   so the cue does not leak into the microphone. On the hotkey path the
//   system mute is deliberately deferred (audio/mute.rs) so the speakers are
//   still audible when this plays.
// - Stop   : stop_recording_core, after the recording is finalized.
// - Cancel : cancel_recording_core (the ESC sound).

use std::io::Cursor;

use tauri::AppHandle;
use tauri_plugin_store::StoreExt;
use tracing::warn;

const STORE_FILE: &str = "parla.settings.json";
const KEY_ENABLED: &str = "sound_feedback_enabled";

// Cues reused from VoiceInk (GPL-3.0), embedded at build time.
const START_BYTES: &[u8] = include_bytes!("../../sounds/recstart.mp3");
const STOP_BYTES: &[u8] = include_bytes!("../../sounds/recstop.mp3");
const CANCEL_BYTES: &[u8] = include_bytes!("../../sounds/esc.wav");

#[derive(Clone, Copy, Debug)]
pub enum Cue {
    Start,
    Stop,
    Cancel,
}

impl Cue {
    /// Embedded audio bytes + playback volume for this cue. Volumes match
    /// VoiceInk's SoundManager (start/stop 0.4, esc 0.3).
    fn payload(self) -> (&'static [u8], f32) {
        match self {
            Cue::Start => (START_BYTES, 0.4),
            Cue::Stop => (STOP_BYTES, 0.4),
            Cue::Cancel => (CANCEL_BYTES, 0.3),
        }
    }
}

/// Whether the sound-feedback feature is enabled. Default: true (VoiceInk
/// ships it on). Mirrors audio::mute::is_enabled.
pub fn is_enabled(app: &AppHandle) -> bool {
    app.store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(KEY_ENABLED).and_then(|v| v.as_bool()))
        .unwrap_or(true)
}

pub fn set_enabled(app: &AppHandle, enabled: bool) -> anyhow::Result<()> {
    let store = app
        .store(STORE_FILE)
        .map_err(|e| anyhow::anyhow!("store: {e}"))?;
    store.set(KEY_ENABLED, serde_json::Value::Bool(enabled));
    store.save().map_err(|e| anyhow::anyhow!("store save: {e}"))
}

/// Play a cue if the feature is enabled. Fire-and-forget : decoding and
/// playback run on a detached thread so the recording path is never blocked.
/// A failure (no output device, decode error) is logged, never fatal.
pub fn play(app: &AppHandle, cue: Cue) {
    if !is_enabled(app) {
        return;
    }
    let (bytes, volume) = cue.payload();
    std::thread::spawn(move || {
        if let Err(e) = play_blocking(bytes, volume) {
            warn!(error = %e, ?cue, "sound feedback: playback failed");
        }
    });
}

fn play_blocking(bytes: &'static [u8], volume: f32) -> anyhow::Result<()> {
    use rodio::{Decoder, OutputStream, Sink};

    // Open the default output device. `_stream` must stay alive until the
    // cue has finished playing (sleep_until_end below).
    let (_stream, handle) =
        OutputStream::try_default().map_err(|e| anyhow::anyhow!("output stream: {e}"))?;
    let sink = Sink::try_new(&handle).map_err(|e| anyhow::anyhow!("sink: {e}"))?;
    let source = Decoder::new(Cursor::new(bytes)).map_err(|e| anyhow::anyhow!("decode: {e}"))?;
    sink.set_volume(volume);
    sink.append(source);
    sink.sleep_until_end();
    Ok(())
}
