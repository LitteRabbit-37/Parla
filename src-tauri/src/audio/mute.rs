// System audio mute during recording.
//
// Reference VoiceInk : MediaController.swift. Same state machine, Windows
// Core Audio API instead of Core Audio macOS. The default render endpoint
// (speakers / headphones) is muted when the user starts dictating so
// notifications / music / calls don't leak into the microphone. A
// configurable delay restores the previous state when recording stops.
//
// State machine :
// - engage()  : invoked on record start. If the user has the feature
//   enabled and the output is currently unmuted, mute it and remember
//   we are the ones who muted. If the output was already muted, respect
//   the user intent (do not un-mute later).
// - release() : invoked on record stop / cancel. Only un-mutes if WE
//   muted it, and only after the configured resumption delay. A new
//   engage() before the delay elapses cancels the pending release via
//   a generation counter (VoiceInk pattern).
//
// Toggling fails silently on devices without mute capability. Not fatal.

use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;
use tracing::{debug, warn};

const STORE_FILE: &str = "parla.settings.json";
const KEY_ENABLED: &str = "system_mute_enabled";
const KEY_DELAY: &str = "audio_resumption_delay_secs";

#[derive(Debug, Default)]
struct State {
    did_mute: bool,
    was_muted_before: bool,
    generation: u64,
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(State::default()))
}

pub fn is_enabled(app: &AppHandle) -> bool {
    app.store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(KEY_ENABLED).and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

pub fn set_enabled(app: &AppHandle, enabled: bool) -> Result<()> {
    let store = app.store(STORE_FILE).map_err(|e| anyhow!("store: {e}"))?;
    store.set(KEY_ENABLED, serde_json::Value::Bool(enabled));
    store.save().map_err(|e| anyhow!("store save: {e}"))
}

pub fn resumption_delay(app: &AppHandle) -> Duration {
    let secs = app
        .store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(KEY_DELAY).and_then(|v| v.as_f64()))
        .unwrap_or(0.2)
        .clamp(0.0, 10.0);
    Duration::from_secs_f64(secs)
}

pub fn set_resumption_delay(app: &AppHandle, secs: f64) -> Result<()> {
    let clamped = secs.clamp(0.0, 10.0);
    let store = app.store(STORE_FILE).map_err(|e| anyhow!("store: {e}"))?;
    store.set(
        KEY_DELAY,
        serde_json::Value::Number(
            serde_json::Number::from_f64(clamped).unwrap_or(serde_json::Number::from(0)),
        ),
    );
    store.save().map_err(|e| anyhow!("store save: {e}"))
}

/// Delay before the mute actually engages after recording starts (VoiceInk's
/// `scheduleSystemMute` idea). Kept short so the mute feels responsive : when
/// system mute is on, only the first ~300ms of the start cue is heard before
/// the speakers are muted. A tap-then-stop shorter than this never mutes.
const ENGAGE_DELAY: Duration = Duration::from_millis(300);

/// Engage system mute if the feature is enabled. Called on record start.
/// The actual mute is deferred by ENGAGE_DELAY and cancelled if a release()
/// (or a new engage()) bumps the generation while we wait.
pub fn engage(app: &AppHandle) {
    if !is_enabled(app) {
        return;
    }
    let my_generation = {
        let mut s = state().lock();
        s.generation = s.generation.wrapping_add(1);
        s.generation
    };

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(ENGAGE_DELAY).await;

        // Current mute state (COM read, no lock held).
        let currently_muted = match is_muted() {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "mute: cannot read current state, skipping");
                return;
            }
        };

        // Decide + act under a single lock so a concurrent release() can't be
        // overridden (which would leak a mute that never gets restored).
        let mut s = state().lock();
        if s.generation != my_generation {
            // Recording stopped (or restarted) during the delay - drop it.
            return;
        }

        if currently_muted {
            // Already muted. If we are the ones who muted it, keep the flag
            // so the next release un-mutes. Otherwise respect the user's
            // own mute : do NOT unmute when we release.
            if s.did_mute {
                s.was_muted_before = false;
            } else {
                s.was_muted_before = true;
                s.did_mute = false;
            }
            return;
        }

        s.was_muted_before = false;
        match set_muted(true) {
            Ok(()) => {
                s.did_mute = true;
                debug!("system audio muted for recording");
            }
            Err(e) => warn!(error = %e, "mute: set_muted(true) failed"),
        }
    });
}

/// Release system mute after the configured delay. Called on record stop
/// / cancel. Spawns a tokio task that sleeps, checks the generation is
/// still ours (no new engage happened), then un-mutes if we were the
/// ones who muted.
pub fn release(app: &AppHandle) {
    if !is_enabled(app) {
        return;
    }
    let delay = resumption_delay(app);
    let (should_unmute, my_generation) = {
        let mut s = state().lock();
        // Bump the generation so a deferred engage() still waiting out its
        // ENGAGE_DELAY sees a mismatch and aborts : the recording is over.
        s.generation = s.generation.wrapping_add(1);
        (s.did_mute && !s.was_muted_before, s.generation)
    };

    tauri::async_runtime::spawn(async move {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        let mut s = state().lock();
        if s.generation != my_generation {
            // A new record started while we were waiting - don't touch
            // the state, the new engage owns it now.
            return;
        }
        if should_unmute {
            if let Err(e) = set_muted(false) {
                warn!(error = %e, "unmute: set_muted(false) failed");
            } else {
                debug!("system audio restored");
            }
        }
        s.did_mute = false;
    });
}

#[cfg(windows)]
fn is_muted() -> Result<bool> {
    windows_audio::read_mute()
}

#[cfg(windows)]
fn set_muted(muted: bool) -> Result<()> {
    windows_audio::write_mute(muted)
}

#[cfg(not(windows))]
fn is_muted() -> Result<bool> {
    Ok(false)
}

#[cfg(not(windows))]
fn set_muted(_muted: bool) -> Result<()> {
    Err(anyhow!("mute not implemented on this platform"))
}

#[cfg(windows)]
mod windows_audio {
    use anyhow::{anyhow, Result};
    use windows::core::Interface;
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };

    /// COM guard - scoped CoInitialize / CoUninitialize. cpal and other
    /// libraries may already have initialized COM in apartment-threaded
    /// mode on this thread. CoInitializeEx with the same mode is fine
    /// (returns S_FALSE) so we do not need to track the init count.
    struct ComGuard;

    impl ComGuard {
        fn new() -> Result<Self> {
            let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            // S_FALSE = 0x00000001 means COM was already initialized on
            // this thread - that's OK. Any other error is fatal.
            if hr.is_err() {
                return Err(anyhow!("CoInitializeEx: {hr:?}"));
            }
            Ok(Self)
        }
    }

    impl Drop for ComGuard {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    fn endpoint_volume() -> Result<IAudioEndpointVolume> {
        let enumerator: IMMDeviceEnumerator = unsafe {
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| anyhow!("CoCreateInstance MMDeviceEnumerator: {e}"))?
        };
        let device = unsafe {
            enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| anyhow!("GetDefaultAudioEndpoint: {e}"))?
        };
        let volume: IAudioEndpointVolume = unsafe {
            device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| anyhow!("IMMDevice::Activate: {e}"))?
        };
        Ok(volume
            .cast()
            .map_err(|e| anyhow!("cast IAudioEndpointVolume: {e}"))?)
    }

    pub fn read_mute() -> Result<bool> {
        let _com = ComGuard::new()?;
        let vol = endpoint_volume()?;
        let muted = unsafe { vol.GetMute().map_err(|e| anyhow!("GetMute: {e}"))? };
        Ok(muted.as_bool())
    }

    pub fn write_mute(muted: bool) -> Result<()> {
        let _com = ComGuard::new()?;
        let vol = endpoint_volume()?;
        unsafe {
            vol.SetMute(muted, std::ptr::null())
                .map_err(|e| anyhow!("SetMute({muted}): {e}"))?;
        }
        Ok(())
    }
}
