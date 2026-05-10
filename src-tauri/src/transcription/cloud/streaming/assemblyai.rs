// AssemblyAI Universal streaming temps reel via WebSocket.
//
// Reference VoiceInk : LLMkit AssemblyAIStreamingClient.swift.
// WSS    : wss://streaming.assemblyai.com/v3/ws
// Query  : sample_rate=16000, encoding=pcm_s16le, speech_model=<resolved>,
//          + min/max_turn_silence et seuils VAD selon modele,
//          + language_detection=true si pas de langue explicite,
//          + keyterms_prompt (JSON array) si modele compatible.
// Auth   : header Authorization: <apiKey> (PAS de Bearer).
// Audio  : frames binaires PCM 16-bit LE 16 kHz, MINIMUM 1600 bytes par
//          message (~50 ms). On bufferise avant d'envoyer.
// Init   : on attend `{"type":"Begin"}` avant d'emettre SessionStarted.
//          Toute autre message contenant `error` est fatal.
// Commit : on flush le buffer puis on envoie `{"type":"Terminate"}`.
// Events :
//   - "Turn" {transcript, end_of_turn, turn_is_formatted, turn_order}
//       end_of_turn=true && (turn_is_formatted || turn nouveau) -> Committed
//       !end_of_turn                                             -> Partial
//   - "Termination"  -> ignored (committed text already sent)
//   - error: ...     -> Error

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

use super::session::{
    connect_ws, drain_ws_messages, i16_to_le_bytes, StreamingChannels, StreamingConfig,
    StreamingEvent, StreamingProvider,
};

pub struct AssemblyAiStreaming;

const MIN_CHUNK_BYTES: usize = 1_600;
const KEYTERMS_LIMIT: usize = 100;

const UNIV3_MIN_TURN_SILENCE_MS: u32 = 1_500;
const UNIV3_MAX_TURN_SILENCE_MS: u32 = 4_000;
const UNIVSTREAMING_END_OF_TURN_CONFIDENCE: &str = "0.75";
const UNIVSTREAMING_MIN_TURN_SILENCE_MS: u32 = 2_000;
const UNIVSTREAMING_MAX_TURN_SILENCE_MS: u32 = 5_000;

#[async_trait]
impl StreamingProvider for AssemblyAiStreaming {
    fn id(&self) -> &'static str {
        "assemblyai"
    }

    async fn run(
        &self,
        api_key: String,
        config: StreamingConfig,
        channels: StreamingChannels,
        on_event: Box<dyn Fn(StreamingEvent) + Send + Sync>,
    ) -> Result<String> {
        let url = build_streaming_url(&config);

        let mut req = url
            .into_client_request()
            .map_err(|e| anyhow!("ws url parse: {e}"))?;
        req.headers_mut().insert(
            "Authorization",
            api_key
                .parse()
                .map_err(|e| anyhow!("auth header: {e}"))?,
        );

        let ws_stream = connect_ws(req).await?;
        let (mut write, mut read) = ws_stream.split();

        // Handshake : on attend Begin (ou error fatal). Tout autre message
        // est ignore en attendant.
        loop {
            match read.next().await {
                Some(Ok(Message::Text(t))) => {
                    let Ok(json) = serde_json::from_str::<Value>(&t) else {
                        continue;
                    };
                    if let Some(err) = json.get("error").and_then(|v| v.as_str()) {
                        return Err(anyhow!(err.to_string()));
                    }
                    if json.get("type").and_then(|v| v.as_str()) == Some("Begin") {
                        break;
                    }
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(anyhow!("ws read: {e}")),
                None => return Err(anyhow!("ws handshake closed prematurement")),
            }
        }
        on_event(StreamingEvent::SessionStarted);

        let StreamingChannels {
            mut audio_rx,
            mut finalize_rx,
        } = channels;

        let mut state = AaiState::default();
        let mut pending: Vec<u8> = Vec::with_capacity(MIN_CHUNK_BYTES * 4);

        loop {
            tokio::select! {
                biased;
                _ = &mut finalize_rx => {
                    while let Ok(chunk) = audio_rx.try_recv() {
                        pending.extend_from_slice(&i16_to_le_bytes(&chunk));
                    }
                    let _ = flush_buffered_chunks(&mut write, &mut pending).await;
                    if !pending.is_empty() {
                        let leftover: Vec<u8> = std::mem::take(&mut pending);
                        let _ = write.send(Message::Binary(leftover.into())).await;
                    }
                    let _ = write
                        .send(Message::Text(r#"{"type":"Terminate"}"#.into()))
                        .await;
                    drain_ws_messages(&mut read, std::time::Duration::from_secs(5), |t| {
                        handle_text(t, &mut state, &on_event);
                    })
                    .await;
                    let _ = write.close().await;
                    return Ok(state.final_text.trim().to_string());
                }
                chunk = audio_rx.recv() => {
                    match chunk {
                        Some(c) => {
                            pending.extend_from_slice(&i16_to_le_bytes(&c));
                            if let Err(e) = flush_buffered_chunks(&mut write, &mut pending).await {
                                return Err(e);
                            }
                        }
                        None => return Ok(state.final_text.trim().to_string()),
                    }
                }
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(t))) => handle_text(&t, &mut state, &on_event),
                        Some(Ok(Message::Close(_))) => return Ok(state.final_text.trim().to_string()),
                        Some(Ok(_)) => {}
                        Some(Err(e)) => return Err(anyhow!("ws read: {e}")),
                        None => return Ok(state.final_text.trim().to_string()),
                    }
                }
            }
        }
    }
}

/// Envoie tous les chunks complets (>= MIN_CHUNK_BYTES) du buffer pending.
/// Le residu (< MIN_CHUNK_BYTES) reste dans pending pour la prochaine fois.
async fn flush_buffered_chunks<W>(write: &mut W, pending: &mut Vec<u8>) -> Result<()>
where
    W: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    while pending.len() >= MIN_CHUNK_BYTES {
        let chunk: Vec<u8> = pending.drain(..MIN_CHUNK_BYTES).collect();
        write
            .send(Message::Binary(chunk.into()))
            .await
            .map_err(|e| anyhow!("ws send: {e}"))?;
    }
    Ok(())
}

fn build_streaming_url(config: &StreamingConfig) -> String {
    let resolved = streaming_model(&config.model, config.language.as_deref());
    let mut url = format!(
        "wss://streaming.assemblyai.com/v3/ws?sample_rate=16000&encoding=pcm_s16le&speech_model={resolved}",
    );

    if is_universal3_pro(&config.model) {
        url.push_str(&format!(
            "&min_turn_silence={}&max_turn_silence={}&vad_threshold=0.4&speaker_labels=false&language_detection=true&u3_rt_pro_vad_threshold=0.5",
            UNIV3_MIN_TURN_SILENCE_MS, UNIV3_MAX_TURN_SILENCE_MS
        ));
    } else {
        url.push_str(&format!(
            "&format_turns=true&end_of_turn_confidence_threshold={}&min_turn_silence={}&max_turn_silence={}",
            UNIVSTREAMING_END_OF_TURN_CONFIDENCE,
            UNIVSTREAMING_MIN_TURN_SILENCE_MS,
            UNIVSTREAMING_MAX_TURN_SILENCE_MS
        ));
        if should_detect_language(config.language.as_deref())
            && resolved == "universal-streaming-multilingual"
        {
            url.push_str("&language_detection=true");
        }
    }

    let keyterms = normalize_keyterms(&config.custom_vocabulary);
    if supports_keyterms(&config.model) {
        if let Some(json_arr) = json_array_string(&keyterms) {
            if !keyterms.is_empty() {
                url.push_str("&keyterms_prompt=");
                url.push_str(&urlencoding::encode(&json_arr));
            }
        }
    }

    url
}

fn is_universal3_pro(model: &str) -> bool {
    matches!(model, "universal-3-pro" | "u3-rt-pro")
}

fn should_detect_language(language: Option<&str>) -> bool {
    match language {
        Some(l) => l.is_empty() || l == "auto",
        None => true,
    }
}

fn supports_keyterms(model: &str) -> bool {
    matches!(
        model,
        "universal-3-pro"
            | "u3-rt-pro"
            | "universal-streaming"
            | "universal-streaming-english"
            | "universal-streaming-multilingual"
    )
}

/// Nom du modele pour le query param `speech_model`. Map identique a LLMkit.
fn streaming_model(model: &str, language: Option<&str>) -> String {
    if model == "universal-3-pro" || model == "u3-rt-pro" {
        return "u3-rt-pro".into();
    }
    if matches!(
        model,
        "universal-streaming-english" | "universal-streaming-multilingual" | "whisper-rt"
    ) {
        return model.to_string();
    }
    match language {
        Some(l) if !l.is_empty() && l != "auto" => {
            if l == "en" {
                "universal-streaming-english".into()
            } else {
                "universal-streaming-multilingual".into()
            }
        }
        _ => "universal-streaming-multilingual".into(),
    }
}

fn normalize_keyterms(raw: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for term in raw {
        let trimmed = term.trim();
        if trimmed.is_empty() || trimmed.chars().count() > 50 {
            continue;
        }
        if trimmed.split_whitespace().count() > 6 {
            continue;
        }
        let key = trimmed.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        out.push(trimmed.to_string());
        if out.len() == KEYTERMS_LIMIT {
            break;
        }
    }
    out
}

fn json_array_string(values: &[String]) -> Option<String> {
    serde_json::to_string(values).ok()
}

#[derive(Default)]
struct AaiState {
    final_text: String,
    last_committed_turn: Option<i64>,
}

fn handle_text(
    t: &str,
    state: &mut AaiState,
    on_event: &(dyn Fn(StreamingEvent) + Send + Sync),
) {
    let Ok(json) = serde_json::from_str::<Value>(t) else {
        return;
    };
    if let Some(err) = json.get("error").and_then(|v| v.as_str()) {
        on_event(StreamingEvent::Error {
            message: err.to_string(),
        });
        return;
    }
    let Some(typ) = json.get("type").and_then(|v| v.as_str()) else {
        return;
    };
    match typ {
        "Turn" => handle_turn(&json, state, on_event),
        "Termination" | "Begin" => {}
        _ => {}
    }
}

fn handle_turn(
    json: &Value,
    state: &mut AaiState,
    on_event: &(dyn Fn(StreamingEvent) + Send + Sync),
) {
    let transcript = json
        .get("transcript")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if transcript.is_empty() {
        return;
    }
    let end_of_turn = json
        .get("end_of_turn")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let turn_is_formatted = json
        .get("turn_is_formatted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let turn_order = json.get("turn_order").and_then(|v| v.as_i64());

    if end_of_turn && (turn_is_formatted || state.last_committed_turn != turn_order) {
        if !state.final_text.is_empty() {
            state.final_text.push(' ');
        }
        state.final_text.push_str(transcript);
        state.last_committed_turn = turn_order;
        on_event(StreamingEvent::Committed {
            text: state.final_text.trim().to_string(),
        });
    } else if !end_of_turn {
        let preview = if state.final_text.is_empty() {
            transcript.to_string()
        } else {
            format!("{} {}", state.final_text, transcript)
        };
        on_event(StreamingEvent::Partial { text: preview });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collector() -> (
        std::sync::Arc<std::sync::Mutex<Vec<StreamingEvent>>>,
        Box<dyn Fn(StreamingEvent) + Send + Sync>,
    ) {
        let acc = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let acc_clone = acc.clone();
        let cb: Box<dyn Fn(StreamingEvent) + Send + Sync> = Box::new(move |e| {
            acc_clone.lock().unwrap().push(e);
        });
        (acc, cb)
    }

    #[test]
    fn streaming_model_resolution() {
        assert_eq!(streaming_model("universal-3-pro", Some("fr")), "u3-rt-pro");
        assert_eq!(
            streaming_model("universal-streaming", Some("en")),
            "universal-streaming-english"
        );
        assert_eq!(
            streaming_model("universal-streaming", Some("fr")),
            "universal-streaming-multilingual"
        );
        assert_eq!(
            streaming_model("universal-streaming", Some("auto")),
            "universal-streaming-multilingual"
        );
        assert_eq!(
            streaming_model("universal-streaming", None),
            "universal-streaming-multilingual"
        );
    }

    #[test]
    fn turn_partial_then_committed() {
        let mut state = AaiState::default();
        let (acc, cb) = collector();
        handle_text(
            r#"{"type":"Turn","transcript":"hello","end_of_turn":false,"turn_order":1}"#,
            &mut state,
            cb.as_ref(),
        );
        handle_text(
            r#"{"type":"Turn","transcript":"hello world","end_of_turn":true,"turn_is_formatted":true,"turn_order":1}"#,
            &mut state,
            cb.as_ref(),
        );
        let events = acc.lock().unwrap().clone();
        assert!(matches!(events.first(), Some(StreamingEvent::Partial { .. })));
        match events.last().unwrap() {
            StreamingEvent::Committed { text } => assert_eq!(text, "hello world"),
            other => panic!("expected Committed, got {:?}", other),
        }
        assert_eq!(state.last_committed_turn, Some(1));
    }

    #[test]
    fn duplicate_turn_not_recommitted() {
        let mut state = AaiState::default();
        let (acc, cb) = collector();
        // Premier end_of_turn formatte : commit OK
        handle_text(
            r#"{"type":"Turn","transcript":"a","end_of_turn":true,"turn_is_formatted":true,"turn_order":1}"#,
            &mut state,
            cb.as_ref(),
        );
        // Second end_of_turn meme turn, NON formatte : ignore (cf condition LLMkit)
        handle_text(
            r#"{"type":"Turn","transcript":"a","end_of_turn":true,"turn_is_formatted":false,"turn_order":1}"#,
            &mut state,
            cb.as_ref(),
        );
        let committed = acc
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, StreamingEvent::Committed { .. }))
            .count();
        assert_eq!(committed, 1);
        assert_eq!(state.final_text.trim(), "a");
    }

    #[test]
    fn keyterms_normalization() {
        let raw = vec![
            "Docker".into(),
            "docker".into(),
            "  ".into(),
            "x".repeat(60),
            "this is more than six words and should be dropped".into(),
        ];
        let n = normalize_keyterms(&raw);
        assert_eq!(n, vec!["Docker".to_string()]);
    }
}
