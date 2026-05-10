// Cartesia Ink Whisper streaming temps reel via WebSocket.
//
// Reference VoiceInk : LLMkit CartesiaStreamingClient.swift.
// WSS    : wss://api.cartesia.ai/stt/websocket
// Query  : model, language ("en" par defaut puisque pas d'auto-detect),
//          encoding=pcm_s16le, sample_rate=16000, cartesia_version=2026-03-01.
// Auth   : header X-API-Key (PAS de Bearer).
// Audio  : frames binaires Int16 little-endian.
// Init   : pas de handshake, on emit SessionStarted des que le WS est ouvert.
// Commit : message texte "finalize" (text frame, pas JSON).
// Close  : message texte "done" puis fermeture du WS.
// Events :
//   - "transcript" {text, is_final}
//       is_final=true -> Committed (texte cumule)
//       sinon          -> Partial (final + " " + text)
//   - "error" {message ou title}
//   - "flush_done" / "done" -> ignores

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

use super::session::{
    connect_ws, drain_ws_messages, i16_to_le_bytes, StreamingChannels, StreamingConfig,
    StreamingEvent, StreamingProvider,
};

pub struct CartesiaStreaming;

const CARTESIA_VERSION: &str = "2026-03-01";

#[async_trait]
impl StreamingProvider for CartesiaStreaming {
    fn id(&self) -> &'static str {
        "cartesia"
    }

    async fn run(
        &self,
        api_key: String,
        config: StreamingConfig,
        channels: StreamingChannels,
        on_event: Box<dyn Fn(StreamingEvent) + Send + Sync>,
    ) -> Result<String> {
        // Cartesia n'a pas d'auto-detect : on retombe sur "en" par defaut
        // (cf LLMkit CartesiaStreamingClient L33).
        let lang = match config.language.as_deref() {
            Some(l) if !l.is_empty() && l != "auto" => l,
            _ => "en",
        };
        let url = format!(
            "wss://api.cartesia.ai/stt/websocket?model={model}&language={lang}&encoding=pcm_s16le&sample_rate=16000&cartesia_version={ver}",
            model = config.model,
            ver = CARTESIA_VERSION,
        );

        let mut req = url
            .into_client_request()
            .map_err(|e| anyhow!("ws url parse: {e}"))?;
        req.headers_mut().insert(
            "X-API-Key",
            api_key
                .parse()
                .map_err(|e| anyhow!("X-API-Key header: {e}"))?,
        );

        let ws_stream = connect_ws(req).await?;
        let (mut write, mut read) = ws_stream.split();

        on_event(StreamingEvent::SessionStarted);

        let StreamingChannels {
            mut audio_rx,
            mut finalize_rx,
        } = channels;

        let mut state = CartesiaState::default();

        loop {
            tokio::select! {
                biased;
                _ = &mut finalize_rx => {
                    while let Ok(chunk) = audio_rx.try_recv() {
                        let _ = write.send(Message::Binary(i16_to_le_bytes(&chunk).into())).await;
                    }
                    let _ = write.send(Message::Text("finalize".into())).await;
                    drain_ws_messages(&mut read, std::time::Duration::from_secs(5), |t| {
                        handle_text(t, &mut state, &on_event);
                    })
                    .await;
                    let _ = write.send(Message::Text("done".into())).await;
                    let _ = write.close().await;
                    return Ok(state.final_text.trim().to_string());
                }
                chunk = audio_rx.recv() => {
                    match chunk {
                        Some(c) => {
                            if let Err(e) = write
                                .send(Message::Binary(i16_to_le_bytes(&c).into()))
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

#[derive(Default)]
struct CartesiaState {
    final_text: String,
}

fn handle_text(
    t: &str,
    state: &mut CartesiaState,
    on_event: &(dyn Fn(StreamingEvent) + Send + Sync),
) {
    let Ok(json) = serde_json::from_str::<Value>(t) else {
        return;
    };
    let Some(typ) = json.get("type").and_then(|v| v.as_str()) else {
        return;
    };

    match typ {
        "transcript" => {
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
            if is_final {
                if !state.final_text.is_empty() {
                    state.final_text.push(' ');
                }
                state.final_text.push_str(text);
                on_event(StreamingEvent::Committed {
                    text: state.final_text.trim().to_string(),
                });
            } else {
                let preview = if state.final_text.is_empty() {
                    text.to_string()
                } else {
                    format!("{} {}", state.final_text, text)
                };
                on_event(StreamingEvent::Partial { text: preview });
            }
        }
        "error" => {
            let msg = json
                .get("message")
                .and_then(|v| v.as_str())
                .or_else(|| json.get("title").and_then(|v| v.as_str()))
                .unwrap_or("Cartesia streaming error")
                .to_string();
            on_event(StreamingEvent::Error { message: msg });
        }
        // flush_done, done : pas d'effet client.
        _ => {}
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
    fn partial_then_final_accumulates() {
        let mut state = CartesiaState::default();
        let (acc, cb) = collector();
        handle_text(
            r#"{"type":"transcript","text":"hello","is_final":false}"#,
            &mut state,
            cb.as_ref(),
        );
        handle_text(
            r#"{"type":"transcript","text":"hello world","is_final":true}"#,
            &mut state,
            cb.as_ref(),
        );
        handle_text(
            r#"{"type":"transcript","text":"how are","is_final":false}"#,
            &mut state,
            cb.as_ref(),
        );
        handle_text(
            r#"{"type":"transcript","text":"how are you","is_final":true}"#,
            &mut state,
            cb.as_ref(),
        );

        let events = acc.lock().unwrap().clone();
        let last_committed = events
            .iter()
            .rev()
            .find_map(|e| match e {
                StreamingEvent::Committed { text } => Some(text.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(last_committed, "hello world how are you");
        assert_eq!(state.final_text.trim(), "hello world how are you");
    }

    #[test]
    fn error_message_or_title() {
        let mut state = CartesiaState::default();
        let (acc, cb) = collector();
        handle_text(
            r#"{"type":"error","title":"Bad Request"}"#,
            &mut state,
            cb.as_ref(),
        );
        let events = acc.lock().unwrap().clone();
        match &events[0] {
            StreamingEvent::Error { message } => assert_eq!(message, "Bad Request"),
            _ => panic!("expected Error"),
        }
    }
}
