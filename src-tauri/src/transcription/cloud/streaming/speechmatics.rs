// Speechmatics realtime enhanced.
//
// Reference VoiceInk : LLMkit SpeechmaticsStreamingClient.swift.
// WSS : wss://eu2.rt.speechmatics.com/v2
// Header : Authorization: Bearer
// StartRecognition envoye immediatement :
//   { message: "StartRecognition",
//     audio_format: { type: "raw", encoding: "pcm_s16le", sample_rate: 16000 },
//     transcription_config: { language, enable_partials: true,
//       operating_point: "enhanced", max_delay: 2.0, max_delay_mode: "flexible"
//       [, additional_vocab: [{content:term}] ] } }
// Handshake : attendre RecognitionStarted (10s) - Error -> LLMKitError(400).
// Audio : frames binaires, incrementer seqNo.
// Commit : {"message":"EndOfStream","last_seq_no":N}
// Messages :
//   - AudioAdded : log
//   - AddPartialTranscript -> .partial(accumulatedFinal + cleaned(transcript))
//   - AddTranscript -> append a accumulatedFinalText
//   - EndOfTranscript -> .committed(clean(accumulated))
//   - Error -> .error(reason)

use std::time::Duration;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use super::session::{
    connect_streaming_socket, i16_to_le_bytes, StreamingChannels, StreamingConfig, StreamingEvent,
    StreamingMessage, StreamingProvider, StreamingSocketRead,
};

pub struct SpeechmaticsStreaming;

fn map_lang_streaming(lang: Option<&str>) -> &str {
    // VoiceInk SpeechmaticsStreamingClient : si nil/empty/auto -> "en".
    match lang {
        None => "en",
        Some(l) if l.is_empty() || l == "auto" => "en",
        Some("zh") => "cmn",
        Some(l) => l,
    }
}

#[async_trait]
impl StreamingProvider for SpeechmaticsStreaming {
    fn id(&self) -> &'static str {
        "speechmatics"
    }

    async fn run(
        &self,
        api_key: String,
        config: StreamingConfig,
        channels: StreamingChannels,
        on_event: Box<dyn Fn(StreamingEvent) + Send + Sync>,
    ) -> Result<String> {
        let url = "wss://eu2.rt.speechmatics.com/v2";
        let mut req = url.into_client_request()?;
        req.headers_mut()
            .insert("Authorization", format!("Bearer {api_key}").parse()?);

        let socket = connect_streaming_socket(req).await?;
        let super::session::StreamingSocket {
            mut write,
            mut read,
        } = socket;

        // Construit transcription_config.
        let lang = map_lang_streaming(config.language.as_deref()).to_string();
        let mut tcfg = serde_json::Map::new();
        tcfg.insert("language".into(), json!(lang));
        tcfg.insert("enable_partials".into(), json!(true));
        tcfg.insert("operating_point".into(), json!("enhanced"));
        tcfg.insert("max_delay".into(), json!(2.0));
        tcfg.insert("max_delay_mode".into(), json!("flexible"));
        if !config.custom_vocabulary.is_empty() {
            let vocab: Vec<_> = config
                .custom_vocabulary
                .iter()
                .map(|t| json!({ "content": t }))
                .collect();
            tcfg.insert("additional_vocab".into(), json!(vocab));
        }

        let start = json!({
            "message": "StartRecognition",
            "audio_format": { "type": "raw", "encoding": "pcm_s16le", "sample_rate": 16000 },
            "transcription_config": tcfg,
        });
        write.send_text(start.to_string()).await?;

        // Handshake : attendre RecognitionStarted (10 s).
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if tokio::time::Instant::now() >= deadline {
                return Err(anyhow!("Speechmatics handshake timeout"));
            }
            let timeout = deadline - tokio::time::Instant::now();
            match tokio::time::timeout(timeout, read.next()).await {
                Ok(Ok(Some(StreamingMessage::Text(t)))) => {
                    let json: Value = serde_json::from_str(&t)?;
                    match json.get("message").and_then(|v| v.as_str()) {
                        Some("RecognitionStarted") => break,
                        Some("Error") => {
                            let reason = json
                                .get("reason")
                                .and_then(|v| v.as_str())
                                .unwrap_or("erreur StartRecognition")
                                .to_string();
                            return Err(anyhow!("Speechmatics: {reason}"));
                        }
                        _ => continue,
                    }
                }
                Ok(Ok(Some(_))) => continue,
                Ok(Err(e)) => return Err(anyhow!("ws read: {e}")),
                Ok(Ok(None)) => return Err(anyhow!("ws closed during handshake")),
                Err(_) => continue,
            }
        }
        on_event(StreamingEvent::SessionStarted);

        let StreamingChannels {
            mut audio_rx,
            mut finalize_rx,
        } = channels;

        let mut seq_no: u64 = 0;
        let mut accumulated_final = String::new();

        loop {
            tokio::select! {
                biased;
                _ = &mut finalize_rx => {
                    while let Ok(chunk) = audio_rx.try_recv() {
                        let _ = write.send_binary(i16_to_le_bytes(&chunk)).await;
                        seq_no += 1;
                    }
                    let end = json!({ "message": "EndOfStream", "last_seq_no": seq_no });
                    let _ = write.send_text(end.to_string()).await;
                    let text = drain(read.as_mut(), &mut accumulated_final, &on_event).await;
                    let _ = write.close().await;
                    return Ok(text);
                }
                chunk = audio_rx.recv() => {
                    match chunk {
                        Some(c) => {
                            if let Err(e) = write.send_binary(i16_to_le_bytes(&c)).await {
                                return Err(anyhow!("ws send: {e}"));
                            }
                            seq_no += 1;
                        }
                        None => return Ok(accumulated_final),
                    }
                }
                msg = read.next() => {
                    match msg {
                        Ok(Some(StreamingMessage::Text(t))) => handle_text(&t, &mut accumulated_final, &on_event),
                        Ok(Some(StreamingMessage::Close)) => return Ok(accumulated_final),
                        Ok(Some(_)) => {}
                        Err(e) => return Err(anyhow!("ws read: {e}")),
                        Ok(None) => return Ok(accumulated_final),
                    }
                }
            }
        }
    }
}

async fn drain(
    read: &mut dyn StreamingSocketRead,
    accumulated: &mut String,
    on_event: &(dyn Fn(StreamingEvent) + Send + Sync),
) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        match tokio::time::timeout(remaining, read.next()).await {
            Ok(Ok(Some(StreamingMessage::Text(t)))) => handle_text(&t, accumulated, on_event),
            Ok(Ok(Some(StreamingMessage::Close))) | Ok(Ok(None)) | Ok(Err(_)) | Err(_) => break,
            Ok(Ok(Some(_))) => {}
        }
    }
    clean_punctuation(accumulated.trim())
}

fn handle_text(
    t: &str,
    accumulated: &mut String,
    on_event: &(dyn Fn(StreamingEvent) + Send + Sync),
) {
    let Ok(json) = serde_json::from_str::<Value>(t) else {
        return;
    };
    let kind = json.get("message").and_then(|v| v.as_str()).unwrap_or("");
    let transcript = json
        .pointer("/metadata/transcript")
        .and_then(|v| v.as_str());

    match kind {
        "AudioAdded" | "Info" | "Warning" | "EndOfUtterance" => {}
        "AddPartialTranscript" => {
            if let Some(txt) = transcript {
                let mut preview = accumulated.clone();
                preview.push_str(txt);
                on_event(StreamingEvent::Partial {
                    text: clean_punctuation(&preview),
                });
            }
        }
        "AddTranscript" => {
            if let Some(txt) = transcript {
                accumulated.push_str(txt);
            }
        }
        "EndOfTranscript" => {
            on_event(StreamingEvent::Committed {
                text: clean_punctuation(accumulated.trim()),
            });
        }
        "Error" => {
            let reason = json
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("erreur")
                .to_string();
            on_event(StreamingEvent::Error { message: reason });
        }
        _ => {}
    }
}

/// Strip des espaces avant .,!?;:' (VoiceInk SpeechmaticsStreamingClient
/// cleanPunctuation).
fn clean_punctuation(input: &str) -> String {
    let re = regex::Regex::new(r#"\s+([.,!?;:'])"#).expect("clean_punctuation regex");
    re.replace_all(input, "$1").trim().to_string()
}
