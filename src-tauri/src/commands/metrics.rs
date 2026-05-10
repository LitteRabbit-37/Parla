// Commandes Tauri pour le panneau de performance par modele.
//
// Reference VoiceInk : Views/Metrics/ModelPerformancePanel.swift.
// Plutot que de maintenir une table SessionMetric separee comme VoiceInk
// (qui a fait une migration depuis Transcription), on agrege directement la
// table `transcriptions` car Parla persiste deja tous les champs requis
// (transcription_model_name, transcription_duration_sec, duration_sec,
// ai_enhancement_model_name, enhancement_duration_sec).

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::transcription::{
    aggregate_enhancement_metrics, aggregate_transcription_metrics, EnhancementModelMetric,
    TranscriptionModelMetric,
};
use crate::db::Database;

/// Plage temporelle a agreger. Aligne sur VoiceInk TimeFilter (ModelPerformancePanel.swift L8-30).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricsPeriod {
    Last7Days,
    Last30Days,
    ThisYear,
    AllTime,
}

fn period_cutoff(period: MetricsPeriod) -> Option<DateTime<Utc>> {
    let now = Utc::now();
    match period {
        MetricsPeriod::Last7Days => Some(now - chrono::Duration::days(7)),
        MetricsPeriod::Last30Days => Some(now - chrono::Duration::days(30)),
        MetricsPeriod::ThisYear => NaiveDate::from_ymd_opt(now.year(), 1, 1)
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .map(|ndt| DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc)),
        MetricsPeriod::AllTime => None,
    }
}

#[derive(Debug, Serialize)]
pub struct ModelPerformanceMetrics {
    pub transcription_models: Vec<TranscriptionModelMetric>,
    pub enhancement_models: Vec<EnhancementModelMetric>,
}

#[tauri::command]
pub fn get_model_performance_metrics(
    db: State<'_, Database>,
    period: MetricsPeriod,
) -> Result<ModelPerformanceMetrics, String> {
    let cutoff = period_cutoff(period);
    let conn = db.0.lock();
    let transcription_models =
        aggregate_transcription_metrics(&conn, cutoff).map_err(|e| e.to_string())?;
    let enhancement_models =
        aggregate_enhancement_metrics(&conn, cutoff).map_err(|e| e.to_string())?;
    Ok(ModelPerformanceMetrics {
        transcription_models,
        enhancement_models,
    })
}
