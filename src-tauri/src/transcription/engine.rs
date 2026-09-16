// Orchestrateur de cycle enregistrement <-> transcription <-> paste.
//
// Reference VoiceInk : VoiceInk/Transcription/Core/VoiceInkEngine.swift.
// Cette fonction porte la logique metier d'un cycle complet declenche par
// un HotkeyAction (Start/Stop/Cancel/EnterHandsFree). Anciennement dans
// lib.rs::handle_action, extraite ici pour alignement architectural avec
// VoiceInk et pour faciliter les tests d'integration sans le runtime Tauri
// complet.
//
// Responsabilites :
// - Emettre les events "hotkey:action" et "power_mode:active".
// - Preparer le contexte au start : remember_foreground (paste cible),
//   ensure_open de la mini-recorder (UI), begin_session PowerMode,
//   capture ecran + OCR (background).
// - Choisir le chemin streaming cloud vs recording local selon la source.
// - Au stop : basculer vers pipeline::finalize_streaming_session (si
//   streaming) ou pipeline::run_after_recording (WAV sur disque).
// - Au cancel : purger la session streaming, stop audio, end_session
//   PowerMode, fermer la mini-recorder.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};
use tracing::warn;

use crate::audio::mute as system_mute;
use crate::commands::hotkey::HotkeyManagerState;
use crate::commands::recording::{
    cancel_recording_core, start_recording_core, start_recording_core_with_chunk,
    stop_recording_core, RecorderState,
};
use crate::commands::streaming::StreamingSessionState;
use crate::history::last_transcription;
use crate::hotkeys::keyboard_hook::{self, UtilityAction};
use crate::hotkeys::manager::{HotkeyAction, HotkeyManager};
use crate::mini_recorder;
use crate::paste;
use crate::power_mode;
use crate::screen_context;
use crate::transcription::pipeline;

/// Dispatch une action hotkey sur le cycle enregistrement complet.
/// A appeler depuis la boucle dispatch_loop du hotkey manager.
pub fn handle_hotkey_action(app: &AppHandle, manager: &Arc<HotkeyManager>, action: HotkeyAction) {
    let state = app.state::<RecorderState>();
    match action {
        HotkeyAction::StartRecording => {
            start(app, manager, &state);
        }
        HotkeyAction::StopRecording => stop(app, manager, &state),
        HotkeyAction::CancelRecording => cancel(app, manager, &state),
        HotkeyAction::EnterHandsFree => {
            let _ = app.emit("hotkey:action", "hands-free");
        }
        HotkeyAction::SelectPowerMode(index) => select_power_mode(app, index),
        HotkeyAction::EscapeHint => escape_hint(app),
        HotkeyAction::Utility(action) => run_utility(app, action),
    }
}

/// Bascule enregistrement depuis une UI (menu tray "Toggle Recorder") :
/// meme cycle complet que le raccourci (mini-recorder, Power Mode, capture
/// ecran, mute), puis la machine a etats est informee qu'une session
/// hands-free a demarre pour que la prochaine pression du raccourci
/// l'arrete. Reference VoiceInk MenuBarView "Toggle Recorder" ->
/// RecorderUIManager.handleToggleRecorderPanelNotification.
pub fn toggle_from_ui(app: &AppHandle) {
    let Some(mgr) = app.try_state::<HotkeyManagerState>() else {
        warn!("toggle_from_ui: hotkey manager absent");
        return;
    };
    let manager = mgr.0.clone();
    let state = app.state::<RecorderState>();
    let recording = state.0.lock().is_some();
    if recording {
        stop(app, &manager, &state);
    } else if start(app, &manager, &state) {
        manager.mark_started_externally();
    }
}

/// Premier Echap pendant un enregistrement : affiche une seule fois
/// l'astuce "Appuyez encore sur Echap pour annuler" dans la mini-recorder.
/// Reference VoiceInk RecorderPanelShortcutManager
/// .showEscapeConfirmationHintIfNeeded (cle
/// hasShownEscapeCancelConfirmationHint).
fn escape_hint(app: &AppHandle) {
    use tauri_plugin_store::StoreExt;
    let Ok(store) = app.store("parla.settings.json") else {
        return;
    };
    let shown = store
        .get(ESCAPE_HINT_KEY)
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if shown {
        return;
    }
    store.set(ESCAPE_HINT_KEY, serde_json::Value::Bool(true));
    let _ = store.save();
    let _ = app.emit("recorder:escape-hint", ());
}

/// Cle store de l'astuce Echap (reinitialisee par le bouton "reset" du
/// raccourci d'annulation dans les reglages).
pub const ESCAPE_HINT_KEY: &str = "escape_cancel_hint_shown";

/// Raccourcis utilitaires globaux (VoiceInk RecordingShortcutManager
/// .handleGlobalShortcut).
fn run_utility(app: &AppHandle, action: UtilityAction) {
    let result = match action {
        UtilityAction::CopyLastTranscription => last_transcription::copy_last(app),
        UtilityAction::PasteLastTranscription => last_transcription::paste_last(app, false),
        UtilityAction::PasteLastEnhancement => last_transcription::paste_last(app, true),
        UtilityAction::RetryLastTranscription => last_transcription::retry_last(app),
        UtilityAction::OpenHistory => {
            last_transcription::open_history(app);
            Ok(())
        }
    };
    if let Err(e) = result {
        warn!(?action, error = %e, "utility shortcut failed");
        let _ = app.emit("tray:notice", format!("{action:?} failed: {e}"));
    }
}

/// Applique manuellement le Nieme profil Power Mode active (raccourci
/// Alt+chiffre pendant l'enregistrement) et notifie l'UI.
fn select_power_mode(app: &AppHandle, index: usize) {
    if let Some(session) = power_mode::session::select_by_index(app, index) {
        let _ = app.emit("power_mode:active", &session);
    }
}

/// Demarre un cycle d'enregistrement. Retourne `true` si la capture (ou la
/// session streaming) a effectivement demarre.
fn start(app: &AppHandle, manager: &Arc<HotkeyManager>, state: &tauri::State<RecorderState>) -> bool {
    let _ = app.emit("hotkey:action", "start");
    // Sauvegarde le HWND de l'app actuellement focus AVANT d'ouvrir
    // la mini-recorder (qui peut voler le focus). On le restaure
    // juste avant Ctrl+V au moment du paste.
    paste::remember_foreground();
    // Ouvre la fenetre mini-recorder EN PREMIER pour que le webview
    // ait le temps de charger pendant l'init du recorder audio. Sans
    // ca, la pill n'apparait qu'au stop (le HTML/JS prend ~500ms a
    // initialiser).
    mini_recorder::ensure_open(app);
    // Power Mode : detection fenetre + URL, snapshot baseline et
    // application de la config matchante. VoiceInk fait ca juste
    // apres recorder.startRecording (VoiceInkEngine.swift:152).
    // On le fait avant, pour que les settings soient en place au
    // moment ou le pipeline choisit la source de transcription.
    if let Some(session) = power_mode::session::begin_session(app) {
        let _ = app.emit("power_mode:active", &session);
    }
    // Arme les raccourcis Alt+chiffre de selection Power Mode pour la duree
    // de l'enregistrement (VoiceInk : shortcuts actifs tant que le recorder
    // est visible). count = nombre de profils actives adressables ; 0 laisse
    // les touches Alt+chiffre passer normalement a l'app cible.
    keyboard_hook::set_power_shortcut_count(power_mode::session::enabled_configs(app).len());
    // Capture + OCR de la fenetre active (background). Remplit
    // le cache consomme par l'enhancement service pour le bloc
    // <CURRENT_WINDOW_CONTEXT>. No-op si la feature est desactivee.
    screen_context::service::trigger_capture(app);
    // Mute the default audio output so notifications / videos / calls
    // don't bleed into the dictation (VoiceInk MediaController). No-op
    // if the feature is disabled. Restored in stop() / cancel().
    system_mute::engage(app);
    // Determine si on utilise le streaming cloud.
    let started = if let Some((provider, model)) = pipeline::active_streaming_target(app) {
        let app_bg = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = start_with_streaming(app_bg.clone(), provider, model).await {
                warn!("start streaming: {e}");
            }
        });
        manager.mark_recording_state(true);
        true
    } else if let Err(e) = start_recording_core(app, state, None) {
        warn!("start_recording via hotkey: {e}");
        // L'enregistrement n'a pas demarre : desarme les raccourcis Power Mode.
        keyboard_hook::set_power_shortcut_count(0);
        manager.mark_recording_state(false);
        mini_recorder::close(app);
        false
    } else {
        manager.mark_recording_state(true);
        true
    };
    // Arme le raccourci d'annulation personnalise (hook) pour la duree de
    // l'enregistrement (VoiceInk : monitor actif tant que le recorder est
    // visible) et rafraichit le menu tray (mode actif).
    keyboard_hook::set_recording_active(started);
    crate::tray::refresh(app);
    started
}

fn stop(app: &AppHandle, manager: &Arc<HotkeyManager>, state: &tauri::State<RecorderState>) {
    let _ = app.emit("hotkey:action", "stop");
    // Restore system audio (respects audio_resumption_delay_secs).
    system_mute::release(app);
    // Streaming actif ?
    let streaming = app.state::<StreamingSessionState>();
    let streaming_handle = streaming.0.lock().take();
    match stop_recording_core(app, state) {
        Ok(stopped) => {
            if let Some(handle) = streaming_handle {
                let app_bg = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) =
                        pipeline::finalize_streaming_session(app_bg, handle).await
                    {
                        warn!("finalize streaming: {e}");
                    }
                });
            } else {
                let wav_path = PathBuf::from(&stopped.wav_path);
                pipeline::run_after_recording(app.clone(), wav_path);
            }
        }
        Err(e) => warn!("stop_recording via hotkey: {e}"),
    }
    // Fin d'enregistrement : desarme les raccourcis Alt+chiffre Power Mode
    // et le raccourci d'annulation personnalise.
    keyboard_hook::set_power_shortcut_count(0);
    keyboard_hook::set_recording_active(false);
    manager.mark_recording_state(false);
}

fn cancel(app: &AppHandle, manager: &Arc<HotkeyManager>, state: &tauri::State<RecorderState>) {
    let _ = app.emit("hotkey:action", "cancel");
    system_mute::release(app);
    // Purge eventuelle session streaming (elle retournera un final
    // mais on l'ignore puisque le user a annule).
    let streaming = app.state::<StreamingSessionState>();
    let _ = streaming.0.lock().take();
    if let Err(e) = cancel_recording_core(app, state) {
        warn!("cancel_recording via hotkey: {e}");
    }
    power_mode::session::end_session(app);
    let _ = app.emit("power_mode:active", serde_json::Value::Null);
    // Annulation : desarme les raccourcis Alt+chiffre Power Mode et le
    // raccourci d'annulation personnalise.
    keyboard_hook::set_power_shortcut_count(0);
    keyboard_hook::set_recording_active(false);
    manager.mark_recording_state(false);
    mini_recorder::close(app);
    crate::tray::refresh(app);
}

/// Demarre simultanement la session WebSocket streaming et le recorder audio.
/// Le callback on_chunk capture directement l'UnboundedSender du handle
/// (clone) pour eviter le double-lookup state + mutex a chaque chunk audio
/// (hot-path a 30-100 Hz).
async fn start_with_streaming(
    app: AppHandle,
    provider: String,
    model: String,
) -> anyhow::Result<()> {
    let language = pipeline::get_language(&app);
    let handle = pipeline::start_streaming_session(
        app.clone(),
        provider.clone(),
        model.clone(),
        language,
    )
    .await?;

    // Ligne history pending (avant le start recording pour que l'UI puisse
    // deja la voir).
    pipeline::insert_pending_streaming_row(&app, &provider, &model);

    // Capture le sender avant de deposer le handle : le callback ne touche
    // jamais le state Tauri.
    let audio_tx = handle.audio_sender();
    let on_chunk: std::sync::Arc<dyn Fn(Vec<i16>) + Send + Sync> =
        std::sync::Arc::new(move |chunk: Vec<i16>| {
            let _ = audio_tx.send(chunk);
        });

    // Le handle vit dans le state pour stop/cancel.
    let state = app.state::<StreamingSessionState>();
    *state.0.lock() = Some(handle);

    let recorder_state = app.state::<RecorderState>();
    start_recording_core_with_chunk(&app, &recorder_state, None, Some(on_chunk))?;
    Ok(())
}
