// Soniox stt-rt-v5 realtime (VoiceInk SonioxStreamingProvider). Le modele vient du catalogue.
//
// Reference VoiceInk : LLMkit SonioxStreamingClient.swift.
// WSS : wss://stt-rt.soniox.com/transcribe-websocket
// Auth : PAS de header, la cle est dans le premier message JSON.
// Message initial (envoye juste apres connect, avant toute reception) :
//   {
//     api_key: "<key>", model, audio_format: "pcm_s16le", sample_rate: 16000,
//     num_channels: 1,
//     [language_hints: [lang], language_hints_strict: true, enable_language_identification: true]
//     ou [enable_language_identification: true] si pas de langue,
//     [context: { terms: [...] }]
//   }
// Puis emit SessionStarted (pas de ack serveur).
// Audio : frames binaires.
// Commit : {"type":"finalize"}.
// Events JSON :
//   - error_code present -> Error(error_message)
//   - finished == true -> Committed(finalText), reset
//   - sinon tokens[] : text == "<fin>" -> sawFin, sinon is_final -> final vs partial
//   - sawFin -> Committed
//   - sinon partialText non vide -> Partial(finalText + partialText)

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use super::session::{
    connect_ws_url, drain_ws_messages, i16_to_le_bytes, StreamingChannels, StreamingConfig,
    StreamingEvent, StreamingProvider,
};

pub struct SonioxStreaming;

#[async_trait]
impl StreamingProvider for SonioxStreaming {
    fn id(&self) -> &'static str {
        "soniox"
    }

    async fn run(
        &self,
        api_key: String,
        config: StreamingConfig,
        channels: StreamingChannels,
        on_event: Box<dyn Fn(StreamingEvent) + Send + Sync>,
    ) -> Result<String> {
        let url = "wss://stt-rt.soniox.com/transcribe-websocket";
        let ws_stream = connect_ws_url(url).await?;
        let (mut write, mut read) = ws_stream.split();

        // Message de configuration initial.
        let mut payload = serde_json::Map::new();
        payload.insert("api_key".into(), json!(api_key));
        payload.insert("model".into(), json!(config.model));
        payload.insert("audio_format".into(), json!("pcm_s16le"));
        payload.insert("sample_rate".into(), json!(16000));
        payload.insert("num_channels".into(), json!(1));

        match config.language.as_deref() {
            Some(lang) if !lang.is_empty() && lang != "auto" => {
                payload.insert("language_hints".into(), json!([lang]));
                payload.insert("language_hints_strict".into(), json!(true));
                payload.insert("enable_language_identification".into(), json!(true));
            }
            _ => {
                payload.insert("enable_language_identification".into(), json!(true));
            }
        }
        if !config.custom_vocabulary.is_empty() {
            payload.insert(
                "context".into(),
                json!({ "terms": config.custom_vocabulary }),
            );
        }

        write
            .send(Message::Text(
                serde_json::to_string(&payload).unwrap().into(),
            ))
            .await?;
        on_event(StreamingEvent::SessionStarted);

        let StreamingChannels {
            mut audio_rx,
            mut finalize_rx,
        } = channels;

        let mut final_text = String::new();

        loop {
            tokio::select! {
                biased;
                _ = &mut finalize_rx => {
                    while let Ok(chunk) = audio_rx.try_recv() {
                        let _ = write.send(Message::Binary(i16_to_le_bytes(&chunk).into())).await;
                    }
                    let _ = write.send(Message::Text(json!({ "type": "finalize" }).to_string().into())).await;
                    let text = drain(&mut read, &mut final_text, &on_event).await;
                    let _ = write.close().await;
                    return Ok(text);
                }
                chunk = audio_rx.recv() => {
                    match chunk {
                        Some(c) => {
                            if let Err(e) = write.send(Message::Binary(i16_to_le_bytes(&c).into())).await {
                                return Err(anyhow!("ws send: {e}"));
                            }
                        }
                        None => return Ok(final_text),
                    }
                }
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(t))) => handle_text(&t, &mut final_text, &on_event),
                        Some(Ok(Message::Close(_))) => return Ok(final_text),
                        Some(Ok(_)) => {}
                        Some(Err(e)) => return Err(anyhow!("ws read: {e}")),
                        None => return Ok(final_text),
                    }
                }
            }
        }
    }
}

async fn drain<S>(
    read: &mut S,
    final_text: &mut String,
    on_event: &(dyn Fn(StreamingEvent) + Send + Sync),
) -> String
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    drain_ws_messages(read, std::time::Duration::from_secs(5), |t| {
        handle_text(t, final_text, on_event)
    })
    .await;
    final_text.trim().to_string()
}

fn handle_text(
    t: &str,
    final_text: &mut String,
    on_event: &(dyn Fn(StreamingEvent) + Send + Sync),
) {
    let Ok(json) = serde_json::from_str::<Value>(t) else {
        return;
    };

    if let Some(code) = json.get("error_code").and_then(|v| v.as_str()) {
        let msg = json
            .get("error_message")
            .and_then(|v| v.as_str())
            .unwrap_or(code)
            .to_string();
        on_event(StreamingEvent::Error { message: msg });
        return;
    }

    if json
        .get("finished")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        on_event(StreamingEvent::Committed {
            text: final_text.trim().to_string(),
        });
        return;
    }

    let tokens = json.get("tokens").and_then(|v| v.as_array());
    let Some(tokens) = tokens else { return };

    let mut saw_fin = false;
    let mut new_final = String::new();
    let mut new_partial = String::new();
    for t in tokens {
        let text = t.get("text").and_then(|v| v.as_str()).unwrap_or("");
        if text == "<fin>" {
            saw_fin = true;
            continue;
        }
        let is_final = t.get("is_final").and_then(|v| v.as_bool()).unwrap_or(false);
        if is_final {
            new_final.push_str(text);
        } else {
            new_partial.push_str(text);
        }
    }

    if saw_fin {
        if !new_final.trim().is_empty() {
            final_text.push_str(&new_final);
        }
        on_event(StreamingEvent::Committed {
            text: final_text.trim().to_string(),
        });
    } else if !new_partial.trim().is_empty() {
        final_text.push_str(&new_final);
        let mut preview = final_text.clone();
        preview.push_str(&new_partial);
        on_event(StreamingEvent::Partial { text: preview });
    } else if !new_final.is_empty() {
        final_text.push_str(&new_final);
    }
}
