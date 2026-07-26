// Parla - entree lib.
// Setup Tauri (tray, GPU info, audio devices, hotkeys, pipeline de
// transcription, enhancement, Power Mode, capture ecran, historique).

mod audio;
mod commands;
mod db;
mod enhancement;
mod gpu;
mod history;
mod hotkeys;
mod mini_recorder;
mod paste;
mod power_mode;
mod screen_context;
mod services;
mod text_processing;
mod transcription;
mod tray;
#[cfg(windows)]
mod window_subclass;

use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tracing::{info, warn};
use tracing_subscriber::prelude::*;

use commands::cloud::{
    cloud_transcribe_wav, delete_api_key, has_api_key, list_cloud_models, list_cloud_providers,
    set_api_key, verify_api_key, CloudRegistryState,
};
use commands::dictionary::{
    add_word_replacement, delete_word_replacement, list_word_replacements, update_word_replacement,
};
use commands::enhancement::{
    add_prompt, delete_prompt, get_active_prompt_id, get_custom_base_url, get_enhancement_enabled,
    get_llm_selection, get_localcli_custom_cmd, get_localcli_timeout_secs, get_ollama_base_url,
    list_extra_templates, list_llm_providers, list_ollama_models, list_prompts,
    set_active_prompt_id, set_custom_base_url, set_enhancement_enabled, set_llm_selection,
    set_localcli_custom_cmd, set_localcli_timeout_secs, set_ollama_base_url, update_prompt,
};
use commands::history::{
    count_history, delete_history_item, export_history_csv, get_history_item,
    get_retention_settings, list_history, run_history_cleanup, set_retention_settings,
};
use commands::hotkey::{
    get_hotkey_config, list_hotkey_options, reset_hotkey_config, set_hotkey_config,
    HotkeyManagerState,
};
use commands::llm_models::{
    cancel_download_gguf_model, delete_gguf_model, download_gguf_model, get_llamacpp_settings,
    get_selected_gguf, import_gguf_model, list_gguf_models, llamacpp_cuda_enabled,
    set_llamacpp_settings, set_selected_gguf,
};
use commands::metrics::get_model_performance_metrics;
use commands::models::{
    cancel_download_whisper_model, delete_whisper_model, download_whisper_model,
    import_whisper_model, list_whisper_models, ModelManagerState,
};
use commands::parakeet::{
    cancel_download_parakeet_model, delete_parakeet_model, download_parakeet_model,
    list_parakeet_models, parakeet_execution_provider,
};
use commands::permissions::{
    check_permissions, get_onboarding_completed, get_recorder_style, open_language_settings,
    open_privacy_microphone, set_autostart_enabled, set_onboarding_completed, set_recorder_style,
};
use commands::power_mode::{
    add_power_config, delete_power_config, get_active_power_session, get_power_auto_restore,
    list_power_configs, power_mode_preview, reorder_power_configs, set_power_auto_restore,
    update_power_config,
};
use commands::recording::{
    cancel_recording, get_audio_meter, is_recording, list_audio_devices, start_recording,
    stop_recording, RecorderState,
};
use commands::screen_context::{
    capture_screen_context_preview, clear_screen_context, get_screen_context_cached,
    get_screen_context_enabled, set_screen_context_enabled,
};
use commands::settings::{
    close_to_tray_enabled, delete_proxy_credentials, get_audio_resumption_delay, get_close_to_tray,
    get_dictation_language, get_proxy_settings, get_selected_whisper_model,
    get_sound_feedback_enabled, get_system_mute_enabled, get_text_processing_settings,
    get_transcription_source, has_proxy_credentials, set_append_trailing_space,
    set_audio_resumption_delay, set_close_to_tray, set_dictation_language, set_filler_words,
    set_proxy_credentials, set_proxy_settings, set_remove_filler_words,
    set_restore_clipboard_after_paste, set_selected_whisper_model, set_sound_feedback_enabled,
    set_system_mute_enabled, set_text_formatting_enabled, set_transcription_kind,
    set_transcription_source,
};
use commands::streaming::{StreamingRegistryState, StreamingSessionState};
use commands::transcription::{transcribe_wav, WhisperEngineState};
use commands::vad::{
    vad_delete, vad_download, vad_get_state, vad_is_enabled, vad_is_ready, vad_set_enabled,
    VadEngineState,
};
use hotkeys::{
    keyboard_hook::install_hook,
    manager::{dispatch_loop, HotkeyManager},
};

/// Info GPU exposee au frontend.
#[derive(serde::Serialize, Clone)]
pub struct GpuInfo {
    pub has_nvidia: bool,
    pub device_name: Option<String>,
    pub driver_version: Option<String>,
    pub cuda_version: Option<String>,
}

#[tauri::command]
fn get_gpu_info() -> GpuInfo {
    gpu::detect()
}

#[tauri::command]
fn ping() -> &'static str {
    "pong"
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Single-instance guard. If the user double-clicks the exe or an
        // autostart entry fires a second time while Parla is already
        // running, this plugin kicks the second instance out and tells the
        // first one to surface its main window. Prevents the "two running
        // parla.exe fight over the global hotkey hook" bug (WH_KEYBOARD_LL
        // is process-wide but Windows only lets one low-level hook "win"
        // per key event - the later hook silently swallows the event and
        // nothing fires in either process).
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.unminimize();
                let _ = win.set_focus();
            }
        }))
        .manage(RecorderState::default())
        .manage(WhisperEngineState::default())
        .manage(VadEngineState::default())
        .manage(CloudRegistryState::default())
        .manage(StreamingRegistryState::default())
        .manage(StreamingSessionState::default())
        .manage(enhancement::service::EnhancementState::default())
        .manage(transcription::parakeet::ParakeetEngineState::default())
        .manage(power_mode::session::PowerSessionState::default())
        .manage(screen_context::service::ScreenContextState::default())
        .manage(transcription::pipeline::HistorySessionState::default())
        // GgufModelManagerState + ParakeetModelManagerState sont enregistres
        // dans le setup() ci-dessous car ils ont besoin de l'AppHandle pour
        // resoudre AppLocalData.
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_store::Builder::new().build())
        // Argument `--minimized` ecrit dans l'entree Run du registry Windows
        // par le plugin quand l'utilisateur active l'autostart. Le setup ci-
        // dessous detecte cet argument pour demarrer Parla en tray-only (hook
        // clavier actif, fenetre cachee) plutot que d'afficher la fenetre
        // principale a chaque demarrage de Windows.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(move |app| {
            configure_tracing(app.handle());
            let gpu = gpu::detect();
            if gpu.has_nvidia {
                info!(
                    device = gpu.device_name.as_deref().unwrap_or("?"),
                    driver = gpu.driver_version.as_deref().unwrap_or("?"),
                    cuda = gpu.cuda_version.as_deref().unwrap_or("?"),
                    "GPU NVIDIA detecte"
                );
            } else {
                warn!("Pas de GPU NVIDIA detecte, execution CPU uniquement");
            }

            if let Err(e) = services::proxy::configure(app.handle()) {
                warn!("Application proxy unavailable: {e}");
            }
            let handle = app.handle().clone();
            app.manage(ModelManagerState::new(handle.clone()));
            app.manage(enhancement::model_manager::GgufModelManagerState::new(
                handle.clone(),
            ));
            app.manage(
                transcription::parakeet_model_manager::ParakeetModelManagerState::new(
                    handle.clone(),
                ),
            );
            match db::Database::open(app.handle()) {
                Ok(db) => {
                    app.manage(db);
                    // Cleanup quotidien de l'historique (orphan sweep +
                    // prune time-based si activee).
                    history::cleanup::spawn_daily_timer(app.handle().clone());
                }
                Err(e) => warn!("Ouverture DB SQLite echec: {e}"),
            }
            tray::setup(app.handle())?;
            // Subclasse WndProc de la main window pour neutraliser le menu
            // systeme Alt+Space (cf window_subclass.rs). Sans ca, Parla
            // intercepte Alt+Space pendant qu'il est focus et les launchers
            // globaux (Raycast, PowerToys Run) ne s'ouvrent plus.
            #[cfg(windows)]
            if let Some(main) = app.get_webview_window("main") {
                if let Ok(hwnd) = main.hwnd() {
                    window_subclass::install_main_window_subclass(hwnd.0 as isize);
                }
            }
            // La main window est creee avec `visible: false` dans tauri.conf.json
            // pour eviter le flash blanc au boot autostart. On l'affiche ici
            // sauf si l'autostart a passe `--minimized` : dans ce cas Parla
            // reste en tray-only et l'utilisateur cliquera sur l'icone tray
            // ou le menu "Open Parla" pour faire apparaitre la fenetre.
            let started_minimized = std::env::args().any(|arg| arg == "--minimized");
            if !started_minimized {
                if let Some(main) = app.get_webview_window("main") {
                    let _ = main.show();
                    let _ = main.set_focus();
                }
            }
            // Migration 0.3.2 : avant cette version, l'entree autostart du
            // registry Windows ne contenait pas l'argument `--minimized`.
            // Les utilisateurs qui avaient deja active l'autostart en 0.3.1
            // continueraient donc a voir la fenetre s'ouvrir au demarrage,
            // meme apres l'update. On re-ecrit l'entree une seule fois
            // (disable + enable) pour qu'elle inclue le flag.
            {
                use tauri_plugin_autostart::ManagerExt;
                use tauri_plugin_store::StoreExt;
                const MIGRATION_KEY: &str = "autostart_v032_migrated";
                if let Ok(store) = app.store("parla.settings.json") {
                    let already_done = store
                        .get(MIGRATION_KEY)
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if !already_done {
                        let mgr = app.autolaunch();
                        if matches!(mgr.is_enabled(), Ok(true)) {
                            let _ = mgr.disable();
                            if let Err(e) = mgr.enable() {
                                warn!("Re-write autostart entry failed: {e}");
                            } else {
                                info!("Autostart entry refreshed with --minimized flag");
                            }
                        }
                        store.set(MIGRATION_KEY, serde_json::Value::Bool(true));
                        let _ = store.save();
                    }
                }
            }
            setup_hotkeys(handle);
            Ok(())
        })
        .on_window_event(|window, event| {
            // Close-to-tray : when the user clicks the X on the main window,
            // we hide it instead of quitting. The tray stays active with a
            // "Quit Parla" menu entry for actual exit. The behavior can be
            // disabled from Settings via the `close_to_tray` store key
            // (default: true). The recorder window is always real-closed
            // (it manages its own lifecycle via mini_recorder::close).
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() != "main" {
                    return;
                }
                let app = window.app_handle();
                if close_to_tray_enabled(app) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            get_gpu_info,
            list_audio_devices,
            start_recording,
            stop_recording,
            cancel_recording,
            get_audio_meter,
            is_recording,
            list_whisper_models,
            download_whisper_model,
            cancel_download_whisper_model,
            delete_whisper_model,
            import_whisper_model,
            transcribe_wav,
            set_selected_whisper_model,
            get_selected_whisper_model,
            get_dictation_language,
            set_dictation_language,
            get_text_processing_settings,
            set_text_formatting_enabled,
            set_remove_filler_words,
            set_filler_words,
            set_append_trailing_space,
            set_restore_clipboard_after_paste,
            get_close_to_tray,
            set_close_to_tray,
            get_system_mute_enabled,
            set_system_mute_enabled,
            get_audio_resumption_delay,
            set_audio_resumption_delay,
            get_sound_feedback_enabled,
            set_sound_feedback_enabled,
            mini_recorder::resize_recorder_window,
            mini_recorder::show_main_window,
            list_word_replacements,
            add_word_replacement,
            update_word_replacement,
            delete_word_replacement,
            vad_get_state,
            vad_download,
            vad_delete,
            vad_is_enabled,
            vad_set_enabled,
            vad_is_ready,
            get_proxy_settings,
            set_proxy_settings,
            set_proxy_credentials,
            delete_proxy_credentials,
            has_proxy_credentials,
            list_cloud_providers,
            list_cloud_models,
            set_api_key,
            delete_api_key,
            has_api_key,
            verify_api_key,
            cloud_transcribe_wav,
            get_transcription_source,
            set_transcription_source,
            set_transcription_kind,
            get_enhancement_enabled,
            set_enhancement_enabled,
            list_prompts,
            add_prompt,
            update_prompt,
            delete_prompt,
            get_active_prompt_id,
            set_active_prompt_id,
            list_extra_templates,
            list_llm_providers,
            get_llm_selection,
            set_llm_selection,
            get_ollama_base_url,
            set_ollama_base_url,
            list_ollama_models,
            get_custom_base_url,
            set_custom_base_url,
            get_localcli_custom_cmd,
            set_localcli_custom_cmd,
            get_localcli_timeout_secs,
            set_localcli_timeout_secs,
            list_gguf_models,
            download_gguf_model,
            cancel_download_gguf_model,
            delete_gguf_model,
            import_gguf_model,
            get_selected_gguf,
            set_selected_gguf,
            get_llamacpp_settings,
            set_llamacpp_settings,
            llamacpp_cuda_enabled,
            list_parakeet_models,
            download_parakeet_model,
            cancel_download_parakeet_model,
            delete_parakeet_model,
            parakeet_execution_provider,
            list_power_configs,
            add_power_config,
            update_power_config,
            delete_power_config,
            reorder_power_configs,
            get_power_auto_restore,
            set_power_auto_restore,
            get_active_power_session,
            power_mode_preview,
            get_screen_context_enabled,
            set_screen_context_enabled,
            get_screen_context_cached,
            clear_screen_context,
            capture_screen_context_preview,
            list_history,
            get_history_item,
            delete_history_item,
            count_history,
            export_history_csv,
            get_retention_settings,
            set_retention_settings,
            run_history_cleanup,
            get_model_performance_metrics,
            check_permissions,
            set_autostart_enabled,
            open_privacy_microphone,
            open_language_settings,
            get_recorder_style,
            set_recorder_style,
            get_onboarding_completed,
            set_onboarding_completed,
            get_hotkey_config,
            set_hotkey_config,
            reset_hotkey_config,
            list_hotkey_options,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn configure_tracing(app: &tauri::AppHandle) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,parla=debug"));

    #[cfg(windows)]
    {
        if let Ok(log_dir) = app
            .path()
            .app_local_data_dir()
            .map(|path| path.join("logs"))
        {
            if let Err(error) = std::fs::create_dir_all(&log_dir) {
                eprintln!(
                    "Unable to create Parla log directory {}: {error}",
                    log_dir.display()
                );
            } else {
                let (file_writer, guard) = tracing_appender::non_blocking(
                    tracing_appender::rolling::daily(&log_dir, "parla.log"),
                );
                // Keep the worker alive for process lifetime; dropping it stops file logging.
                let _ = Box::leak(Box::new(guard));
                let _ = tracing_subscriber::registry()
                    .with(filter.clone())
                    .with(tracing_subscriber::fmt::layer())
                    .with(tracing_subscriber::fmt::layer().with_writer(file_writer))
                    .try_init();
                info!(
                    path = %log_dir.display(),
                    "Durable tracing log enabled"
                );
                return;
            }
        }
    }

    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn setup_hotkeys(app: AppHandle) {
    let cfg = commands::hotkey::load(&app);
    let manager = Arc::new(HotkeyManager::with_modes(
        cfg.primary.mode,
        cfg.secondary.mode,
    ));
    let rx = install_hook(cfg.primary.trigger, cfg.secondary.trigger);

    info!(
        primary = ?cfg.primary.trigger,
        primary_mode = ?cfg.primary.mode,
        secondary = ?cfg.secondary.trigger,
        secondary_mode = ?cfg.secondary.mode,
        "Hook clavier bas niveau installe"
    );

    // Partage le manager avec les commandes Tauri pour pouvoir update les
    // modes a chaud quand l'utilisateur change la config dans Settings.
    app.manage(HotkeyManagerState(manager.clone()));

    let app_for_dispatch = app.clone();
    let manager_for_dispatch = manager.clone();
    std::thread::Builder::new()
        .name("parla-hotkey-dispatch".into())
        .spawn(move || {
            dispatch_loop(rx, manager_for_dispatch.clone(), move |action| {
                transcription::engine::handle_hotkey_action(
                    &app_for_dispatch,
                    &manager_for_dispatch,
                    action,
                );
            });
        })
        .expect("thread dispatch hotkey");
}
