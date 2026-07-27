// ElevenLabs Scribe v2 realtime.
//
// Reference VoiceInk : LLMkit ElevenLabsStreamingClient.swift.
// WSS : wss://api.elevenlabs.io/v1/speech-to-text/realtime?model_id=scribe_v2_realtime
//       &audio_format=pcm_16000&commit_strategy=vad[&language_code=...]
// Header : xi-api-key
// Handshake : attendre {"message_type":"session_started"}.
// Audio : JSON { message_type: input_audio_chunk, audio_base_64: <b64>,
//                commit: false, sample_rate: 16000 }.
// Commit : meme structure avec audio_base_64="" et commit:true.
// Events : partial_transcript / committed_transcript[_with_timestamps] / error*.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use super::session::{
    connect_streaming_socket, i16_to_base64, StreamingChannels, StreamingConfig, StreamingEvent,
    StreamingMessage, StreamingProvider, StreamingSocketRead,
};

pub struct ElevenLabsStreaming;

#[async_trait]
impl StreamingProvider for ElevenLabsStreaming {
    fn id(&self) -> &'static str {
        "elevenlabs"
    }

    async fn run(
        &self,
        api_key: String,
        config: StreamingConfig,
        channels: StreamingChannels,
        on_event: Box<dyn Fn(StreamingEvent) + Send + Sync>,
    ) -> Result<String> {
        // VoiceInk hard-code scribe_v2_realtime (ElevenLabsStreamingProvider L34).
        // On respecte ca meme si un autre model_id est passe.
        let _ = config.model;
        let mut url = String::from(
            "wss://api.elevenlabs.io/v1/speech-to-text/realtime?model_id=scribe_v2_realtime&audio_format=pcm_16000&commit_strategy=vad",
        );
        if let Some(lang) = config.language.as_deref() {
            if !lang.is_empty() && lang != "auto" {
                url.push_str(&format!("&language_code={}", urlencoding::encode(lang)));
            }
        }

        let mut req = url.into_client_request()?;
        req.headers_mut().insert("xi-api-key", api_key.parse()?);

        let socket = connect_streaming_socket(req).await?;
        let super::session::StreamingSocket {
            mut write,
            mut read,
        } = socket;

        // Handshake : attend session_started.
        loop {
            match read.next().await {
                Ok(Some(StreamingMessage::Text(t))) => {
                    let json: Value = serde_json::from_str(&t)?;
                    match json.get("message_type").and_then(|v| v.as_str()) {
                        Some("session_started") => break,
                        Some("error") | Some("auth_error") => {
                            let msg = json
                                .get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("handshake error")
                                .to_string();
                            return Err(anyhow!("ElevenLabs handshake: {msg}"));
                        }
                        _ => continue,
                    }
                }
                Ok(Some(_)) => continue,
                Err(e) => return Err(anyhow!("ws read: {e}")),
                Ok(None) => return Err(anyhow!("ws closed during handshake")),
            }
        }
        on_event(StreamingEvent::SessionStarted);

        let StreamingChannels {
            mut audio_rx,
            mut finalize_rx,
        } = channels;

        let mut committed_text = String::new();

        loop {
            tokio::select! {
                biased;
                _ = &mut finalize_rx => {
                    while let Ok(chunk) = audio_rx.try_recv() {
                        let msg = json!({
                            "message_type": "input_audio_chunk",
                            "audio_base_64": i16_to_base64(&chunk),
                            "commit": false,
                            "sample_rate": 16000,
                        });
                        let _ = write.send_text(msg.to_string()).await;
                    }
                    let commit = json!({
                        "message_type": "input_audio_chunk",
                        "audio_base_64": "",
                        "commit": true,
                        "sample_rate": 16000,
                    });
                    let _ = write.send_text(commit.to_string()).await;
                    let final_text = drain_after_commit(read.as_mut(), &mut committed_text, &on_event).await;
                    let _ = write.close().await;
                    return Ok(final_text);
                }
                chunk = audio_rx.recv() => {
                    match chunk {
                        Some(c) => {
                            let msg = json!({
                                "message_type": "input_audio_chunk",
                                "audio_base_64": i16_to_base64(&c),
                                "commit": false,
                                "sample_rate": 16000,
                            });
                            if let Err(e) = write.send_text(msg.to_string()).await {
                                return Err(anyhow!("ws send: {e}"));
                            }
                        }
                        None => return Ok(committed_text),
                    }
                }
                msg = read.next() => {
                    match msg {
                        Ok(Some(StreamingMessage::Text(t))) => handle_text(&t, &mut committed_text, &on_event),
                        Ok(Some(StreamingMessage::Close)) | Ok(None) => return Ok(committed_text),
                        Ok(Some(_)) => {}
                        Err(e) => return Err(anyhow!("ws read: {e}")),
                    }
                }
            }
        }
    }
}

async fn drain_after_commit(
    read: &mut dyn StreamingSocketRead,
    committed: &mut String,
    on_event: &(dyn Fn(StreamingEvent) + Send + Sync),
) -> String {
    let timeout = std::time::Duration::from_secs(5);
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        match tokio::time::timeout(remaining, read.next()).await {
            Ok(Ok(Some(StreamingMessage::Text(text)))) => handle_text(&text, committed, on_event),
            Ok(Ok(Some(StreamingMessage::Binary(_))))
            | Ok(Ok(Some(StreamingMessage::Close)))
            | Ok(Ok(None))
            | Ok(Err(_))
            | Err(_) => break,
        }
    }
    committed.trim().to_string()
}

fn handle_text(t: &str, committed: &mut String, on_event: &(dyn Fn(StreamingEvent) + Send + Sync)) {
    let Ok(json) = serde_json::from_str::<Value>(t) else {
        return;
    };
    match json.get("message_type").and_then(|v| v.as_str()) {
        Some("partial_transcript") => {
            if let Some(text) = json.get("text").and_then(|v| v.as_str()) {
                on_event(StreamingEvent::Partial {
                    text: text.to_string(),
                });
            }
        }
        Some("committed_transcript") | Some("committed_transcript_with_timestamps") => {
            if let Some(text) = json.get("text").and_then(|v| v.as_str()) {
                if !committed.is_empty() {
                    committed.push(' ');
                }
                committed.push_str(text);
                on_event(StreamingEvent::Committed {
                    text: committed.clone(),
                });
            }
        }
        Some("error")
        | Some("auth_error")
        | Some("quota_exceeded")
        | Some("rate_limited")
        | Some("resource_exhausted")
        | Some("session_time_limit_exceeded")
        | Some("input_error")
        | Some("chunk_size_exceeded")
        | Some("transcriber_error") => {
            let msg = json
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("erreur provider")
                .to_string();
            on_event(StreamingEvent::Error { message: msg });
        }
        _ => {}
    }
}
