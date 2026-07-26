// Liste des filler words (mots de remplissage) et configuration on/off.
//
// Reference VoiceInk : FillerWordManager.swift
// - defaultFillerWords L6-9 : 12 mots lowercase
// - UserDefaults keys : RemoveFillerWords (bool), FillerWords ([String])
// - isEnabled retourne false si la cle n'existe pas : OFF par defaut.

use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "parla.settings.json";
const ENABLED_KEY: &str = "remove_filler_words";
const WORDS_KEY: &str = "filler_words";

/// Liste par defaut alignee sur FillerWordManager.swift L6-9.
pub const DEFAULT_FILLER_WORDS: &[&str] = &[
    "uh", "um", "uhm", "umm", "uhh", "uhhh", "hmm", "hm", "mmm", "mm", "mh", "ehh",
];

pub fn is_enabled(app: &AppHandle) -> bool {
    let Some(store) = app.store(STORE_FILE).ok() else {
        return false;
    };
    store
        .get(ENABLED_KEY)
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

pub fn set_enabled(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(ENABLED_KEY, serde_json::Value::Bool(enabled));
    store.save().map_err(|e| e.to_string())
}

/// Retourne la liste custom de l'utilisateur, sinon la liste par defaut.
pub fn current_list(app: &AppHandle) -> Vec<String> {
    let Some(store) = app.store(STORE_FILE).ok() else {
        return DEFAULT_FILLER_WORDS.iter().map(|s| s.to_string()).collect();
    };
    match store.get(WORDS_KEY).and_then(|v| v.as_array().cloned()) {
        Some(arr) => arr
            .into_iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        None => DEFAULT_FILLER_WORDS.iter().map(|s| s.to_string()).collect(),
    }
}

pub fn set_list(app: &AppHandle, words: Vec<String>) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let normalized: Vec<serde_json::Value> = words
        .into_iter()
        .map(|w| serde_json::Value::String(w.trim().to_lowercase()))
        .filter(|v| v.as_str().map(|s| !s.is_empty()).unwrap_or(false))
        .collect();
    store.set(WORDS_KEY, serde_json::Value::Array(normalized));
    store.save().map_err(|e| e.to_string())
}
