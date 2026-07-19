// Gestion de session Power Mode : snapshot de l'etat global avant
// d'appliquer une config, puis restauration.
//
// Reference VoiceInk : PowerMode/PowerModeSessionManager.swift.
// On snapshot les reglages impactes par une config (enhancement, prompt
// actif, LLM provider/model, source transcription + sous-options, langue)
// avant d'overrider puis on restaure au stop si auto_restore est active.
//
// Stockage : en memoire (Mutex). Pas de persistance cross-process : en cas
// de crash, la baseline est perdue, mais les settings globaux precedents
// etaient deja sauvegardes puisqu'on les lit au snapshot time.

use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

use tauri::Manager;
use tracing::{info, warn};

use crate::enhancement::prompts as prompt_store;
use crate::enhancement::service as llm_service;

use super::active_window::foreground_window;
use super::browser_url::extract_url;
use super::config::{self, PowerModeConfig};
use super::matcher::resolve;

const SETTINGS_FILE: &str = "parla.settings.json";

/// Snapshot des reglages globaux affectes par Power Mode.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Baseline {
    pub enhancement_enabled: bool,
    pub use_screen_capture: bool,
    pub active_prompt_id: Option<String>,
    pub llm_provider_id: Option<String>,
    pub llm_model: Option<String>,
    pub transcription_kind: Option<String>,
    pub whisper_model_id: Option<String>,
    pub cloud_provider: Option<String>,
    pub cloud_model: Option<String>,
    pub parakeet_model_id: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PowerSession {
    pub config_id: String,
    pub config_name: String,
    pub emoji: String,
    pub baseline: Baseline,
}

#[derive(Default)]
pub struct PowerSessionState(pub Mutex<Option<PowerSession>>);

// -- Snapshot ---------------------------------------------------------------

pub fn snapshot(app: &AppHandle) -> Baseline {
    let store = app.store(SETTINGS_FILE).ok();

    let enhancement_enabled = prompt_store::is_enhancement_enabled(app);
    let use_screen_capture = crate::screen_context::service::is_enabled(app);
    let active_prompt_id = prompt_store::get_active_prompt_id(app);

    let sel = llm_service::get_selection(app);
    let (llm_provider_id, llm_model) = match sel {
        Some(s) => (Some(s.provider_id), Some(s.model)),
        None => (None, None),
    };

    let get_str = |k: &str| -> Option<String> {
        store
            .as_ref()
            .and_then(|s| s.get(k).and_then(|v| v.as_str().map(String::from)))
    };

    Baseline {
        enhancement_enabled,
        use_screen_capture,
        active_prompt_id,
        llm_provider_id,
        llm_model,
        transcription_kind: get_str("transcription_source_kind"),
        whisper_model_id: get_str("selected_whisper_model"),
        cloud_provider: get_str("selected_cloud_provider"),
        cloud_model: get_str("selected_cloud_model"),
        parakeet_model_id: get_str("selected_parakeet_model"),
        language: get_str("whisper_language"),
    }
}

// -- Apply ------------------------------------------------------------------

/// Applique les overrides de `cfg`. Ne touche pas aux champs None de la
/// config (laisse la baseline en place).
pub fn apply(app: &AppHandle, cfg: &PowerModeConfig) -> Result<()> {
    let store = app
        .store(SETTINGS_FILE)
        .map_err(|e| anyhow!("store: {e}"))?;

    prompt_store::set_enhancement_enabled(app, cfg.is_enhancement_enabled)?;

    if let Some(b) = cfg.use_screen_capture {
        crate::screen_context::service::set_enabled(app, b)?;
    }

    if let Some(pid) = &cfg.selected_prompt_id {
        prompt_store::set_active_prompt_id(app, Some(pid))?;
    }

    match (&cfg.selected_llm_provider, &cfg.selected_llm_model) {
        (Some(p), Some(m)) => {
            llm_service::set_selection(app, p, m)?;
        }
        (Some(p), None) => {
            llm_service::set_selection(app, p, "")?;
        }
        _ => {}
    }

    if let Some(kind) = &cfg.transcription_kind {
        store.set(
            "transcription_source_kind",
            serde_json::Value::String(kind.clone()),
        );
        match kind.as_str() {
            "local" => {
                if let Some(id) = &cfg.whisper_model_id {
                    store.set(
                        "selected_whisper_model",
                        serde_json::Value::String(id.clone()),
                    );
                }
            }
            "cloud" => {
                if let Some(p) = &cfg.cloud_provider {
                    store.set(
                        "selected_cloud_provider",
                        serde_json::Value::String(p.clone()),
                    );
                }
                if let Some(m) = &cfg.cloud_model {
                    store.set(
                        "selected_cloud_model",
                        serde_json::Value::String(m.clone()),
                    );
                }
            }
            "parakeet" => {
                if let Some(id) = &cfg.parakeet_model_id {
                    store.set(
                        "selected_parakeet_model",
                        serde_json::Value::String(id.clone()),
                    );
                }
            }
            _ => {}
        }
    }

    if let Some(lang) = &cfg.language {
        store.set("whisper_language", serde_json::Value::String(lang.clone()));
    }

    store.save().map_err(|e| anyhow!("store save: {e}"))?;
    crate::commands::settings::emit_source_changed(app);
    Ok(())
}

// -- Restore ----------------------------------------------------------------

pub fn restore(app: &AppHandle, b: &Baseline) -> Result<()> {
    let store = app
        .store(SETTINGS_FILE)
        .map_err(|e| anyhow!("store: {e}"))?;

    prompt_store::set_enhancement_enabled(app, b.enhancement_enabled)?;
    crate::screen_context::service::set_enabled(app, b.use_screen_capture)?;

    match &b.active_prompt_id {
        Some(id) => prompt_store::set_active_prompt_id(app, Some(id))?,
        None => prompt_store::set_active_prompt_id(app, None)?,
    }

    match (&b.llm_provider_id, &b.llm_model) {
        (Some(p), Some(m)) => llm_service::set_selection(app, p, m)?,
        (Some(p), None) => llm_service::set_selection(app, p, "")?,
        (None, _) => {}
    }

    set_or_delete(&store, "transcription_source_kind", b.transcription_kind.as_deref());
    set_or_delete(&store, "selected_whisper_model", b.whisper_model_id.as_deref());
    set_or_delete(&store, "selected_cloud_provider", b.cloud_provider.as_deref());
    set_or_delete(&store, "selected_cloud_model", b.cloud_model.as_deref());
    set_or_delete(&store, "selected_parakeet_model", b.parakeet_model_id.as_deref());
    set_or_delete(&store, "whisper_language", b.language.as_deref());

    store.save().map_err(|e| anyhow!("store save: {e}"))?;
    crate::commands::settings::emit_source_changed(app);
    Ok(())
}

fn set_or_delete(
    store: &std::sync::Arc<tauri_plugin_store::Store<tauri::Wry>>,
    key: &str,
    value: Option<&str>,
) {
    match value {
        Some(v) => store.set(key, serde_json::Value::String(v.to_string())),
        None => {
            store.delete(key);
        }
    }
}

// -- High-level API (called by the recording pipeline) ---------------------

/// Au demarrage d'un enregistrement : detecte la fenetre active + URL,
/// cherche la meilleure config, snapshot la baseline, applique la config.
/// Si aucune config ne matche : aucun effet.
pub fn begin_session(app: &AppHandle) -> Option<PowerSession> {
    let state = app.try_state::<PowerSessionState>()?;
    let configs = match config::load_all(app) {
        Ok(c) => c,
        Err(e) => {
            warn!("power_mode load: {e}");
            return None;
        }
    };
    if configs.is_empty() {
        return None;
    }

    let active = match foreground_window() {
        Ok(a) => a,
        Err(e) => {
            warn!("foreground_window: {e}");
            return None;
        }
    };
    let url = extract_url(&active);
    let matched = resolve(&configs, &active, url.as_deref())?;

    let baseline = snapshot(app);
    if let Err(e) = apply(app, matched) {
        warn!("power_mode apply: {e}");
        return None;
    }

    let session = PowerSession {
        config_id: matched.id.clone(),
        config_name: matched.name.clone(),
        emoji: matched.emoji.clone(),
        baseline,
    };
    *state.0.lock() = Some(session.clone());
    let _ = config::set_active_id(app, Some(&session.config_id));
    info!(
        config = %session.config_name,
        exe = %active.exe_name,
        url = url.as_deref().unwrap_or(""),
        "Power Mode actif"
    );
    Some(session)
}

/// A la fin de l'enregistrement (success ou cancel) : restaure la baseline
/// si auto_restore est active, sinon laisse la config en place.
pub fn end_session(app: &AppHandle) {
    let Some(state) = app.try_state::<PowerSessionState>() else {
        return;
    };
    let session = state.0.lock().take();
    let Some(session) = session else {
        return;
    };
    if config::is_auto_restore(app) {
        if let Err(e) = restore(app, &session.baseline) {
            warn!("power_mode restore: {e}");
        } else {
            let _ = config::set_active_id(app, None);
            info!(config = %session.config_name, "Power Mode baseline restauree");
        }
    }
}

/// Renvoie la session courante sans l'alterer (pour l'UI).
pub fn current(app: &AppHandle) -> Option<PowerSession> {
    app.try_state::<PowerSessionState>()?.0.lock().clone()
}

/// Liste des configs activees, dans l'ordre stocke. Mirroir VoiceInk
/// `enabledConfigurations = configurations.filter { $0.isEnabled }`
/// (PowerModeConfig.swift L300). Sert a la selection manuelle par index via
/// les raccourcis Alt+chiffre et a compter les profils adressables.
pub fn enabled_configs(app: &AppHandle) -> Vec<PowerModeConfig> {
    config::load_all(app)
        .unwrap_or_default()
        .into_iter()
        .filter(|c| c.is_enabled)
        .collect()
}

/// Selectionne manuellement le Nieme profil active (raccourci Alt+chiffre,
/// index 0-base). Contrairement a begin_session (matching auto fenetre/URL),
/// on applique le profil demande explicitement.
///
/// Si une session Power Mode est deja active, on CONSERVE sa baseline
/// d'origine : le restore de fin de dictee doit ramener aux reglages d'avant
/// l'enregistrement, pas a ceux du profil precedemment applique. Sinon on
/// snapshot l'etat courant (cas ou aucun profil n'avait matche au start).
///
/// Reference VoiceInk : MiniRecorderShortcutManager.setupPowerModeHandler ->
/// setActiveConfiguration + PowerModeSessionManager.beginSession(with:).
pub fn select_by_index(app: &AppHandle, index: usize) -> Option<PowerSession> {
    let state = app.try_state::<PowerSessionState>()?;
    let configs = enabled_configs(app);
    let cfg = configs.get(index)?;

    // Bind avant le match pour relacher le lock immediatement (ne pas le tenir
    // pendant snapshot/apply).
    let existing = state.0.lock().clone();
    let baseline = match existing {
        Some(s) => s.baseline,
        None => snapshot(app),
    };

    if let Err(e) = apply(app, cfg) {
        warn!("power_mode select apply: {e}");
        return None;
    }

    let session = PowerSession {
        config_id: cfg.id.clone(),
        config_name: cfg.name.clone(),
        emoji: cfg.emoji.clone(),
        baseline,
    };
    *state.0.lock() = Some(session.clone());
    let _ = config::set_active_id(app, Some(&session.config_id));
    info!(
        config = %session.config_name,
        index,
        "Power Mode selectionne via raccourci"
    );
    Some(session)
}
