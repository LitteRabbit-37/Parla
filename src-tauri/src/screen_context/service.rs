// Orchestration capture + OCR + cache du texte de la fenetre active.
//
// Reference VoiceInk : ScreenCaptureService.swift
// - lastCapturedText @Published String? cache en memoire, persiste entre
//   enregistrements, ecrase a la capture suivante.
// - captureAndExtractText assemble "Active Window: <title>\nApplication:
//   <appName>\n\nWindow Content:\n<OCR text>".
// - Reentrancy : un flag isCapturing ; les captures concurrentes sont droppees.

use std::time::Instant;

use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;
use tracing::{info, warn};

use crate::power_mode::active_window::foreground_window;

use super::capture;
use super::ocr;

const STORE_FILE: &str = "parla.settings.json";
const KEY_ENABLED: &str = "use_screen_capture_context";

pub struct ScreenContextState {
    pub last_text: Mutex<Option<String>>,
    capturing: Mutex<bool>,
}

impl Default for ScreenContextState {
    fn default() -> Self {
        Self {
            last_text: Mutex::new(None),
            capturing: Mutex::new(false),
        }
    }
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

pub fn cached_text(app: &AppHandle) -> Option<String> {
    app.try_state::<ScreenContextState>()?
        .last_text
        .lock()
        .clone()
}

pub fn clear(app: &AppHandle) {
    if let Some(state) = app.try_state::<ScreenContextState>() {
        *state.last_text.lock() = None;
    }
}

/// Capture la fenetre active + OCR + met a jour le cache. Drop si une
/// capture est deja en cours. Bloquant : a appeler depuis spawn_blocking.
pub fn capture_and_ocr(app: &AppHandle) -> Result<Option<String>> {
    let state = app
        .try_state::<ScreenContextState>()
        .ok_or_else(|| anyhow!("ScreenContextState absent"))?;

    // Reentrance guard.
    {
        let mut cap = state.capturing.lock();
        if *cap {
            return Ok(None);
        }
        *cap = true;
    }

    let result = (|| -> Result<String> {
        let active = foreground_window()?;
        let start = Instant::now();
        let cap = capture::capture_foreground(&active)?;
        let text = ocr::recognize_png(&cap.png)?;
        let elapsed = start.elapsed();
        info!(
            w_app = %cap.app_name,
            title = %cap.window_title,
            chars = text.len(),
            ms = elapsed.as_millis() as u64,
            "Screen context OCR"
        );

        let mut out = String::new();
        out.push_str(&format!(
            "Active Window: {}\nApplication: {}\n\nWindow Content:\n",
            if cap.window_title.is_empty() {
                "Unknown"
            } else {
                &cap.window_title
            },
            if cap.app_name.is_empty() {
                "Unknown"
            } else {
                &cap.app_name
            },
        ));
        if text.is_empty() {
            out.push_str("No text detected via OCR");
        } else {
            out.push_str(&text);
        }
        Ok(out)
    })();

    // Release guard quoi qu'il arrive.
    *state.capturing.lock() = false;

    match result {
        Ok(text) => {
            *state.last_text.lock() = Some(text.clone());
            Ok(Some(text))
        }
        Err(e) => {
            warn!("screen context capture echec: {e}");
            Err(e)
        }
    }
}

/// Helper a appeler au record start : si enabled, lance la capture en
/// background (tokio::task::spawn_blocking). Ne bloque pas le hotkey.
pub fn trigger_capture(app: &AppHandle) {
    if !is_enabled(app) {
        return;
    }
    let app_bg: AppHandle = app.clone();
    // Utilise tauri async_runtime pour rester coherent avec les autres tasks.
    tauri::async_runtime::spawn(async move {
        let app_inner = app_bg.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let _ = capture_and_ocr(&app_inner);
        })
        .await;
    });
}
