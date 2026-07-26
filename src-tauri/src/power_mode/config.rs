// Modele de donnees et persistance des configurations Power Mode.
//
// Reference VoiceInk : PowerMode/PowerModeConfig.swift (struct Codable
// persisted dans UserDefaults["powerModeConfigurationsV2"]). Sur Parla,
// stockage JSON via tauri-plugin-store dans parla.power_mode.json.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;
use uuid::Uuid;

const STORE_FILE: &str = "parla.power_mode.json";
const KEY_CONFIGS: &str = "configurations";
const KEY_ACTIVE_ID: &str = "active_configuration_id";
const KEY_AUTO_RESTORE: &str = "auto_restore_enabled";

/// Association app (exe name sans extension, lowercase) -> config.
/// VoiceInk utilise bundleIdentifier + appName ; sur Windows on stocke
/// `exe_name` et un `app_name` facultatif (display).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppTrigger {
    pub id: String,
    /// Nom de l'exe sans extension, en minuscules (ex "chrome", "code").
    pub exe_name: String,
    /// Libelle affiche dans l'UI.
    pub app_name: String,
}

/// URL glob : VoiceInk fait une simple substring match apres normalisation
/// (clean_for_match). On garde ce comportement.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UrlTrigger {
    pub id: String,
    /// Chaine raw telle que l'user l'a saisie (ex "github.com", "docs.").
    pub url: String,
}

/// Quelles touches envoyer apres paste pour declencher un submit auto ?
/// Reference VoiceInk AutoSendKey.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutoSendKey {
    #[default]
    None,
    Enter,
    ShiftEnter,
    CtrlEnter,
}

/// Une configuration Power Mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerModeConfig {
    pub id: String,
    pub name: String,
    pub emoji: String,

    #[serde(default)]
    pub app_triggers: Vec<AppTrigger>,
    #[serde(default)]
    pub url_triggers: Vec<UrlTrigger>,

    #[serde(default)]
    pub is_enhancement_enabled: bool,
    /// Active la capture d'ecran + OCR comme contexte enhance (VoiceInk
    /// useScreenCaptureContext). None = inchange, Some(b) = force la valeur.
    #[serde(default)]
    pub use_screen_capture: Option<bool>,
    /// UUID d'un prompt existant.
    #[serde(default)]
    pub selected_prompt_id: Option<String>,

    /// Provider LLM (id du LLMProvider ou None = laisse le defaut).
    #[serde(default)]
    pub selected_llm_provider: Option<String>,
    /// Modele LLM associe au provider.
    #[serde(default)]
    pub selected_llm_model: Option<String>,

    /// "local" | "cloud" | "parakeet".
    #[serde(default)]
    pub transcription_kind: Option<String>,
    /// Pour kind=local : id du modele Whisper.
    #[serde(default)]
    pub whisper_model_id: Option<String>,
    /// Pour kind=cloud : couple provider+model.
    #[serde(default)]
    pub cloud_provider: Option<String>,
    #[serde(default)]
    pub cloud_model: Option<String>,
    /// Pour kind=parakeet : id de variante.
    #[serde(default)]
    pub parakeet_model_id: Option<String>,

    /// Code langue whisper/cloud (fr, en, auto...).
    #[serde(default)]
    pub language: Option<String>,

    #[serde(default)]
    pub auto_send_key: AutoSendKey,

    #[serde(default = "default_true")]
    pub is_enabled: bool,
    #[serde(default)]
    pub is_default: bool,
}

fn default_true() -> bool {
    true
}

impl PowerModeConfig {
    #[allow(dead_code)]
    pub fn new(name: String, emoji: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            emoji,
            app_triggers: Vec::new(),
            url_triggers: Vec::new(),
            is_enhancement_enabled: false,
            use_screen_capture: None,
            selected_prompt_id: None,
            selected_llm_provider: None,
            selected_llm_model: None,
            transcription_kind: None,
            whisper_model_id: None,
            cloud_provider: None,
            cloud_model: None,
            parakeet_model_id: None,
            language: None,
            auto_send_key: AutoSendKey::None,
            is_enabled: true,
            is_default: false,
        }
    }
}

// -- Persistance ------------------------------------------------------------

pub fn load_all(app: &AppHandle) -> Result<Vec<PowerModeConfig>> {
    let store = app.store(STORE_FILE).map_err(|e| anyhow!("store: {e}"))?;
    if let Some(v) = store.get(KEY_CONFIGS) {
        if let Ok(list) = serde_json::from_value::<Vec<PowerModeConfig>>(v) {
            return Ok(list);
        }
    }
    Ok(Vec::new())
}

pub fn save_all(app: &AppHandle, list: &[PowerModeConfig]) -> Result<()> {
    let store = app.store(STORE_FILE).map_err(|e| anyhow!("store: {e}"))?;
    store.set(KEY_CONFIGS, serde_json::to_value(list)?);
    store.save().map_err(|e| anyhow!("store save: {e}"))
}

pub fn set_active_id(app: &AppHandle, id: Option<&str>) -> Result<()> {
    let store = app.store(STORE_FILE).map_err(|e| anyhow!("store: {e}"))?;
    match id {
        Some(i) => store.set(KEY_ACTIVE_ID, serde_json::Value::String(i.into())),
        None => {
            store.delete(KEY_ACTIVE_ID);
        }
    }
    store.save().map_err(|e| anyhow!("store save: {e}"))
}

pub fn is_auto_restore(app: &AppHandle) -> bool {
    app.store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(KEY_AUTO_RESTORE).and_then(|v| v.as_bool()))
        .unwrap_or(true)
}

pub fn set_auto_restore(app: &AppHandle, enabled: bool) -> Result<()> {
    let store = app.store(STORE_FILE).map_err(|e| anyhow!("store: {e}"))?;
    store.set(KEY_AUTO_RESTORE, serde_json::Value::Bool(enabled));
    store.save().map_err(|e| anyhow!("store save: {e}"))
}
