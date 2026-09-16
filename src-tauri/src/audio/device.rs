// Enumeration des peripheriques d'entree audio.
//
// Reference VoiceInk : VoiceInk/Services/AudioDeviceManager.swift
// - systemDefault : peripherique par defaut de l'OS
// - custom : UID choisi par l'utilisateur et persiste
// - prioritized : liste priorisee avec fallback automatique
//
// Pour Phase 1a on expose la liste + default. La selection personnalisee
// et la logique prioritized viennent en Phase 1d (settings store).

use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;
use tracing::debug;

const STORE_FILE: &str = "parla.settings.json";
/// Nom du micro choisi par l'utilisateur (reglages / menu tray). Absent ou
/// vide = peripherique par defaut du systeme (VoiceInk
/// AudioDeviceManager.selectedDeviceMode == .systemDefault).
const KEY_SELECTED_INPUT: &str = "selected_input_device";

/// Micro selectionne, `None` = defaut systeme.
pub fn selected_input_device(app: &AppHandle) -> Option<String> {
    app.store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(KEY_SELECTED_INPUT))
        .and_then(|v| v.as_str().map(String::from))
        .filter(|s| !s.trim().is_empty())
}

/// Persiste le micro choisi (`None` = defaut systeme).
pub fn set_selected_input_device(app: &AppHandle, name: Option<String>) -> anyhow::Result<()> {
    let store = app
        .store(STORE_FILE)
        .map_err(|e| anyhow::anyhow!("store: {e}"))?;
    match name.filter(|s| !s.trim().is_empty()) {
        Some(n) => store.set(KEY_SELECTED_INPUT, serde_json::Value::String(n)),
        None => {
            store.delete(KEY_SELECTED_INPUT);
        }
    }
    store
        .save()
        .map_err(|e| anyhow::anyhow!("store save: {e}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDeviceInfo {
    /// Nom humain expose par le driver.
    pub name: String,
    /// Vrai si c'est le peripherique par defaut de l'OS.
    pub is_default: bool,
    /// Sample rate natif par defaut du peripherique (informatif).
    pub default_sample_rate: u32,
    /// Nombre de canaux du format par defaut.
    pub default_channels: u16,
}

pub fn list_input_devices() -> Vec<AudioDeviceInfo> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.name().ok());

    let mut out = Vec::new();
    match host.input_devices() {
        Ok(iter) => {
            for device in iter {
                let name = match device.name() {
                    Ok(n) => n,
                    Err(e) => {
                        debug!("Device sans nom: {e}");
                        continue;
                    }
                };
                let is_default = default_name.as_deref() == Some(name.as_str());
                let (sr, ch) = device
                    .default_input_config()
                    .map(|c| (c.sample_rate().0, c.channels()))
                    .unwrap_or((0, 0));
                out.push(AudioDeviceInfo {
                    name,
                    is_default,
                    default_sample_rate: sr,
                    default_channels: ch,
                });
            }
        }
        Err(e) => {
            debug!("Impossible d'enumerer les peripheriques audio: {e}");
        }
    }
    out
}

/// Cherche un peripherique par nom exact. None si introuvable.
pub fn find_input_device_by_name(name: &str) -> Option<cpal::Device> {
    let host = cpal::default_host();
    host.input_devices()
        .ok()?
        .find(|d| d.name().map(|n| n == name).unwrap_or(false))
}

/// Peripherique d'entree par defaut de l'OS.
pub fn default_input_device() -> Option<cpal::Device> {
    cpal::default_host().default_input_device()
}
