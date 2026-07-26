// Module db - SQLite (rusqlite bundled).
//
// Reference VoiceInk : utilise SwiftData pour Transcription, WordReplacement, etc.
// Ici on utilise SQLite pour la portabilite. Schema identique sur le fond.

pub mod schema;
pub mod transcription;
pub mod word_replacement;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::Connection;
use tauri::{AppHandle, Manager};

pub struct Database(pub Arc<Mutex<Connection>>);

impl Database {
    pub fn open(app: &AppHandle) -> Result<Self> {
        let path = db_path(app)?;
        let conn =
            Connection::open(&path).with_context(|| format!("ouverture DB {}", path.display()))?;
        schema::init(&conn)?;
        Ok(Self(Arc::new(Mutex::new(conn))))
    }
}

fn db_path(app: &AppHandle) -> Result<PathBuf> {
    let base = app
        .path()
        .app_local_data_dir()
        .map_err(|e| anyhow::anyhow!("app_local_data_dir: {e}"))?;
    std::fs::create_dir_all(&base).ok();
    Ok(base.join("parla.db"))
}
