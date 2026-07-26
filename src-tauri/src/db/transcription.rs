// Persistance des transcriptions (historique).
//
// Reference VoiceInk : Models/Transcription.swift + TranscriptionPipeline.swift.
// Ecriture : une ligne creee au stop_recording (status pending), puis mise a
// jour apres transcription et enhancement (status completed ou failed).
// Lecture : pagination par timestamp desc, search full-text tolere sur raw
// et enhanced.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Statut d'une ligne. VoiceInk : pending / completed / failed.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptionStatus {
    Pending,
    Completed,
    Failed,
}

impl TranscriptionStatus {
    pub fn parse(s: &str) -> Self {
        match s {
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionRecord {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub status: TranscriptionStatus,
    pub text: String,
    pub enhanced_text: Option<String>,
    pub duration_sec: Option<f64>,
    pub transcription_duration_sec: Option<f64>,
    pub enhancement_duration_sec: Option<f64>,
    pub audio_file_name: Option<String>,
    pub transcription_model_name: Option<String>,
    pub ai_enhancement_model_name: Option<String>,
    pub prompt_name: Option<String>,
    pub ai_request_system_message: Option<String>,
    pub ai_request_user_message: Option<String>,
    pub power_mode_name: Option<String>,
    pub power_mode_emoji: Option<String>,
    pub language: Option<String>,
}

fn row_to_record(row: &Row<'_>) -> rusqlite::Result<TranscriptionRecord> {
    let ts: String = row.get("timestamp")?;
    let timestamp = DateTime::parse_from_rfc3339(&ts)
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let status_str: String = row.get("status")?;
    Ok(TranscriptionRecord {
        id: row.get("id")?,
        timestamp,
        status: TranscriptionStatus::parse(&status_str),
        text: row.get("text")?,
        enhanced_text: row.get("enhanced_text")?,
        duration_sec: row.get("duration_sec")?,
        transcription_duration_sec: row.get("transcription_duration_sec")?,
        enhancement_duration_sec: row.get("enhancement_duration_sec")?,
        audio_file_name: row.get("audio_file_name")?,
        transcription_model_name: row.get("transcription_model_name")?,
        ai_enhancement_model_name: row.get("ai_enhancement_model_name")?,
        prompt_name: row.get("prompt_name")?,
        ai_request_system_message: row.get("ai_request_system_message")?,
        ai_request_user_message: row.get("ai_request_user_message")?,
        power_mode_name: row.get("power_mode_name")?,
        power_mode_emoji: row.get("power_mode_emoji")?,
        language: row.get("language")?,
    })
}

/// Cree une ligne "pending" a la reception du WAV (pre-transcription).
/// Renvoie l'id cree. VoiceInk VoiceInkEngine.swift ~L94.
pub fn insert_pending(
    conn: &Connection,
    audio_file_name: Option<&str>,
    duration_sec: Option<f64>,
) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let ts = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO transcriptions (id, timestamp, status, text, duration_sec, audio_file_name)
         VALUES (?1, ?2, 'pending', '', ?3, ?4)",
        params![id, ts, duration_sec, audio_file_name],
    )?;
    Ok(id)
}

#[derive(Debug, Default)]
pub struct TranscribeFields<'a> {
    pub text: &'a str,
    pub transcription_duration_sec: f64,
    pub transcription_model_name: &'a str,
    pub language: Option<&'a str>,
    pub power_mode_name: Option<&'a str>,
    pub power_mode_emoji: Option<&'a str>,
}

/// Met a jour la ligne apres la transcription (succes). VoiceInk :
/// `transcriptionPipeline.run` apres `transcribe` (L~90).
pub fn mark_transcribed(conn: &Connection, id: &str, f: &TranscribeFields<'_>) -> Result<()> {
    conn.execute(
        "UPDATE transcriptions
         SET text = ?1,
             transcription_duration_sec = ?2,
             transcription_model_name = ?3,
             language = ?4,
             power_mode_name = ?5,
             power_mode_emoji = ?6,
             status = CASE WHEN status = 'failed' THEN 'failed' ELSE 'completed' END
         WHERE id = ?7",
        params![
            f.text,
            f.transcription_duration_sec,
            f.transcription_model_name,
            f.language,
            f.power_mode_name,
            f.power_mode_emoji,
            id,
        ],
    )?;
    Ok(())
}

#[derive(Debug, Default)]
pub struct EnhanceFields<'a> {
    pub enhanced_text: &'a str,
    pub enhancement_duration_sec: f64,
    pub ai_enhancement_model_name: &'a str,
    pub prompt_name: Option<&'a str>,
    pub ai_request_system_message: Option<&'a str>,
    pub ai_request_user_message: Option<&'a str>,
}

/// Met a jour la ligne apres enhancement LLM. VoiceInk :
/// TranscriptionPipeline.swift L~130-150.
pub fn mark_enhanced(conn: &Connection, id: &str, f: &EnhanceFields<'_>) -> Result<()> {
    conn.execute(
        "UPDATE transcriptions
         SET enhanced_text = ?1,
             enhancement_duration_sec = ?2,
             ai_enhancement_model_name = ?3,
             prompt_name = ?4,
             ai_request_system_message = ?5,
             ai_request_user_message = ?6
         WHERE id = ?7",
        params![
            f.enhanced_text,
            f.enhancement_duration_sec,
            f.ai_enhancement_model_name,
            f.prompt_name,
            f.ai_request_system_message,
            f.ai_request_user_message,
            id,
        ],
    )?;
    Ok(())
}

pub fn mark_failed(conn: &Connection, id: &str, err: &str) -> Result<()> {
    // VoiceInk encode l'erreur dans text si la transcription elle-meme a
    // echoue. On suit la meme convention.
    conn.execute(
        "UPDATE transcriptions
         SET status = 'failed', text = ?1
         WHERE id = ?2",
        params![format!("Transcription Failed: {err}"), id],
    )?;
    Ok(())
}

/// Liste la page la plus recente (timestamp DESC). `before` permet la
/// pagination curseur : renvoie les rows dont timestamp < before.
pub fn list_page(
    conn: &Connection,
    limit: i64,
    before: Option<DateTime<Utc>>,
    search: Option<&str>,
) -> Result<Vec<TranscriptionRecord>> {
    let like = search
        .map(|s| format!("%{}%", s.trim().to_lowercase()))
        .filter(|s| s.len() > 2);
    let mut out = Vec::new();
    let mut push = |row: &Row<'_>| -> rusqlite::Result<()> {
        out.push(row_to_record(row)?);
        Ok(())
    };

    match (before, like) {
        (Some(ts), Some(q)) => {
            let ts_s = ts.to_rfc3339();
            let mut stmt = conn.prepare(
                "SELECT * FROM transcriptions
                 WHERE timestamp < ?1
                   AND (lower(text) LIKE ?2 OR lower(ifnull(enhanced_text, '')) LIKE ?2)
                 ORDER BY timestamp DESC
                 LIMIT ?3",
            )?;
            let mut rows = stmt.query(params![ts_s, q, limit])?;
            while let Some(row) = rows.next()? {
                push(row)?;
            }
        }
        (Some(ts), None) => {
            let ts_s = ts.to_rfc3339();
            let mut stmt = conn.prepare(
                "SELECT * FROM transcriptions
                 WHERE timestamp < ?1
                 ORDER BY timestamp DESC
                 LIMIT ?2",
            )?;
            let mut rows = stmt.query(params![ts_s, limit])?;
            while let Some(row) = rows.next()? {
                push(row)?;
            }
        }
        (None, Some(q)) => {
            let mut stmt = conn.prepare(
                "SELECT * FROM transcriptions
                 WHERE lower(text) LIKE ?1 OR lower(ifnull(enhanced_text, '')) LIKE ?1
                 ORDER BY timestamp DESC
                 LIMIT ?2",
            )?;
            let mut rows = stmt.query(params![q, limit])?;
            while let Some(row) = rows.next()? {
                push(row)?;
            }
        }
        (None, None) => {
            let mut stmt =
                conn.prepare("SELECT * FROM transcriptions ORDER BY timestamp DESC LIMIT ?1")?;
            let mut rows = stmt.query(params![limit])?;
            while let Some(row) = rows.next()? {
                push(row)?;
            }
        }
    }

    Ok(out)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<TranscriptionRecord>> {
    let row = conn
        .query_row(
            "SELECT * FROM transcriptions WHERE id = ?1",
            params![id],
            row_to_record,
        )
        .optional()?;
    Ok(row)
}

pub fn delete(conn: &Connection, id: &str) -> Result<Option<String>> {
    // Renvoie audio_file_name si existait (pour purger le WAV en amont).
    let wav: Option<String> = conn
        .query_row(
            "SELECT audio_file_name FROM transcriptions WHERE id = ?1",
            params![id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    conn.execute("DELETE FROM transcriptions WHERE id = ?1", params![id])?;
    Ok(wav)
}

pub fn count(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM transcriptions", [], |r| r.get(0))
        .map_err(|e| anyhow!("count: {e}"))
}

// -- Metrics aggregation par modele ----------------------------------------
//
// Reference VoiceInk : Views/Metrics/ModelPerformancePanel.swift +
// ModelPerformanceAccumulator. Le panel ne consomme que des sums et counts ;
// SQLite peut tout faire en une requete par section (transcription /
// enhancement). Pas de table separee : on agrege directement la table
// `transcriptions` (status='completed').

#[derive(Debug, Clone, Serialize)]
pub struct TranscriptionModelMetric {
    pub name: String,
    pub session_count: i64,
    pub total_audio_sec: f64,
    pub total_processing_sec: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnhancementModelMetric {
    pub name: String,
    pub session_count: i64,
    pub total_duration_sec: f64,
}

pub fn aggregate_transcription_metrics(
    conn: &Connection,
    cutoff: Option<DateTime<Utc>>,
) -> Result<Vec<TranscriptionModelMetric>> {
    let mut sql = String::from(
        "SELECT
            transcription_model_name,
            COUNT(*) AS session_count,
            COALESCE(SUM(duration_sec), 0.0) AS total_audio_sec,
            COALESCE(SUM(transcription_duration_sec), 0.0) AS total_processing_sec
         FROM transcriptions
         WHERE status = 'completed'
           AND transcription_model_name IS NOT NULL
           AND transcription_model_name != ''
           AND transcription_duration_sec IS NOT NULL
           AND transcription_duration_sec > 0",
    );
    let cutoff_str = cutoff.map(|c| c.to_rfc3339());
    if cutoff_str.is_some() {
        sql.push_str(" AND timestamp >= ?1");
    }
    sql.push_str(
        " GROUP BY transcription_model_name
          ORDER BY total_processing_sec / session_count ASC",
    );

    let mut stmt = conn.prepare(&sql)?;
    let map = |row: &Row<'_>| -> rusqlite::Result<TranscriptionModelMetric> {
        Ok(TranscriptionModelMetric {
            name: row.get(0)?,
            session_count: row.get(1)?,
            total_audio_sec: row.get(2)?,
            total_processing_sec: row.get(3)?,
        })
    };
    let rows: Vec<_> = if let Some(c) = cutoff_str {
        stmt.query_map(params![c], map)?
            .collect::<rusqlite::Result<_>>()?
    } else {
        stmt.query_map([], map)?.collect::<rusqlite::Result<_>>()?
    };
    Ok(rows)
}

pub fn aggregate_enhancement_metrics(
    conn: &Connection,
    cutoff: Option<DateTime<Utc>>,
) -> Result<Vec<EnhancementModelMetric>> {
    let mut sql = String::from(
        "SELECT
            ai_enhancement_model_name,
            COUNT(*) AS session_count,
            COALESCE(SUM(enhancement_duration_sec), 0.0) AS total_duration_sec
         FROM transcriptions
         WHERE status = 'completed'
           AND ai_enhancement_model_name IS NOT NULL
           AND ai_enhancement_model_name != ''
           AND enhancement_duration_sec IS NOT NULL
           AND enhancement_duration_sec > 0",
    );
    let cutoff_str = cutoff.map(|c| c.to_rfc3339());
    if cutoff_str.is_some() {
        sql.push_str(" AND timestamp >= ?1");
    }
    sql.push_str(
        " GROUP BY ai_enhancement_model_name
          ORDER BY total_duration_sec / session_count ASC",
    );

    let mut stmt = conn.prepare(&sql)?;
    let map = |row: &Row<'_>| -> rusqlite::Result<EnhancementModelMetric> {
        Ok(EnhancementModelMetric {
            name: row.get(0)?,
            session_count: row.get(1)?,
            total_duration_sec: row.get(2)?,
        })
    };
    let rows: Vec<_> = if let Some(c) = cutoff_str {
        stmt.query_map(params![c], map)?
            .collect::<rusqlite::Result<_>>()?
    } else {
        stmt.query_map([], map)?.collect::<rusqlite::Result<_>>()?
    };
    Ok(rows)
}

/// Supprime les transcriptions plus vieilles que `older_than`. Renvoie
/// les audio_file_name supprimees (pour purge des WAV).
pub fn delete_older_than(conn: &Connection, older_than: DateTime<Utc>) -> Result<Vec<String>> {
    let ts = older_than.to_rfc3339();
    let mut stmt = conn.prepare(
        "SELECT audio_file_name FROM transcriptions
         WHERE timestamp < ?1 AND audio_file_name IS NOT NULL",
    )?;
    let wavs: Vec<String> = stmt
        .query_map(params![ts], |r| r.get::<_, Option<String>>(0))?
        .filter_map(|r| r.ok().flatten())
        .collect();
    drop(stmt);
    conn.execute(
        "DELETE FROM transcriptions WHERE timestamp < ?1",
        params![ts],
    )?;
    Ok(wavs)
}

/// Efface uniquement le audio_file_name des lignes plus vieilles que
/// `older_than` (pour le mode audio-only cleanup, aligne VoiceInk).
pub fn clear_audio_older_than(conn: &Connection, older_than: DateTime<Utc>) -> Result<Vec<String>> {
    let ts = older_than.to_rfc3339();
    let mut stmt = conn.prepare(
        "SELECT audio_file_name FROM transcriptions
         WHERE timestamp < ?1 AND audio_file_name IS NOT NULL",
    )?;
    let wavs: Vec<String> = stmt
        .query_map(params![ts], |r| r.get::<_, Option<String>>(0))?
        .filter_map(|r| r.ok().flatten())
        .collect();
    drop(stmt);
    conn.execute(
        "UPDATE transcriptions SET audio_file_name = NULL
         WHERE timestamp < ?1",
        params![ts],
    )?;
    Ok(wavs)
}

/// Liste tous les audio_file_name references (pour l'orphan sweep).
pub fn all_audio_file_names(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT audio_file_name FROM transcriptions WHERE audio_file_name IS NOT NULL")?;
    let out: Vec<String> = stmt
        .query_map([], |r| r.get::<_, Option<String>>(0))?
        .filter_map(|r| r.ok().flatten())
        .collect();
    Ok(out)
}
