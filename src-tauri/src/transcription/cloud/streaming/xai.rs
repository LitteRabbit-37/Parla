// xAI Grok streaming temps reel via WebSocket.
//
// Reference VoiceInk : LLMkit XAIStreamingClient.swift.
// WSS    : wss://api.x.ai/v1/stt
// Query  : sample_rate=16000, encoding=pcm, interim_results=true,
//          endpointing=800 (800 ms de silence = fin d'enonce, defaut 10 ms
//          coupe a la moindre micro-pause), [language=<code>] si != auto.
// Auth   : Authorization: Bearer <apiKey> en header WS.
// Audio  : frames binaires Int16 little-endian.
// Init   : on attend un message `transcript.created` (ou `error`) avant
//          d'emettre SessionStarted.
// Commit : message texte JSON `{"type":"audio.done"}`.
// Events :
//   - "transcript.partial" {text, is_final, speech_final}
//       speech_final=true -> commit l'utterance courante (locked + text)
//       is_final=true     -> accumule dans locked, emit Partial (final + locked)
//       sinon             -> Partial (final + locked + text), preview
//   - "transcript.done" {text}    -> Committed (cumul)
//   - "error" {message}           -> Error

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use super::session::{
    connect_streaming_socket, i16_to_le_bytes, StreamingChannels, StreamingConfig, StreamingEvent,
    StreamingMessage, StreamingProvider, StreamingSocketRead,
};

pub struct XaiStreaming;

#[async_trait]
impl StreamingProvider for XaiStreaming {
    fn id(&self) -> &'static str {
        "xai"
    }

    async fn run(
        &self,
        api_key: String,
        config: StreamingConfig,
        channels: StreamingChannels,
        on_event: Box<dyn Fn(StreamingEvent) + Send + Sync>,
    ) -> Result<String> {
        let mut url = String::from(
            "wss://api.x.ai/v1/stt?sample_rate=16000&encoding=pcm&interim_results=true&endpointing=800",
        );
        if let Some(lang) = config.language.as_deref() {
            if !lang.is_empty() && lang != "auto" {
                url.push_str("&language=");
                url.push_str(lang);
            }
        }

        let mut req = url
            .into_client_request()
            .map_err(|e| anyhow!("ws url parse: {e}"))?;
        req.headers_mut().insert(
            "Authorization",
            format!("Bearer {api_key}")
                .parse()
                .map_err(|e| anyhow!("auth header: {e}"))?,
        );

        let socket = connect_streaming_socket(req).await?;
        let super::session::StreamingSocket {
            mut write,
            mut read,
        } = socket;

        // Handshake : on attend `transcript.created` (ou error fatal).
        match read.next().await {
            Ok(Some(StreamingMessage::Text(t))) => {
                if let Ok(json) = serde_json::from_str::<Value>(&t) {
                    match json.get("type").and_then(|v| v.as_str()) {
                        Some("error") => {
                            let msg = json
                                .get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("xAI handshake error")
                                .to_string();
                            return Err(anyhow!(msg));
                        }
                        Some("transcript.created") => {}
                        _ => {}
                    }
                }
            }
            Ok(Some(_)) => {}
            Err(e) => return Err(anyhow!("ws read: {e}")),
            Ok(None) => return Err(anyhow!("ws handshake closed prematurement")),
        }
        on_event(StreamingEvent::SessionStarted);

        let StreamingChannels {
            mut audio_rx,
            mut finalize_rx,
        } = channels;

        let mut state = XaiState::default();

        loop {
            tokio::select! {
                biased;
                _ = &mut finalize_rx => {
                    while let Ok(chunk) = audio_rx.try_recv() {
                        let _ = write.send_binary(i16_to_le_bytes(&chunk)).await;
                    }
                    let _ = write
                        .send_text(json!({ "type": "audio.done" }).to_string())
                        .await;
                    drain(&mut *read, &mut state, &on_event).await;
                    let _ = write.close().await;
                    return Ok(state.final_text.trim().to_string());
                }
                chunk = audio_rx.recv() => {
                    match chunk {
                        Some(c) => {
                            if let Err(e) = write
                                .send_binary(i16_to_le_bytes(&c))
                                .await
                            {
                                return Err(anyhow!("ws send: {e}"));
                            }
                        }
                        None => return Ok(state.final_text.trim().to_string()),
                    }
                }
                msg = read.next() => {
                    match msg {
                        Ok(Some(StreamingMessage::Text(t))) => handle_text(&t, &mut state, &on_event),
                        Ok(Some(StreamingMessage::Close)) => return Ok(state.final_text.trim().to_string()),
                        Ok(Some(StreamingMessage::Binary(_))) => {}
                        Err(e) => return Err(anyhow!("ws read: {e}")),
                        Ok(None) => return Ok(state.final_text.trim().to_string()),
                    }
                }
            }
        }
    }
}

async fn drain(
    read: &mut (dyn StreamingSocketRead + Send),
    state: &mut XaiState,
    on_event: &(dyn Fn(StreamingEvent) + Send + Sync),
) {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        match tokio::time::timeout(remaining, read.next()).await {
            Ok(Ok(Some(StreamingMessage::Text(t)))) => handle_text(&t, state, on_event),
            Ok(Ok(Some(StreamingMessage::Close))) | Ok(Ok(None)) | Err(_) => break,
            _ => {}
        }
    }
}

#[derive(Default)]
struct XaiState {
    /// Texte cumule des utterances commitees (separateur " ").
    final_text: String,
    /// Buffer de l'utterance en cours : concatene les is_final reçus avant
    /// le speech_final qui clot l'utterance.
    locked: String,
}

fn handle_text(t: &str, state: &mut XaiState, on_event: &(dyn Fn(StreamingEvent) + Send + Sync)) {
    let Ok(json) = serde_json::from_str::<Value>(t) else {
        return;
    };
    let Some(typ) = json.get("type").and_then(|v| v.as_str()) else {
        return;
    };

    match typ {
        "transcript.partial" => {
            let Some(text) = json.get("text").and_then(|v| v.as_str()) else {
                return;
            };
            if text.trim().is_empty() {
                return;
            }
            let is_final = json
                .get("is_final")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let speech_final = json
                .get("speech_final")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if speech_final {
                if !state.locked.is_empty() {
                    append_with_space(&mut state.final_text, &state.locked);
                    state.locked.clear();
                }
                append_with_space(&mut state.final_text, text);
                on_event(StreamingEvent::Committed {
                    text: state.final_text.trim().to_string(),
                });
            } else if is_final {
                if !state.locked.is_empty() {
                    state.locked.push(' ');
                }
                state.locked.push_str(text);
                on_event(StreamingEvent::Partial {
                    text: combine(&state.final_text, &state.locked, ""),
                });
            } else {
                on_event(StreamingEvent::Partial {
                    text: combine(&state.final_text, &state.locked, text),
                });
            }
        }
        "transcript.done" => {
            if let Some(text) = json.get("text").and_then(|v| v.as_str()) {
                if !text.trim().is_empty() {
                    append_with_space(&mut state.final_text, text);
                }
            }
            state.locked.clear();
            on_event(StreamingEvent::Committed {
                text: state.final_text.trim().to_string(),
            });
        }
        "error" => {
            let msg = json
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("xAI streaming error")
                .to_string();
            on_event(StreamingEvent::Error { message: msg });
        }
        _ => {}
    }
}

fn append_with_space(dst: &mut String, frag: &str) {
    if frag.is_empty() {
        return;
    }
    if !dst.is_empty() {
        dst.push(' ');
    }
    dst.push_str(frag);
}

fn combine(final_text: &str, locked: &str, partial: &str) -> String {
    let mut out = String::with_capacity(final_text.len() + locked.len() + partial.len() + 2);
    out.push_str(final_text);
    if !out.is_empty() && !locked.is_empty() {
        out.push(' ');
    }
    out.push_str(locked);
    if !out.is_empty() && !partial.is_empty() {
        out.push(' ');
    }
    out.push_str(partial);
    out
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

    fn last_partial(events: &[StreamingEvent]) -> Option<String> {
        events.iter().rev().find_map(|e| match e {
            StreamingEvent::Partial { text } => Some(text.clone()),
            _ => None,
        })
    }

    fn last_committed(events: &[StreamingEvent]) -> Option<String> {
        events.iter().rev().find_map(|e| match e {
            StreamingEvent::Committed { text } => Some(text.clone()),
            _ => None,
        })
    }

    #[test]
    fn partial_then_speech_final_commits_full_utterance() {
        let mut state = XaiState::default();
        let (acc, cb) = collector();
        handle_text(
            r#"{"type":"transcript.partial","text":"hello","is_final":false,"speech_final":false}"#,
            &mut state,
            cb.as_ref(),
        );
        handle_text(
            r#"{"type":"transcript.partial","text":"hello","is_final":true,"speech_final":false}"#,
            &mut state,
            cb.as_ref(),
        );
        handle_text(
            r#"{"type":"transcript.partial","text":"world","is_final":false,"speech_final":true}"#,
            &mut state,
            cb.as_ref(),
        );
        let events = acc.lock().unwrap().clone();
        assert_eq!(last_partial(&events).unwrap(), "hello");
        assert_eq!(last_committed(&events).unwrap(), "hello world");
        assert_eq!(state.final_text.trim(), "hello world");
        assert!(state.locked.is_empty());
    }

    #[test]
    fn multiple_utterances_accumulate() {
        let mut state = XaiState::default();
        let (_, cb) = collector();
        handle_text(
            r#"{"type":"transcript.partial","text":"first","is_final":false,"speech_final":true}"#,
            &mut state,
            cb.as_ref(),
        );
        handle_text(
            r#"{"type":"transcript.partial","text":"second","is_final":false,"speech_final":true}"#,
            &mut state,
            cb.as_ref(),
        );
        assert_eq!(state.final_text.trim(), "first second");
    }

    #[test]
    fn error_emits_error_event() {
        let mut state = XaiState::default();
        let (acc, cb) = collector();
        handle_text(
            r#"{"type":"error","message":"oops"}"#,
            &mut state,
            cb.as_ref(),
        );
        let events = acc.lock().unwrap().clone();
        assert!(matches!(events[0], StreamingEvent::Error { .. }));
    }
}
