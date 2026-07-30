// Commandes Tauri pour configurer le hotkey global a chaud.
//
// Etat persiste sous parla.settings.json -> "hotkey" -> { primary, secondary }.
// Au boot, lib.rs::setup_hotkeys lit la config et installe le hook avec ces
// triggers + modes. A chaque set_hotkey_config :
//   1. on persiste dans le store,
//   2. on appelle keyboard_hook::update_watched pour swap les triggers
//      surveilles sans re-installer le hook,
//   3. on appelle HotkeyManager::update_modes pour swap les modes.
// Pas de redemarrage requis.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{command, AppHandle, State};
use tauri_plugin_store::StoreExt;

use crate::hotkeys::keyboard_hook::{self, HotkeyOption, HotkeyTrigger};
use crate::hotkeys::manager::{HotkeyManager, HotkeyMode};

const STORE_FILE: &str = "parla.settings.json";
const KEY_HOTKEY: &str = "hotkey";

/// Wrapper Tauri State pour partager l'Arc<HotkeyManager> entre setup et
/// commandes. Le manager lui-meme est interieurement Sync (Mutex).
pub struct HotkeyManagerState(pub Arc<HotkeyManager>);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    pub primary: HotkeySlotConfig,
    pub secondary: HotkeySlotConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeySlotConfig {
    pub trigger: HotkeyTrigger,
    pub mode: HotkeyMode,
}

impl HotkeyConfig {
    pub fn defaults() -> Self {
        Self {
            primary: HotkeySlotConfig {
                trigger: HotkeyTrigger::default_primary(),
                mode: HotkeyMode::Hybrid,
            },
            secondary: HotkeySlotConfig {
                trigger: HotkeyTrigger::None,
                mode: HotkeyMode::Hybrid,
            },
        }
    }
}

/// Lit la config depuis le store. Defaults appliques par champ : si la
/// cle est absente OU si le format a change (deserialize fail), on retombe
/// sur Right Alt + Hybrid.
pub fn load(app: &AppHandle) -> HotkeyConfig {
    let Some(store) = app.store(STORE_FILE).ok() else {
        return HotkeyConfig::defaults();
    };
    let Some(value) = store.get(KEY_HOTKEY) else {
        return HotkeyConfig::defaults();
    };
    serde_json::from_value(value).unwrap_or_else(|err| {
        tracing::warn!("hotkey config invalide, fallback default: {err}");
        HotkeyConfig::defaults()
    })
}

#[command]
pub fn get_hotkey_config(app: AppHandle) -> HotkeyConfig {
    load(&app)
}

#[command]
pub fn set_hotkey_config(
    app: AppHandle,
    state: State<HotkeyManagerState>,
    config: HotkeyConfig,
) -> Result<(), String> {
    persist(&app, &config)?;
    apply_live(&state.0, &config);
    Ok(())
}

#[command]
pub fn reset_hotkey_config(
    app: AppHandle,
    state: State<HotkeyManagerState>,
) -> Result<HotkeyConfig, String> {
    let cfg = HotkeyConfig::defaults();
    persist(&app, &cfg)?;
    apply_live(&state.0, &cfg);
    Ok(cfg)
}

fn persist(app: &AppHandle, cfg: &HotkeyConfig) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let value = serde_json::to_value(cfg).map_err(|e| e.to_string())?;
    store.set(KEY_HOTKEY, value);
    store.save().map_err(|e| e.to_string())
}

fn apply_live(manager: &Arc<HotkeyManager>, cfg: &HotkeyConfig) {
    keyboard_hook::update_watched(cfg.primary.trigger, cfg.secondary.trigger);
    manager.update_modes(cfg.primary.mode, cfg.secondary.mode);
}

/// Liste exhaustive des modifier-only options exposees au frontend pour
/// peupler le picker. L'ordre est aligne avec ce que VoiceInk montre dans
/// son Settings (Right Option / Left Option / Right Control / etc.).
#[command]
pub fn list_hotkey_options() -> Vec<HotkeyOptionInfo> {
    vec![
        HotkeyOptionInfo {
            id: HotkeyOption::None,
            vk: None,
        },
        HotkeyOptionInfo {
            id: HotkeyOption::RightAlt,
            vk: HotkeyOption::RightAlt.vk(),
        },
        HotkeyOptionInfo {
            id: HotkeyOption::LeftAlt,
            vk: HotkeyOption::LeftAlt.vk(),
        },
        HotkeyOptionInfo {
            id: HotkeyOption::RightCtrl,
            vk: HotkeyOption::RightCtrl.vk(),
        },
        HotkeyOptionInfo {
            id: HotkeyOption::LeftCtrl,
            vk: HotkeyOption::LeftCtrl.vk(),
        },
        HotkeyOptionInfo {
            id: HotkeyOption::RightWin,
            vk: HotkeyOption::RightWin.vk(),
        },
        HotkeyOptionInfo {
            id: HotkeyOption::RightShift,
            vk: HotkeyOption::RightShift.vk(),
        },
        HotkeyOptionInfo {
            id: HotkeyOption::LeftShift,
            vk: HotkeyOption::LeftShift.vk(),
        },
    ]
}

#[derive(Debug, Serialize)]
pub struct HotkeyOptionInfo {
    pub id: HotkeyOption,
    pub vk: Option<u32>,
}
