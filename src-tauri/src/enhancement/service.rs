// Service d'orchestration de l'enhancement LLM.
//
// Reference VoiceInk : VoiceInk/Services/AIEnhancement/AIEnhancementService.swift.
// Flow reproduit (L~150-290) :
//   1. is_configured (clef + provider selectionnes)
//   2. build system message : active_prompt.final_prompt_text + <CLIPBOARD_CONTEXT>
//      + <CURRENT_WINDOW_CONTEXT> + <CUSTOM_VOCABULARY>
//   3. wrap user: "\n<TRANSCRIPT>\n{text}\n</TRANSCRIPT>"
//   4. branche par provider (Anthropic / OpenAI-compat / Ollama / LocalCLI)
//   5. filter output (<thinking>/<think>/<reasoning>)
//   6. retry : 3 tentatives avec backoff exponentiel (sauf Ollama/LocalCLI)
//   7. rate limit : 1.0s min entre deux requetes cloud.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;
use tracing::{info, warn};

use super::providers::llamacpp::LlamaRuntime;

use crate::db::{word_replacement as word_repo, Database};
use crate::services::api_keys;

use super::output_filter;
use super::prompts::{self, CustomPrompt};
use super::provider::{EnhancementRequest, ReasoningConfig};
use super::registry::LLMRegistry;

const STORE_FILE: &str = "parla.settings.json";
const KEY_LLM_PROVIDER: &str = "llm_provider";
const KEY_LLM_MODEL: &str = "llm_model";
const KEY_LLM_TIMEOUT: &str = "llm_timeout_seconds";
const KEY_LLM_RETRY_ON_TIMEOUT: &str = "llm_retry_on_timeout";
const KEY_LLM_SKIP_SHORT_WORDS: &str = "llm_skip_short_word_threshold";

const DEFAULT_TIMEOUT_SECS: u64 = 7;
const RATE_LIMIT_INTERVAL: Duration = Duration::from_millis(1000);
const MAX_RETRIES: u32 = 3;

/// State Tauri : registry + horodatage derniere requete (pour rate limit).
pub struct EnhancementState {
    pub registry: Arc<LLMRegistry>,
    last_request: Mutex<Option<Instant>>,
}

impl Default for EnhancementState {
    fn default() -> Self {
        Self {
            registry: Arc::new(LLMRegistry::new()),
            last_request: Mutex::new(None),
        }
    }
}

/// Runtime llama.cpp global. Initialise de maniere paresseuse au premier
/// appel de l'enhancement llamacpp. LlamaBackend ne doit etre instancie
/// qu'une seule fois par process.
static LLAMA_RUNTIME: OnceLock<Arc<LlamaRuntime>> = OnceLock::new();

pub fn llama_runtime() -> Option<Arc<LlamaRuntime>> {
    LLAMA_RUNTIME.get().cloned()
}

fn ensure_llama_runtime() -> Result<Arc<LlamaRuntime>> {
    if let Some(rt) = LLAMA_RUNTIME.get() {
        return Ok(rt.clone());
    }
    let rt = Arc::new(LlamaRuntime::new()?);
    let _ = LLAMA_RUNTIME.set(rt.clone());
    Ok(LLAMA_RUNTIME.get().cloned().unwrap_or(rt))
}

/// Configuration LLM extraite du store.
#[derive(Debug, Clone)]
pub struct LLMSelection {
    pub provider_id: String,
    pub model: String,
}

pub fn get_selection(app: &AppHandle) -> Option<LLMSelection> {
    let store = app.store(STORE_FILE).ok()?;
    let provider_id = store.get(KEY_LLM_PROVIDER)?.as_str()?.to_string();
    let model = store.get(KEY_LLM_MODEL)?.as_str()?.to_string();
    if provider_id.is_empty() {
        return None;
    }
    Some(LLMSelection { provider_id, model })
}

pub fn set_selection(app: &AppHandle, provider_id: &str, model: &str) -> Result<()> {
    let store = app.store(STORE_FILE).map_err(|e| anyhow!("store: {e}"))?;
    store.set(
        KEY_LLM_PROVIDER,
        serde_json::Value::String(provider_id.into()),
    );
    store.set(KEY_LLM_MODEL, serde_json::Value::String(model.into()));
    store.save().map_err(|e| anyhow!("store save: {e}"))?;
    Ok(())
}

pub fn get_timeout(app: &AppHandle) -> Duration {
    let secs = app
        .store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(KEY_LLM_TIMEOUT).and_then(|v| v.as_u64()))
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    Duration::from_secs(secs.max(1))
}

pub fn get_retry_on_timeout(app: &AppHandle) -> bool {
    app.store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(KEY_LLM_RETRY_ON_TIMEOUT).and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

pub fn get_skip_short_threshold(app: &AppHandle) -> Option<usize> {
    app.store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(KEY_LLM_SKIP_SHORT_WORDS).and_then(|v| v.as_u64()))
        .map(|v| v as usize)
}

/// Indique si l'enhancement est configure (enabled + provider + cle).
pub fn is_configured(app: &AppHandle) -> bool {
    if !prompts::is_enhancement_enabled(app) {
        return false;
    }
    has_provider_configured(app)
}

/// Indique si un provider est configure (selection + cle), independamment de
/// `is_enhancement_enabled`. Utilise par la detection trigger_words qui peut
/// forcer l'enhancement meme si le toggle global est off.
pub fn has_provider_configured(app: &AppHandle) -> bool {
    let Some(sel) = get_selection(app) else {
        return false;
    };
    // Custom provider : la cle peut etre facultative selon l'endpoint.
    let requires_key = app
        .try_state::<EnhancementState>()
        .and_then(|s| {
            s.registry
                .find(&sel.provider_id)
                .map(|p| p.requires_api_key())
        })
        .unwrap_or(true);
    if !requires_key {
        return true;
    }
    api_keys::has_api_key(&sel.provider_id)
}

/// Construit la chaine config passee a LlamaCppProvider via endpoint_override.
/// Format : `{gguf_path}|{n_gpu_layers}|{context_size}|{max_tokens}`.
fn build_llamacpp_endpoint(app: &AppHandle) -> Result<Option<String>> {
    use super::providers::llamacpp as llcp;
    let id =
        llcp::get_selected_gguf(app).ok_or_else(|| anyhow!("aucun modele GGUF selectionne"))?;
    let mgr_state = app
        .try_state::<crate::commands::llm_models::GgufModelManagerState>()
        .ok_or_else(|| anyhow!("gguf manager state absent"))?;
    let path = mgr_state
        .0
        .path_for_id(&id)
        .ok_or_else(|| anyhow!("modele GGUF non telecharge: {id}"))?;
    let n_gpu = llcp::get_n_gpu_layers(app);
    let ctx = llcp::get_context_size(app);
    let max = llcp::get_max_tokens(app);
    // Init paresseux du runtime des qu'on sait qu'on va en avoir besoin.
    ensure_llama_runtime()?;
    Ok(Some(format!(
        "{}|{}|{}|{}",
        path.to_string_lossy(),
        n_gpu,
        ctx,
        max
    )))
}

/// Doit-on skipper l'enhancement pour un texte donne (seuil mots) ?
pub fn should_skip_short(app: &AppHandle, text: &str) -> bool {
    let Some(th) = get_skip_short_threshold(app) else {
        return false;
    };
    let words = text.split_whitespace().count();
    words <= th
}

/// Construit la temperature appliquee. VoiceInk AIService L~140.
fn temperature_for(model: &str) -> f32 {
    if model.starts_with("gpt-5") {
        1.0
    } else {
        0.3
    }
}

/// Reasoning config par (provider, model). Reference VoiceInk AIService
/// ReasoningConfig L~200-260.
fn reasoning_for(provider_id: &str, model: &str) -> ReasoningConfig {
    let mut cfg = ReasoningConfig::default();
    match (provider_id, model) {
        ("openai", "gpt-5.4" | "gpt-5.4-mini" | "gpt-5.4-nano" | "gpt-5.2") => {
            cfg.effort = Some("none".into());
        }
        ("openai", "gpt-5-mini" | "gpt-5-nano") => {
            cfg.effort = Some("minimal".into());
        }
        ("gemini", "gemini-2.5-flash" | "gemini-2.5-flash-lite") => {
            cfg.effort = Some("none".into());
        }
        (
            "gemini",
            "gemini-3.1-pro-preview" | "gemini-3-flash-preview" | "gemini-3.1-flash-lite-preview",
        ) => {
            cfg.effort = Some("minimal".into());
        }
        ("cerebras", "gpt-oss-120b") => cfg.effort = Some("low".into()),
        ("cerebras", "zai-glm-4.7") => {
            let mut map = serde_json::Map::new();
            map.insert("disable_reasoning".into(), serde_json::Value::Bool(true));
            cfg.extra_body = Some(map);
        }
        ("groq", "openai/gpt-oss-120b" | "openai/gpt-oss-20b") => {
            cfg.effort = Some("low".into());
        }
        ("groq", "qwen/qwen3-32b") => cfg.effort = Some("none".into()),
        _ => {}
    }
    cfg
}

/// Assemble le systeme prompt final : prompt.final_prompt_text + blocs de
/// contexte optionnels (clipboard, screen, vocabulary). VoiceInk AIEnhancementService
/// L~220-300. Ordre : selected_text + clipboard + screen + vocabulary.
fn build_system_message(app: &AppHandle, active_prompt: &CustomPrompt) -> String {
    let mut out = active_prompt.final_prompt_text();

    // <CURRENT_WINDOW_CONTEXT> : texte OCR de la fenetre active (Phase 7).
    // VoiceInk AIEnhancementService L~165-171 : gate sur useScreenCaptureContext
    // + presence d'un lastCapturedText non vide. On lit le cache populate par
    // screen_context::service::capture_and_ocr au record start.
    if crate::screen_context::service::is_enabled(app) {
        if let Some(text) = crate::screen_context::service::cached_text(app) {
            if !text.trim().is_empty() {
                out.push_str("\n\n<CURRENT_WINDOW_CONTEXT>\n");
                out.push_str(&text);
                out.push_str("\n</CURRENT_WINDOW_CONTEXT>");
            }
        }
    }

    // <CUSTOM_VOCABULARY> : liste dictionnaire enabled.
    if let Some(db) = app.try_state::<Database>() {
        if let Ok(rules) = word_repo::list_enabled(&db.0.lock()) {
            if !rules.is_empty() {
                let mut terms: Vec<String> = Vec::new();
                for r in rules {
                    for t in r.original_text.split(',') {
                        let t = t.trim();
                        if !t.is_empty() {
                            terms.push(t.to_string());
                        }
                    }
                }
                if !terms.is_empty() {
                    out.push_str("\n\n<CUSTOM_VOCABULARY>\n");
                    out.push_str(&terms.join("\n"));
                    out.push_str("\n</CUSTOM_VOCABULARY>");
                }
            }
        }
    }
    out
}

/// Wrap du texte utilisateur. VoiceInk AIEnhancementService L~200.
fn build_user_message(text: &str) -> String {
    format!("\n<TRANSCRIPT>\n{text}\n</TRANSCRIPT>")
}

/// Entree publique : applique l'enhancement. Renvoie :
///   - Some(texte_ameliore) si l'enhancement a reussi et doit remplacer le texte
///   - None si skip (non configure, seuil mots, desactive)
///   - Err si une tentative a echoue apres retries
///
/// `prompt_override` : utilise par la detection trigger_words (VoiceInk
/// PromptDetectionService.applyDetectionResult) qui force l'enhancement ON
/// avec un prompt specifique meme si le toggle global est off.
pub async fn enhance_with_override(
    app: AppHandle,
    text: String,
    prompt_override: Option<CustomPrompt>,
) -> Result<Option<(String, u64)>> {
    // Sans override : on exige enabled + provider + cle (is_configured).
    // Avec override : on force l'enhancement meme si disabled, on exige
    // juste un provider configure (cle + selection).
    if prompt_override.is_none() {
        if !is_configured(&app) {
            return Ok(None);
        }
        if should_skip_short(&app, &text) {
            info!("Enhancement skip (texte trop court)");
            return Ok(None);
        }
    } else if !has_provider_configured(&app) {
        info!("trigger_words detecte mais aucun provider LLM configure, skip");
        return Ok(None);
    }

    let state = app
        .try_state::<EnhancementState>()
        .ok_or_else(|| anyhow!("EnhancementState absent"))?;

    let sel = get_selection(&app).ok_or_else(|| anyhow!("aucun LLM selectionne"))?;
    let provider = state
        .registry
        .find(&sel.provider_id)
        .ok_or_else(|| anyhow!("provider LLM inconnu: {}", sel.provider_id))?;

    let model = if sel.model.is_empty() {
        provider.default_model().to_string()
    } else {
        sel.model
    };

    let api_key = if provider.requires_api_key() {
        api_keys::get_api_key(provider.id())
            .map_err(|e| anyhow!("keyring: {e}"))?
            .ok_or_else(|| anyhow!("aucune cle API pour {}", provider.id()))?
    } else {
        String::new()
    };

    let active = match prompt_override {
        Some(p) => p,
        None => prompts::get_active_prompt(&app)?,
    };
    let system_prompt = build_system_message(&app, &active);
    let user_message = build_user_message(&text);

    // endpoint_override : Ollama et Custom ont une URL configurable,
    // LocalCLI recoit la commande custom si template=custom, llamacpp recoit
    // le chemin GGUF + config d'inference packee.
    let endpoint_override = match provider.id() {
        "ollama" => Some(super::providers::ollama::get_base_url(&app)),
        "custom" => super::providers::custom::get_base_url(&app),
        "localcli" if model == "custom" => super::providers::local_cli::get_custom_cmd(&app),
        "llamacpp" => build_llamacpp_endpoint(&app)?,
        _ => None,
    };

    // Timeout : LocalCLI a son propre timeout (defaut 45s) car un LLM
    // local peut etre beaucoup plus lent que 7s.
    let timeout = if provider.id() == "localcli" {
        Duration::from_secs(super::providers::local_cli::get_timeout_secs(&app))
    } else {
        get_timeout(&app)
    };

    let req = EnhancementRequest {
        system_prompt,
        user_message,
        model: model.clone(),
        temperature: temperature_for(&model),
        reasoning: reasoning_for(provider.id(), &model),
        timeout,
        endpoint_override,
    };

    // Rate limit 1s : uniquement pour les providers cloud (VoiceInk exempte
    // Ollama et LocalCLI). On ne tient jamais le lock au travers d'un .await.
    if provider.rate_limited() {
        let wait = {
            let last = state.last_request.lock();
            last.and_then(|prev| {
                let elapsed = prev.elapsed();
                if elapsed < RATE_LIMIT_INTERVAL {
                    Some(RATE_LIMIT_INTERVAL - elapsed)
                } else {
                    None
                }
            })
        };
        if let Some(w) = wait {
            tokio::time::sleep(w).await;
        }
        *state.last_request.lock() = Some(Instant::now());
    }

    let retry_on_timeout = get_retry_on_timeout(&app);
    let start = Instant::now();
    let mut backoff = Duration::from_millis(1000);
    let mut last_err: Option<anyhow::Error> = None;

    for attempt in 0..MAX_RETRIES {
        match provider.chat_completion(&api_key, &req).await {
            Ok(resp) => {
                let filtered = output_filter::filter(&resp.text);
                let duration_ms = start.elapsed().as_millis() as u64;
                info!(
                    provider = provider.id(),
                    model = %req.model,
                    chars_in = text.len(),
                    chars_out = filtered.len(),
                    duration_ms,
                    "Enhancement LLM ok"
                );
                return Ok(Some((filtered, duration_ms)));
            }
            Err(e) => {
                let msg = e.to_string();
                let is_timeout = msg.to_lowercase().contains("timeout");
                warn!(attempt, error = %msg, "Enhancement LLM attempt echec");
                last_err = Some(e);
                if attempt + 1 >= MAX_RETRIES {
                    break;
                }
                if is_timeout && !retry_on_timeout {
                    break;
                }
                if !is_timeout {
                    tokio::time::sleep(backoff).await;
                    backoff *= 2;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("enhancement echec inconnu")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temperature_for_gpt5_family() {
        // VoiceInk : toute la famille gpt-5* prend 1.0.
        assert_eq!(temperature_for("gpt-5"), 1.0);
        assert_eq!(temperature_for("gpt-5.4"), 1.0);
        assert_eq!(temperature_for("gpt-5-mini"), 1.0);
        assert_eq!(temperature_for("gpt-5.4-nano"), 1.0);
    }

    #[test]
    fn temperature_for_other_models() {
        assert_eq!(temperature_for("gpt-4.1"), 0.3);
        assert_eq!(temperature_for("claude-opus-4-6"), 0.3);
        assert_eq!(temperature_for("gemini-2.5-pro"), 0.3);
        assert_eq!(temperature_for("llama-3.3-70b-versatile"), 0.3);
        assert_eq!(temperature_for(""), 0.3);
    }

    #[test]
    fn reasoning_for_openai_gpt5_flagship() {
        let r = reasoning_for("openai", "gpt-5.4");
        assert_eq!(r.effort, Some("none".into()));
        assert!(r.extra_body.is_none());
    }

    #[test]
    fn reasoning_for_openai_gpt5_mini() {
        let r = reasoning_for("openai", "gpt-5-mini");
        assert_eq!(r.effort, Some("minimal".into()));
    }

    #[test]
    fn reasoning_for_gemini_flash() {
        let r = reasoning_for("gemini", "gemini-2.5-flash");
        assert_eq!(r.effort, Some("none".into()));
    }

    #[test]
    fn reasoning_for_gemini_pro() {
        // gemini-2.5-pro n'est pas dans la liste "flash" donc effort = None.
        let r = reasoning_for("gemini", "gemini-2.5-pro");
        assert_eq!(r.effort, None);
    }

    #[test]
    fn reasoning_for_cerebras_oss120b() {
        let r = reasoning_for("cerebras", "gpt-oss-120b");
        assert_eq!(r.effort, Some("low".into()));
    }

    #[test]
    fn reasoning_for_cerebras_glm() {
        let r = reasoning_for("cerebras", "zai-glm-4.7");
        assert_eq!(r.effort, None);
        let body = r.extra_body.expect("extra_body attendu pour zai-glm");
        assert_eq!(
            body.get("disable_reasoning"),
            Some(&serde_json::Value::Bool(true))
        );
    }

    #[test]
    fn reasoning_for_groq_oss() {
        assert_eq!(
            reasoning_for("groq", "openai/gpt-oss-120b").effort,
            Some("low".into())
        );
        assert_eq!(
            reasoning_for("groq", "openai/gpt-oss-20b").effort,
            Some("low".into())
        );
    }

    #[test]
    fn reasoning_for_unknown_provider() {
        let r = reasoning_for("anthropic", "claude-opus-4-6");
        assert_eq!(r.effort, None);
        assert!(r.extra_body.is_none());
    }

    #[test]
    fn reasoning_for_unknown_model() {
        let r = reasoning_for("openai", "unknown-model");
        assert_eq!(r.effort, None);
    }
}
