// Client HTTP partage pour tous les providers "OpenAI chat/completions".
//
// Reference VoiceInk : LLMkit/OpenAILLMClient.swift.
// Shape requete :
//   POST {endpoint}
//   Header : Authorization: Bearer {api_key}
//   Body   : {
//     "model": ...,
//     "messages": [
//       {"role":"system","content":system},
//       {"role":"user","content":user}
//     ],
//     "temperature": f32,
//     ["reasoning_effort": "none"|"minimal"|"low"],
//     [...extra_body merge]
//   }
// Reponse : choices[0].message.content.
//
// Providers compatibles : OpenAI, Gemini (shim /v1beta/openai),
// Mistral, Groq, Cerebras, OpenRouter, Custom.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::enhancement::provider::{EnhancementRequest, EnhancementResponse};

/// Appel chat completion OpenAI-compatible.
pub async fn chat_completion(
    endpoint: &str,
    api_key: &str,
    req: &EnhancementRequest,
) -> Result<EnhancementResponse> {
    let mut body = serde_json::Map::new();
    body.insert("model".into(), json!(req.model));
    body.insert(
        "messages".into(),
        json!([
            {"role": "system", "content": req.system_prompt},
            {"role": "user", "content": req.user_message},
        ]),
    );
    body.insert("temperature".into(), json!(req.temperature));
    if let Some(effort) = req.reasoning.effort.as_ref() {
        body.insert("reasoning_effort".into(), json!(effort));
    }
    if let Some(extra) = req.reasoning.extra_body.as_ref() {
        for (k, v) in extra {
            body.insert(k.clone(), v.clone());
        }
    }

    let url = reqwest::Url::parse(endpoint).map_err(|e| anyhow!("endpoint: {e}"))?;
    let client = crate::services::proxy::apply_for_url(reqwest::Client::builder(), &url)?
        .timeout(req.timeout)
        .build()
        .map_err(|e| anyhow!("http client: {e}"))?;

    let mut builder = client.post(endpoint).json(&Value::Object(body));
    if !api_key.is_empty() {
        builder = builder.bearer_auth(api_key);
    }

    let resp = builder.send().await.map_err(map_http_err)?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let truncated: String = body.chars().take(500).collect();
        if status.as_u16() == 429 {
            return Err(anyhow!("rate_limit ({status}): {truncated}"));
        }
        if status.is_server_error() {
            return Err(anyhow!("server_error ({status}): {truncated}"));
        }
        return Err(anyhow!("http {status}: {truncated}"));
    }

    let json: Value = resp.json().await.map_err(|e| anyhow!("json parse: {e}"))?;
    let content = json
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("reponse sans choices[0].message.content"))?;
    Ok(EnhancementResponse {
        text: content.to_string(),
    })
}

fn map_http_err(e: reqwest::Error) -> anyhow::Error {
    if e.is_timeout() {
        return anyhow!("timeout: {e}");
    }
    if e.is_connect() {
        return anyhow!("network_error: {e}");
    }
    anyhow!("http: {e}")
}
