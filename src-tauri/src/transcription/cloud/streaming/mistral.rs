// Mistral Voxtral realtime.
//
// Reference VoiceInk : LLMkit MistralStreamingClient.swift.
// WSS : wss://api.mistral.ai/v1/audio/transcriptions/realtime?model=voxtral-mini-transcribe-realtime-2602
// Header : Authorization: Bearer
// Handshake : {"type":"session.created"}.
// Apres handshake : envoyer session.update avec audio_format pcm_s16le 16000.
// Audio : {"type":"input_audio.append","audio":"<b64>"}.
// Commit : {"type":"input_audio.end"}.
// Events :
//   - transcription.text.delta -> accumuler + Partial(accumule)
//   - transcription.done -> Committed(accumule) + reset
//   - transcription.language / session.updated -> ignore
//   - error -> Error (extract error.message/error.detail/error/message)

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use super::session::{
    connect_streaming_socket, drain_ws_messages, i16_to_base64, StreamingChannels, StreamingConfig,
    StreamingEvent, StreamingMessage, StreamingProvider,
};

pub struct MistralStreaming;

#[async_trait]
impl StreamingProvider for MistralStreaming {
    fn id(&self) -> &'static str {
        "mistral"
    }

    async fn run(
        &self,
        api_key: String,
        config: StreamingConfig,
        channels: StreamingChannels,
        on_event: Box<dyn Fn(StreamingEvent) + Send + Sync>,
    ) -> Result<String> {
        // VoiceInk hard-code voxtral-mini-transcribe-realtime-2602
        // (MistralStreamingProvider L34).
        let model = if config.model.is_empty() {
            "voxtral-mini-transcribe-realtime-2602".to_string()
        } else {
            config.model.clone()
        };
        let url = format!(
            "wss://api.mistral.ai/v1/audio/transcriptions/realtime?model={}",
            urlencoding::encode(&model)
        );

        let mut req = url.into_client_request()?;
        req.headers_mut()
            .insert("Authorization", format!("Bearer {api_key}").parse()?);

        let socket = connect_streaming_socket(req).await?;
        let super::session::StreamingSocket {
            mut write,
            mut read,
        } = socket;

        // Handshake : attend session.created.
        loop {
            match read.next().await {
                Ok(Some(StreamingMessage::Text(t))) => {
                    let json: Value = serde_json::from_str(&t)?;
                    match json.get("type").and_then(|v| v.as_str()) {
                        Some("session.created") => break,
                        Some("error") => {
                            return Err(anyhow!("Mistral handshake: {}", extract_error(&json)));
                        }
                        _ => continue,
                    }
                }
                Ok(Some(_)) => continue,
                Err(e) => return Err(anyhow!("ws read: {e}")),
                Ok(None) => return Err(anyhow!("ws closed during handshake")),
            }
        }

        // session.update
        let update = json!({
            "type": "session.update",
            "session": {
                "audio_format": { "encoding": "pcm_s16le", "sample_rate": 16000 }
            }
        });
        write.send_text(update.to_string()).await?;

        on_event(StreamingEvent::SessionStarted);

        let StreamingChannels {
            mut audio_rx,
            mut finalize_rx,
        } = channels;

        let mut accumulated = String::new();
        let mut committed_text = String::new();

        loop {
            tokio::select! {
                biased;
                _ = &mut finalize_rx => {
                    while let Ok(chunk) = audio_rx.try_recv() {
                        let msg = json!({ "type": "input_audio.append", "audio": i16_to_base64(&chunk) });
                        let _ = write.send_text(msg.to_string()).await;
                    }
                    let _ = write.send_text(json!({ "type": "input_audio.end" }).to_string()).await;
                    drain_ws_messages(read.as_mut(), std::time::Duration::from_secs(5), |text| {
                        handle_text(text, &mut accumulated, &mut committed_text, &on_event)
                    }).await;
                    let final_text = if committed_text.is_empty() && !accumulated.is_empty() {
                        committed_text.push_str(&accumulated);
                        committed_text.trim().to_string()
                    } else {
                        committed_text.trim().to_string()
                    };
                    let _ = write.close().await;
                    return Ok(final_text);
                }
                chunk = audio_rx.recv() => {
                    match chunk {
                        Some(c) => {
                            let msg = json!({ "type": "input_audio.append", "audio": i16_to_base64(&c) });
                            if let Err(e) = write.send_text(msg.to_string()).await {
                                return Err(anyhow!("ws send: {e}"));
                            }
                        }
                        None => return Ok(committed_text),
                    }
                }
                msg = read.next() => {
                    match msg {
                        Ok(Some(StreamingMessage::Text(t))) => handle_text(&t, &mut accumulated, &mut committed_text, &on_event),
                        Ok(Some(StreamingMessage::Close)) => return Ok(committed_text),
                        Ok(Some(_)) => {}
                        Err(e) => return Err(anyhow!("ws read: {e}")),
                        Ok(None) => return Ok(committed_text),
                    }
                }
            }
        }
    }
}

fn handle_text(
    t: &str,
    accumulated: &mut String,
    committed: &mut String,
    on_event: &(dyn Fn(StreamingEvent) + Send + Sync),
) {
    let Ok(json) = serde_json::from_str::<Value>(t) else {
        return;
    };
    match json.get("type").and_then(|v| v.as_str()) {
        Some("transcription.text.delta") => {
            if let Some(delta) = json.get("text").and_then(|v| v.as_str()) {
                accumulated.push_str(delta);
                on_event(StreamingEvent::Partial {
                    text: accumulated.clone(),
                });
            }
        }
        Some("transcription.done") => {
            if !accumulated.trim().is_empty() {
                if !committed.is_empty() {
                    committed.push(' ');
                }
                committed.push_str(accumulated.trim());
                on_event(StreamingEvent::Committed {
                    text: committed.clone(),
                });
                accumulated.clear();
            }
        }
        Some("error") => {
            on_event(StreamingEvent::Error {
                message: extract_error(&json),
            });
        }
        _ => {}
    }
}

fn extract_error(json: &Value) -> String {
    if let Some(msg) = json.pointer("/error/message").and_then(|v| v.as_str()) {
        return msg.to_string();
    }
    if let Some(msg) = json.pointer("/error/detail").and_then(|v| v.as_str()) {
        return msg.to_string();
    }
    if let Some(msg) = json.get("error").and_then(|v| v.as_str()) {
        return msg.to_string();
    }
    if let Some(msg) = json.get("message").and_then(|v| v.as_str()) {
        return msg.to_string();
    }
    "erreur provider".to_string()
}
