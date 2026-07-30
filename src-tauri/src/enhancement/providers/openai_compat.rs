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

pub fn build_request(
    endpoint: &str,
    api_key: &str,
    req: &EnhancementRequest,
) -> Result<crate::transcription::cloud::http::HttpRequest> {
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
    let request = crate::transcription::cloud::http::HttpRequest::new("POST", endpoint)
        .header("content-type", "application/json")
        .body(serde_json::to_vec(&Value::Object(body))?);
    Ok(if api_key.is_empty() {
        request
    } else {
        request.header("authorization", format!("Bearer {api_key}"))
    })
}

pub fn parse_response(body: &[u8]) -> Result<EnhancementResponse> {
    let json: Value = serde_json::from_slice(body).map_err(|e| anyhow!("json parse: {e}"))?;
    let content = json
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("reponse sans choices[0].message.content"))?;
    Ok(EnhancementResponse {
        text: content.to_string(),
    })
}

/// Appel chat completion OpenAI-compatible.
pub async fn chat_completion(
    endpoint: &str,
    api_key: &str,
    req: &EnhancementRequest,
) -> Result<EnhancementResponse> {
    let request = build_request(endpoint, api_key, req)?;
    let response = crate::transcription::cloud::http::BatchHttpClient::new(endpoint)?
        .send_with_timeout(request.clone(), req.timeout)
        .await?;
    if !(200..300).contains(&response.status) {
        let detail = crate::transcription::cloud::http::http_status_error(
            response.status,
            &response.body,
            &request,
        );
        if response.status == 429 {
            return Err(anyhow!("rate_limit ({}){}", response.status, detail));
        }
        if response.status >= 500 {
            return Err(anyhow!("server_error ({}){}", response.status, detail));
        }
        return Err(detail);
    }

    parse_response(&response.body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enhancement::provider::{EnhancementRequest, ReasoningConfig};
    use std::time::Duration;

    #[test]
    fn request_and_response_match_openai_shape() {
        let req = EnhancementRequest {
            system_prompt: "system".into(),
            user_message: "user".into(),
            model: "gpt-test".into(),
            temperature: 0.2,
            reasoning: ReasoningConfig::default(),
            timeout: Duration::from_secs(1),
            endpoint_override: None,
        };
        let request = build_request("https://example.test/chat", "key", &req).unwrap();
        assert_eq!(
            request.headers[1],
            ("authorization".into(), "Bearer key".into())
        );
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["messages"][1]["content"], "user");
        assert_eq!(
            parse_response(br#"{"choices":[{"message":{"content":"done"}}]}"#)
                .unwrap()
                .text,
            "done"
        );
    }
}
