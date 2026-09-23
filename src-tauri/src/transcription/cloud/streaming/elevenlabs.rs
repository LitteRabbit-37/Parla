// ElevenLabs Scribe v2 realtime.
//
// Reference VoiceInk : LLMkit ElevenLabsStreamingClient.swift.
// WSS : wss://api.elevenlabs.io/v1/speech-to-text/realtime?model_id=scribe_v2_realtime
//       &audio_format=pcm_16000&commit_strategy=vad&no_verbatim=true
//       [&language_code=...][&keyterms=...]*
//       no_verbatim : VoiceInk "Enable no-verbatim for 11labs Scribe V2"
//       (#808), le flux retire les hesitations comme le batch.
//       keyterms   : vocabulaire personnalise, un parametre par terme
//       (LLMkit ElevenLabsStreamingClient.normalizedKeyterms : trim,
//       <= 20 caracteres, sans < > { } [ ] \, dedup, 50 max).
// Header : xi-api-key
// Handshake : attendre {"message_type":"session_started"}.
// Audio : JSON { message_type: input_audio_chunk, audio_base_64: <b64>,
//                commit: false, sample_rate: 16000 }.
// Commit : meme structure avec audio_base_64="" et commit:true.
// Events : partial_transcript / committed_transcript[_with_timestamps] / error*.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

use super::session::{
    connect_ws, drain_ws_messages, i16_to_base64, StreamingChannels, StreamingConfig,
    StreamingEvent, StreamingProvider,
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
            "wss://api.elevenlabs.io/v1/speech-to-text/realtime?model_id=scribe_v2_realtime&audio_format=pcm_16000&commit_strategy=vad&no_verbatim=true",
        );
        if let Some(lang) = config.language.as_deref() {
            if !lang.is_empty() && lang != "auto" {
                url.push_str(&format!("&language_code={}", urlencoding::encode(lang)));
            }
        }
        for term in normalize_keyterms(&config.custom_vocabulary) {
            url.push_str(&format!("&keyterms={}", urlencoding::encode(&term)));
        }

        let mut req = url.into_client_request()?;
        req.headers_mut().insert("xi-api-key", api_key.parse()?);

        let ws_stream = connect_ws(req).await?;
        let (mut write, mut read) = ws_stream.split();

        // Handshake : attend session_started.
        loop {
            match read.next().await {
                Some(Ok(Message::Text(t))) => {
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
                Some(Ok(_)) => continue,
                Some(Err(e)) => return Err(anyhow!("ws read: {e}")),
                None => return Err(anyhow!("ws closed during handshake")),
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
                        let _ = write.send(Message::Text(msg.to_string().into())).await;
                    }
                    let commit = json!({
                        "message_type": "input_audio_chunk",
                        "audio_base_64": "",
                        "commit": true,
                        "sample_rate": 16000,
                    });
                    let _ = write.send(Message::Text(commit.to_string().into())).await;
                    let final_text = drain_after_commit(&mut read, &mut committed_text, &on_event).await;
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
                            if let Err(e) = write.send(Message::Text(msg.to_string().into())).await {
                                return Err(anyhow!("ws send: {e}"));
                            }
                        }
                        None => return Ok(committed_text),
                    }
                }
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(t))) => handle_text(&t, &mut committed_text, &on_event),
                        Some(Ok(Message::Close(_))) => return Ok(committed_text),
                        Some(Ok(_)) => {}
                        Some(Err(e)) => return Err(anyhow!("ws read: {e}")),
                        None => return Ok(committed_text),
                    }
                }
            }
        }
    }
}

async fn drain_after_commit<S>(
    read: &mut S,
    committed: &mut String,
    on_event: &(dyn Fn(StreamingEvent) + Send + Sync),
) -> String
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    drain_ws_messages(read, std::time::Duration::from_secs(5), |t| {
        handle_text(t, committed, on_event)
    })
    .await;
    committed.trim().to_string()
}

/// LLMkit ElevenLabsStreamingClient.normalizedKeyterms : trim, 20 caracteres
/// max, sans < > { } [ ] \, dedup insensible a la casse, 50 termes max.
fn normalize_keyterms(raw: &[String]) -> Vec<String> {
    const UNSUPPORTED: [char; 7] = ['<', '>', '{', '}', '[', ']', '\\'];
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for term in raw {
        let trimmed = term.trim();
        if trimmed.is_empty()
            || trimmed.chars().count() > 20
            || trimmed.chars().any(|c| UNSUPPORTED.contains(&c))
        {
            continue;
        }
        if !seen.insert(trimmed.to_lowercase()) {
            continue;
        }
        out.push(trimmed.to_string());
        if out.len() == 50 {
            break;
        }
    }
    out
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

#[cfg(test)]
mod keyterm_tests {
    use super::normalize_keyterms;

    #[test]
    fn keyterms_follow_llmkit_rules() {
        let raw: Vec<String> = vec![
            " Docker ".into(),
            "docker".into(),
            "a<b".into(),
            "x".repeat(21),
            "".into(),
            "Kubernetes".into(),
        ];
        assert_eq!(
            normalize_keyterms(&raw),
            vec!["Docker".to_string(), "Kubernetes".to_string()]
        );
        let many: Vec<String> = (0..80).map(|i| format!("t{i}")).collect();
        assert_eq!(normalize_keyterms(&many).len(), 50);
    }
}
