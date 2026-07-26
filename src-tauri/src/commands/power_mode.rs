// Commandes Tauri pour gerer les configurations Power Mode + la session
// active.

use serde::Serialize;
use tauri::{command, AppHandle};

use crate::power_mode::active_window::{foreground_window, ActiveWindow};
use crate::power_mode::browser_url::extract_url;
use crate::power_mode::config::{self, PowerModeConfig};
use crate::power_mode::matcher::resolve;
use crate::power_mode::session::{self, PowerSession};

// -- CRUD configurations ----------------------------------------------------

#[command]
pub fn list_power_configs(app: AppHandle) -> Result<Vec<PowerModeConfig>, String> {
    config::load_all(&app).map_err(|e| e.to_string())
}

#[command]
pub fn add_power_config(
    app: AppHandle,
    config: PowerModeConfig,
) -> Result<PowerModeConfig, String> {
    let mut all = config::load_all(&app).map_err(|e| e.to_string())?;
    let mut c = config;
    if c.id.is_empty() {
        c.id = uuid::Uuid::new_v4().to_string();
    }
    if c.is_default {
        // Un seul default.
        for x in all.iter_mut() {
            x.is_default = false;
        }
    }
    all.push(c.clone());
    config::save_all(&app, &all).map_err(|e| e.to_string())?;
    Ok(c)
}

#[command]
pub fn update_power_config(app: AppHandle, config: PowerModeConfig) -> Result<(), String> {
    let mut all = config::load_all(&app).map_err(|e| e.to_string())?;
    let pos = all
        .iter()
        .position(|c| c.id == config.id)
        .ok_or_else(|| format!("config introuvable: {}", config.id))?;
    if config.is_default {
        for (i, x) in all.iter_mut().enumerate() {
            if i != pos {
                x.is_default = false;
            }
        }
    }
    all[pos] = config;
    config::save_all(&app, &all).map_err(|e| e.to_string())
}

#[command]
pub fn delete_power_config(app: AppHandle, id: String) -> Result<(), String> {
    let mut all = config::load_all(&app).map_err(|e| e.to_string())?;
    all.retain(|c| c.id != id);
    config::save_all(&app, &all).map_err(|e| e.to_string())
}

#[command]
pub fn reorder_power_configs(app: AppHandle, ordered_ids: Vec<String>) -> Result<(), String> {
    let mut all = config::load_all(&app).map_err(|e| e.to_string())?;
    let mut remapped: Vec<PowerModeConfig> = Vec::with_capacity(all.len());
    for id in ordered_ids {
        if let Some(pos) = all.iter().position(|c| c.id == id) {
            remapped.push(all.remove(pos));
        }
    }
    remapped.append(&mut all);
    config::save_all(&app, &remapped).map_err(|e| e.to_string())
}

// -- Auto restore + active session -----------------------------------------

#[command]
pub fn get_power_auto_restore(app: AppHandle) -> bool {
    config::is_auto_restore(&app)
}

#[command]
pub fn set_power_auto_restore(app: AppHandle, enabled: bool) -> Result<(), String> {
    config::set_auto_restore(&app, enabled).map_err(|e| e.to_string())
}

#[command]
pub fn get_active_power_session(app: AppHandle) -> Option<PowerSession> {
    session::current(&app)
}

// -- Debug / Preview --------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct DetectionPreview {
    pub active: ActiveWindow,
    pub url: Option<String>,
    pub matched_config_id: Option<String>,
    pub matched_config_name: Option<String>,
}

// Serialise ActiveWindow a la main (evite de deriver Serialize dans la def).
impl serde::Serialize for crate::power_mode::active_window::ActiveWindow {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("ActiveWindow", 4)?;
        s.serialize_field("hwnd", &self.hwnd)?;
        s.serialize_field("pid", &self.pid)?;
        s.serialize_field("title", &self.title)?;
        s.serialize_field("exe_name", &self.exe_name)?;
        s.end()
    }
}

#[command]
pub fn power_mode_preview(app: AppHandle) -> Result<DetectionPreview, String> {
    let active = foreground_window().map_err(|e| e.to_string())?;
    let url = extract_url(&active);
    let configs = config::load_all(&app).map_err(|e| e.to_string())?;
    let matched = resolve(&configs, &active, url.as_deref());
    Ok(DetectionPreview {
        matched_config_id: matched.map(|c| c.id.clone()),
        matched_config_name: matched.map(|c| c.name.clone()),
        active,
        url,
    })
}
