// Purge time-based des transcriptions (audio uniquement OU ligne complete).
//
// Reference VoiceInk :
// - Services/AudioCleanupManager.swift : supprime le WAV N jours apres.
// - Services/TranscriptionAutoCleanupService.swift : supprime la ligne + WAV
//   N minutes apres (retentionMinutes=0 = ephemere sur completion).
// VoiceInk active au plus un des deux modes : si transcription cleanup est
// on, audio cleanup ne tourne pas.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Result};
use chrono::Utc;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_store::StoreExt;
use tracing::{info, warn};

use crate::db::{transcription as repo, Database};

const STORE_FILE: &str = "parla.settings.json";
const KEY_TRANSC_CLEAN: &str = "history_transcription_cleanup_enabled";
const KEY_TRANSC_MINUTES: &str = "history_transcription_retention_minutes";
const KEY_AUDIO_CLEAN: &str = "history_audio_cleanup_enabled";
const KEY_AUDIO_DAYS: &str = "history_audio_retention_days";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RetentionSettings {
    pub transcription_cleanup: bool,
    pub transcription_retention_minutes: i64,
    pub audio_cleanup: bool,
    pub audio_retention_days: i64,
}

impl Default for RetentionSettings {
    fn default() -> Self {
        // Aligne avec VoiceInk AppDefaults : tout desactive par defaut,
        // transcription retention = 24h (1440 min), audio retention = 7 jours.
        Self {
            transcription_cleanup: false,
            transcription_retention_minutes: 1440,
            audio_cleanup: false,
            audio_retention_days: 7,
        }
    }
}

pub fn load(app: &AppHandle) -> RetentionSettings {
    let Some(store) = app.store(STORE_FILE).ok() else {
        return RetentionSettings::default();
    };
    RetentionSettings {
        transcription_cleanup: store
            .get(KEY_TRANSC_CLEAN)
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        transcription_retention_minutes: store
            .get(KEY_TRANSC_MINUTES)
            .and_then(|v| v.as_i64())
            .unwrap_or(1440),
        audio_cleanup: store
            .get(KEY_AUDIO_CLEAN)
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        audio_retention_days: store
            .get(KEY_AUDIO_DAYS)
            .and_then(|v| v.as_i64())
            .unwrap_or(7),
    }
}

pub fn save(app: &AppHandle, s: &RetentionSettings) -> Result<()> {
    let store = app.store(STORE_FILE).map_err(|e| anyhow!("store: {e}"))?;
    store.set(
        KEY_TRANSC_CLEAN,
        serde_json::Value::Bool(s.transcription_cleanup),
    );
    store.set(
        KEY_TRANSC_MINUTES,
        serde_json::Value::Number(serde_json::Number::from(
            s.transcription_retention_minutes.max(0),
        )),
    );
    store.set(KEY_AUDIO_CLEAN, serde_json::Value::Bool(s.audio_cleanup));
    store.set(
        KEY_AUDIO_DAYS,
        serde_json::Value::Number(serde_json::Number::from(s.audio_retention_days.max(0))),
    );
    store.save().map_err(|e| anyhow!("store save: {e}"))
}

fn recordings_dir(app: &AppHandle) -> Option<PathBuf> {
    let base = app.path().app_local_data_dir().ok()?;
    Some(base.join("Recordings"))
}

fn remove_wav(app: &AppHandle, audio_file_name: &str) {
    if let Some(dir) = recordings_dir(app) {
        let p = dir.join(audio_file_name);
        if p.exists() {
            if let Err(e) = std::fs::remove_file(&p) {
                warn!("cleanup remove {}: {e}", p.display());
            }
        }
    }
}

/// Lance un passage de nettoyage. A appeler au demarrage + periodiquement.
pub fn run_once(app: &AppHandle) {
    let s = load(app);
    let Some(db) = app.try_state::<Database>() else {
        return;
    };

    if s.transcription_cleanup {
        let older =
            Utc::now() - chrono::Duration::minutes(s.transcription_retention_minutes.max(0));
        let wavs = match repo::delete_older_than(&db.0.lock(), older) {
            Ok(v) => v,
            Err(e) => {
                warn!("cleanup delete_older_than: {e}");
                Vec::new()
            }
        };
        for w in &wavs {
            remove_wav(app, w);
        }
        if !wavs.is_empty() {
            info!(count = wavs.len(), "transcriptions anciennes purgees");
            let _ = app.emit("history:cleaned", serde_json::Value::Null);
        }
    } else if s.audio_cleanup {
        let older = Utc::now() - chrono::Duration::days(s.audio_retention_days.max(0));
        let wavs = match repo::clear_audio_older_than(&db.0.lock(), older) {
            Ok(v) => v,
            Err(e) => {
                warn!("cleanup clear_audio_older_than: {e}");
                Vec::new()
            }
        };
        for w in &wavs {
            remove_wav(app, w);
        }
        if !wavs.is_empty() {
            info!(count = wavs.len(), "fichiers audio anciens purges");
            let _ = app.emit("history:cleaned", serde_json::Value::Null);
        }
    }

    // Orphan sweep : si transcription cleanup est actif, on supprime les WAV
    // orphelins (plus references par aucune ligne). VoiceInk fait ca au
    // launch (TranscriptionAutoCleanupService).
    if s.transcription_cleanup {
        if let Some(dir) = recordings_dir(app) {
            if dir.exists() {
                let referenced: std::collections::HashSet<String> =
                    repo::all_audio_file_names(&db.0.lock())
                        .unwrap_or_default()
                        .into_iter()
                        .collect();
                if let Ok(iter) = std::fs::read_dir(&dir) {
                    for entry in iter.flatten() {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if !referenced.contains(&name) {
                            let _ = std::fs::remove_file(entry.path());
                        }
                    }
                }
            }
        }
    }
}

/// Timer de cleanup quotidien. A spawn dans setup() apres l'init DB.
pub fn spawn_daily_timer(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Un premier passage au demarrage (orphan sweep + prune).
        run_once(&app);
        loop {
            tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
            run_once(&app);
        }
    });
}
