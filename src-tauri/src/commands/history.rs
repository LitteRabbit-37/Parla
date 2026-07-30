// Commandes Tauri pour l'historique des transcriptions.

use chrono::{DateTime, Utc};
use tauri::{command, AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::db::{transcription as repo, Database};
use crate::history::cleanup::{self, RetentionSettings};
use crate::history::export;

#[command]
pub fn list_history(
    app: AppHandle,
    limit: Option<i64>,
    before: Option<String>,
    search: Option<String>,
) -> Result<Vec<repo::TranscriptionRecord>, String> {
    let db = app
        .try_state::<Database>()
        .ok_or_else(|| "DB absente".to_string())?;
    let lim = limit.unwrap_or(20).clamp(1, 200);
    let before_ts = before
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.with_timezone(&Utc));
    let search_s = search.filter(|s| !s.trim().is_empty());
    let conn = db.0.lock();
    repo::list_page(&conn, lim, before_ts, search_s.as_deref()).map_err(|e| e.to_string())
}

#[command]
pub fn get_history_item(
    app: AppHandle,
    id: String,
) -> Result<Option<repo::TranscriptionRecord>, String> {
    let db = app
        .try_state::<Database>()
        .ok_or_else(|| "DB absente".to_string())?;
    let conn = db.0.lock();
    repo::get(&conn, &id).map_err(|e| e.to_string())
}

#[command]
pub fn delete_history_item(app: AppHandle, id: String) -> Result<(), String> {
    let db = app
        .try_state::<Database>()
        .ok_or_else(|| "DB absente".to_string())?;
    let wav = {
        let conn = db.0.lock();
        repo::delete(&conn, &id).map_err(|e| e.to_string())?
    };
    if let Some(name) = wav {
        if let Ok(dir) = app.path().app_local_data_dir() {
            let p = dir.join("Recordings").join(name);
            if p.exists() {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    Ok(())
}

#[command]
pub fn count_history(app: AppHandle) -> Result<i64, String> {
    let db = app
        .try_state::<Database>()
        .ok_or_else(|| "DB absente".to_string())?;
    let conn = db.0.lock();
    repo::count(&conn).map_err(|e| e.to_string())
}

#[command]
pub async fn export_history_csv(
    app: AppHandle,
    ids: Vec<String>,
) -> Result<Option<String>, String> {
    // Collect les records a exporter.
    let db = app
        .try_state::<Database>()
        .ok_or_else(|| "DB absente".to_string())?;
    let records: Vec<repo::TranscriptionRecord> = {
        let conn = db.0.lock();
        ids.iter()
            .filter_map(|id| repo::get(&conn, id).ok().flatten())
            .collect()
    };
    if records.is_empty() {
        return Ok(None);
    }
    let csv = export::to_csv(&records);

    // Pick un fichier via dialog.
    let Some(path) = app
        .dialog()
        .file()
        .set_file_name("parla-history.csv")
        .add_filter("CSV", &["csv"])
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let p = path
        .as_path()
        .ok_or_else(|| "chemin invalide".to_string())?
        .to_path_buf();
    std::fs::write(&p, csv).map_err(|e| e.to_string())?;
    Ok(Some(p.to_string_lossy().into_owned()))
}

#[command]
pub fn get_retention_settings(app: AppHandle) -> RetentionSettings {
    cleanup::load(&app)
}

#[command]
pub fn set_retention_settings(app: AppHandle, settings: RetentionSettings) -> Result<(), String> {
    cleanup::save(&app, &settings).map_err(|e| e.to_string())?;
    // Declenche un passage immediat pour appliquer les nouveaux criteres.
    cleanup::run_once(&app);
    Ok(())
}

#[command]
pub fn run_history_cleanup(app: AppHandle) {
    cleanup::run_once(&app);
}
