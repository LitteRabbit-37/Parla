// Commandes Tauri pour configurer les raccourcis globaux a chaud.
//
// Etat persiste sous parla.settings.json -> "hotkey" ->
//   { primary, secondary, actions: { copy_last_transcription, ... } }.
// Au boot, lib.rs::setup_hotkeys lit la config et installe le hook avec ces
// triggers + modes. A chaque set_hotkey_config :
//   1. on valide (conflits, touches reservees),
//   2. on persiste dans le store,
//   3. on appelle keyboard_hook::update_watched / update_utilities pour
//      swap les triggers surveilles sans re-installer le hook,
//   4. on appelle HotkeyManager::update_modes / set_custom_cancel,
//   5. on reconstruit le menu tray (accelerateurs affiches).
// Pas de redemarrage requis.
//
// Reference VoiceInk : Features/Shortcuts/State/ShortcutStore.swift
// (une cle UserDefaults par action), ShortcutValidator.swift (conflits
// entre actions, touche seule interdite hors F-keys, Option+chiffre reserve
// a la selection de mode).

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{command, AppHandle, State};
use tauri_plugin_store::StoreExt;

use crate::hotkeys::keyboard_hook::{self, HotkeyOption, HotkeyTrigger, UtilityAction};
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
    /// Raccourcis additionnels (VoiceInk "Additional Shortcuts"). Absent
    /// des configs < 0.6.0 : serde(default) = tout a `None`.
    #[serde(default)]
    pub actions: UtilityShortcuts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeySlotConfig {
    pub trigger: HotkeyTrigger,
    pub mode: HotkeyMode,
}

/// Un trigger par action utilitaire, plus le raccourci d'annulation.
/// Tous `None` par defaut, comme VoiceInk ("Not Set").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UtilityShortcuts {
    /// Ajout Parla (VoiceInk n'a qu'un accelerateur de menu Shift+Cmd+C).
    pub copy_last_transcription: HotkeyTrigger,
    /// VoiceInk "Paste Last Transcription (Original)".
    pub paste_last_transcription: HotkeyTrigger,
    /// VoiceInk "Paste Last Transcription (Enhanced)".
    pub paste_last_enhancement: HotkeyTrigger,
    /// VoiceInk "Retry Last Transcription".
    pub retry_last_transcription: HotkeyTrigger,
    /// VoiceInk "Open History Window" (Parla : fenetre principale, onglet
    /// Historique).
    pub open_history: HotkeyTrigger,
    /// VoiceInk "Cancel Recording". `None` = double-Echap par defaut.
    pub cancel_recording: HotkeyTrigger,
}

impl UtilityShortcuts {
    pub fn trigger_for(&self, action: UtilityAction) -> HotkeyTrigger {
        match action {
            UtilityAction::CopyLastTranscription => self.copy_last_transcription,
            UtilityAction::PasteLastTranscription => self.paste_last_transcription,
            UtilityAction::PasteLastEnhancement => self.paste_last_enhancement,
            UtilityAction::RetryLastTranscription => self.retry_last_transcription,
            UtilityAction::OpenHistory => self.open_history,
        }
    }

    /// Paires (action, trigger) pour le hook clavier.
    pub fn as_utility_list(&self) -> Vec<(UtilityAction, HotkeyTrigger)> {
        UtilityAction::ALL
            .iter()
            .map(|a| (*a, self.trigger_for(*a)))
            .collect()
    }
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
            actions: UtilityShortcuts::default(),
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
    validate(&config)?;
    persist(&app, &config)?;
    apply_live(&state.0, &config);
    crate::tray::refresh(&app);
    Ok(())
}

/// Remet les deux raccourcis d'enregistrement a leur valeur par defaut
/// (Right Alt + Hybrid, pas de secondaire). Les raccourcis additionnels
/// sont conserves : chacun a son propre bouton d'effacement dans l'UI.
#[command]
pub fn reset_hotkey_config(
    app: AppHandle,
    state: State<HotkeyManagerState>,
) -> Result<HotkeyConfig, String> {
    let mut cfg = HotkeyConfig::defaults();
    cfg.actions = load(&app).actions;
    persist(&app, &cfg)?;
    apply_live(&state.0, &cfg);
    crate::tray::refresh(&app);
    Ok(cfg)
}

fn persist(app: &AppHandle, cfg: &HotkeyConfig) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let value = serde_json::to_value(cfg).map_err(|e| e.to_string())?;
    store.set(KEY_HOTKEY, value);
    store.save().map_err(|e| e.to_string())
}

pub fn apply_live(manager: &Arc<HotkeyManager>, cfg: &HotkeyConfig) {
    keyboard_hook::update_watched(cfg.primary.trigger, cfg.secondary.trigger);
    keyboard_hook::update_utilities(cfg.actions.as_utility_list(), cfg.actions.cancel_recording);
    manager.update_modes(cfg.primary.mode, cfg.secondary.mode);
    manager.set_custom_cancel(!matches!(cfg.actions.cancel_recording, HotkeyTrigger::None));
}

// -- Validation (VoiceInk ShortcutValidator) --------------------------------

/// Identifiant stable d'une action pour les messages d'erreur, traduit
/// cote frontend (`errors.shortcutConflict` + `hotkey.additional.<id>`).
fn action_id(action: UtilityAction) -> &'static str {
    match action {
        UtilityAction::CopyLastTranscription => "copyLast",
        UtilityAction::PasteLastTranscription => "pasteLast",
        UtilityAction::PasteLastEnhancement => "pasteLastEnhanced",
        UtilityAction::RetryLastTranscription => "retryLast",
        UtilityAction::OpenHistory => "openHistory",
    }
}

/// Touches acceptees seules (sans modificateur) : F1-F24, Pause,
/// PrintScreen, Insert, Delete, Home, End, PageUp/Down. Miroir de
/// STANDALONE_VKS dans src/components/HotkeyRecorder.tsx et de la regle
/// VoiceInk `isFunctionKeyCode` (F-keys autorisees sans modificateur).
fn standalone_allowed(vk: u32) -> bool {
    matches!(vk, 0x13 | 0x21 | 0x22 | 0x23 | 0x24 | 0x2C | 0x2D | 0x2E) || (0x70..=0x87).contains(&vk)
}

/// Verifie la config avant persistance. Codes d'erreur stables :
///  - `PARLA_ERR:shortcutNeedsModifier:<action>` : touche textuelle seule.
///  - `PARLA_ERR:shortcutReserved:<action>` : Alt+chiffre (selection de
///    profil Power Mode pendant l'enregistrement).
///  - `PARLA_ERR:shortcutConflict:<action a>:<action b>` : meme trigger
///    utilise deux fois.
pub fn validate(cfg: &HotkeyConfig) -> Result<(), String> {
    let mut entries: Vec<(&'static str, HotkeyTrigger)> = vec![
        ("primary", cfg.primary.trigger),
        ("secondary", cfg.secondary.trigger),
        ("cancelRecording", cfg.actions.cancel_recording),
    ];
    for a in UtilityAction::ALL {
        entries.push((action_id(a), cfg.actions.trigger_for(a)));
    }
    let entries: Vec<(&'static str, HotkeyTrigger)> = entries
        .into_iter()
        .filter(|(_, t)| !matches!(t, HotkeyTrigger::None))
        .collect();

    for (name, trigger) in &entries {
        if let HotkeyTrigger::Combo {
            vk,
            ctrl,
            alt,
            shift,
            win,
        } = trigger
        {
            let has_modifier = *ctrl || *alt || *shift || *win;
            // Echap seul est accepte uniquement pour l'annulation (VoiceInk
            // ShortcutValidator : `.cancelRecorder` + kVK_Escape sans flags).
            let escape_for_cancel = *name == "cancelRecording" && *vk == 0x1B;
            if !has_modifier && !standalone_allowed(*vk) && !escape_for_cancel {
                return Err(format!("PARLA_ERR:shortcutNeedsModifier:{name}"));
            }
            let is_top_row_digit = (0x30..=0x39).contains(vk);
            if *alt && !*ctrl && !*shift && !*win && is_top_row_digit {
                return Err(format!("PARLA_ERR:shortcutReserved:{name}"));
            }
        }
        if let HotkeyTrigger::Modifier {
            option: HotkeyOption::None,
        } = trigger
        {
            return Err(format!("PARLA_ERR:shortcutNeedsModifier:{name}"));
        }
    }

    for (i, (name_a, trig_a)) in entries.iter().enumerate() {
        for (name_b, trig_b) in entries.iter().skip(i + 1) {
            if trig_a == trig_b {
                return Err(format!("PARLA_ERR:shortcutConflict:{name_a}:{name_b}"));
            }
        }
    }
    Ok(())
}

/// Liste exhaustive des modifier-only options exposees au frontend pour
/// peupler le picker. L'ordre est aligne avec ce que VoiceInk montre dans
/// son Settings (Right Option / Left Option / Right Control / etc.).
#[command]
pub fn list_hotkey_options() -> Vec<HotkeyOptionInfo> {
    vec![
        HotkeyOptionInfo { id: HotkeyOption::None, vk: None },
        HotkeyOptionInfo { id: HotkeyOption::RightAlt, vk: HotkeyOption::RightAlt.vk() },
        HotkeyOptionInfo { id: HotkeyOption::LeftAlt, vk: HotkeyOption::LeftAlt.vk() },
        HotkeyOptionInfo { id: HotkeyOption::RightCtrl, vk: HotkeyOption::RightCtrl.vk() },
        HotkeyOptionInfo { id: HotkeyOption::LeftCtrl, vk: HotkeyOption::LeftCtrl.vk() },
        HotkeyOptionInfo { id: HotkeyOption::RightWin, vk: HotkeyOption::RightWin.vk() },
        HotkeyOptionInfo { id: HotkeyOption::RightShift, vk: HotkeyOption::RightShift.vk() },
        HotkeyOptionInfo { id: HotkeyOption::LeftShift, vk: HotkeyOption::LeftShift.vk() },
    ]
}

#[derive(Debug, Serialize)]
pub struct HotkeyOptionInfo {
    pub id: HotkeyOption,
    pub vk: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combo(vk: u32, ctrl: bool, alt: bool, shift: bool) -> HotkeyTrigger {
        HotkeyTrigger::Combo {
            vk,
            ctrl,
            alt,
            shift,
            win: false,
        }
    }

    #[test]
    fn defaults_are_valid_and_legacy_json_loads() {
        assert!(validate(&HotkeyConfig::defaults()).is_ok());
        // Config 0.5.x sans champ `actions`.
        let legacy = serde_json::json!({
            "primary": { "trigger": { "kind": "modifier", "option": "rightAlt" }, "mode": "hybrid" },
            "secondary": { "trigger": { "kind": "none" }, "mode": "hybrid" }
        });
        let cfg: HotkeyConfig = serde_json::from_value(legacy).expect("legacy config");
        assert!(matches!(cfg.actions.copy_last_transcription, HotkeyTrigger::None));
        assert!(matches!(cfg.actions.cancel_recording, HotkeyTrigger::None));
    }

    #[test]
    fn rejects_conflict_between_actions() {
        let mut cfg = HotkeyConfig::defaults();
        cfg.actions.copy_last_transcription = combo(0x43, true, false, true);
        cfg.actions.paste_last_transcription = combo(0x43, true, false, true);
        let err = validate(&cfg).unwrap_err();
        assert_eq!(err, "PARLA_ERR:shortcutConflict:copyLast:pasteLast");
    }

    #[test]
    fn rejects_conflict_with_recording_trigger() {
        let mut cfg = HotkeyConfig::defaults();
        cfg.primary.trigger = combo(0x52, true, true, false); // Ctrl+Alt+R
        cfg.actions.retry_last_transcription = combo(0x52, true, true, false);
        let err = validate(&cfg).unwrap_err();
        assert_eq!(err, "PARLA_ERR:shortcutConflict:primary:retryLast");
    }

    #[test]
    fn rejects_reserved_alt_digit_and_bare_letter() {
        let mut cfg = HotkeyConfig::defaults();
        cfg.actions.open_history = combo(0x31, false, true, false); // Alt+1
        assert_eq!(
            validate(&cfg).unwrap_err(),
            "PARLA_ERR:shortcutReserved:openHistory"
        );
        let mut cfg = HotkeyConfig::defaults();
        cfg.actions.open_history = combo(0x48, false, false, false); // H seul
        assert_eq!(
            validate(&cfg).unwrap_err(),
            "PARLA_ERR:shortcutNeedsModifier:openHistory"
        );
    }

    #[test]
    fn accepts_function_key_alone_and_escape_for_cancel() {
        let mut cfg = HotkeyConfig::defaults();
        cfg.actions.copy_last_transcription = combo(0x7C, false, false, false); // F13
        cfg.actions.cancel_recording = combo(0x1B, false, false, false); // Esc
        assert!(validate(&cfg).is_ok());
        // Esc seul n'est pas accepte pour une autre action.
        let mut cfg = HotkeyConfig::defaults();
        cfg.actions.open_history = combo(0x1B, false, false, false);
        assert!(validate(&cfg).is_err());
    }
}
