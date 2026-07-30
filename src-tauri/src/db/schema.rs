// Schema SQLite et migrations simples.
//
// Les tables correspondent aux modeles SwiftData de VoiceInk :
// - word_replacements : Models/WordReplacement.swift L4-18
// - transcriptions : Models/Transcription.swift (Phase 8)

use anyhow::{Context, Result};
use rusqlite::Connection;

/// Version courante du schema. A incrementer pour chaque migration future.
const CURRENT_VERSION: i32 = 2;

pub fn init(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r"
        PRAGMA foreign_keys = ON;
        PRAGMA journal_mode = WAL;

        CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS word_replacements (
            id TEXT PRIMARY KEY,
            original_text TEXT NOT NULL,
            replacement_text TEXT NOT NULL,
            date_added TEXT NOT NULL,
            is_enabled INTEGER NOT NULL DEFAULT 1
        );

        CREATE INDEX IF NOT EXISTS idx_word_replacements_enabled
            ON word_replacements(is_enabled);

        CREATE TABLE IF NOT EXISTS transcriptions (
            id TEXT PRIMARY KEY,
            timestamp TEXT NOT NULL,
            status TEXT NOT NULL,
            text TEXT NOT NULL DEFAULT '',
            enhanced_text TEXT,
            duration_sec REAL,
            transcription_duration_sec REAL,
            enhancement_duration_sec REAL,
            audio_file_name TEXT,
            transcription_model_name TEXT,
            ai_enhancement_model_name TEXT,
            prompt_name TEXT,
            ai_request_system_message TEXT,
            ai_request_user_message TEXT,
            power_mode_name TEXT,
            power_mode_emoji TEXT,
            language TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_transcriptions_timestamp
            ON transcriptions(timestamp DESC);
        ",
    )
    .context("init schema")?;

    // Migration incrementale. V1 -> V2 : ajout table transcriptions (deja
    // faite par CREATE TABLE IF NOT EXISTS ci-dessus, rien d'autre a faire).
    let current: Option<i32> = conn
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
            row.get(0)
        })
        .ok();
    match current {
        None => {
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                [CURRENT_VERSION],
            )?;
        }
        Some(v) if v < CURRENT_VERSION => {
            conn.execute("UPDATE schema_version SET version = ?1", [CURRENT_VERSION])?;
        }
        _ => {}
    }
    Ok(())
}
