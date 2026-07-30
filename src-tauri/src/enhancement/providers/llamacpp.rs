// Provider llama.cpp embarque.
//
// Backend : crate `llama-cpp-2` (ffi llama.cpp).
// CUDA : active via la feature Parla `cuda-llama` qui propage
// `llama-cpp-2/cuda`. Quand active et qu'un GPU NVIDIA est detecte, on
// pousse toutes les layers sur GPU (n_gpu_layers=1000).
//
// Generation : non-streamee (parity avec les autres providers). Le modele
// est maintenu charge en memoire dans un Arc<Mutex<...>> (singleton cote
// service) et rechargement paresseux si l'ID selectionne change.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use parking_lot::Mutex;
use tauri_plugin_store::StoreExt;
use tracing::{info, warn};

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;

use crate::enhancement::provider::{EnhancementRequest, EnhancementResponse, LLMProvider};

const STORE_FILE: &str = "parla.settings.json";
const KEY_SELECTED_GGUF: &str = "llm_selected_gguf";
const KEY_N_GPU_LAYERS: &str = "llm_gguf_n_gpu_layers";
const KEY_MAX_TOKENS: &str = "llm_gguf_max_tokens";
const KEY_CONTEXT_SIZE: &str = "llm_gguf_context_size";

const DEFAULT_MAX_TOKENS: u32 = 1024;
const DEFAULT_CONTEXT_SIZE: u32 = 4096;

#[cfg(feature = "cuda-llama")]
const DEFAULT_GPU_LAYERS: u32 = 1000;
#[cfg(not(feature = "cuda-llama"))]
const DEFAULT_GPU_LAYERS: u32 = 0;

// -- Settings helpers -------------------------------------------------------

pub fn get_selected_gguf(app: &tauri::AppHandle) -> Option<String> {
    app.store(STORE_FILE)
        .ok()
        .and_then(|s| {
            s.get(KEY_SELECTED_GGUF)
                .and_then(|v| v.as_str().map(String::from))
        })
        .filter(|s| !s.is_empty())
}

pub fn set_selected_gguf(app: &tauri::AppHandle, id: Option<&str>) -> Result<()> {
    let store = app.store(STORE_FILE).map_err(|e| anyhow!("store: {e}"))?;
    match id {
        Some(i) => store.set(KEY_SELECTED_GGUF, serde_json::Value::String(i.into())),
        None => {
            store.delete(KEY_SELECTED_GGUF);
        }
    }
    store.save().map_err(|e| anyhow!("store save: {e}"))
}

fn get_u32(app: &tauri::AppHandle, key: &str, default: u32) -> u32 {
    app.store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(key).and_then(|v| v.as_u64()))
        .map(|v| v as u32)
        .unwrap_or(default)
}

pub fn get_max_tokens(app: &tauri::AppHandle) -> u32 {
    get_u32(app, KEY_MAX_TOKENS, DEFAULT_MAX_TOKENS).max(32)
}

pub fn set_max_tokens(app: &tauri::AppHandle, v: u32) -> Result<()> {
    let store = app.store(STORE_FILE).map_err(|e| anyhow!("store: {e}"))?;
    store.set(
        KEY_MAX_TOKENS,
        serde_json::Value::Number(serde_json::Number::from(v.max(32))),
    );
    store.save().map_err(|e| anyhow!("store save: {e}"))
}

pub fn get_context_size(app: &tauri::AppHandle) -> u32 {
    get_u32(app, KEY_CONTEXT_SIZE, DEFAULT_CONTEXT_SIZE).max(512)
}

pub fn set_context_size(app: &tauri::AppHandle, v: u32) -> Result<()> {
    let store = app.store(STORE_FILE).map_err(|e| anyhow!("store: {e}"))?;
    store.set(
        KEY_CONTEXT_SIZE,
        serde_json::Value::Number(serde_json::Number::from(v.max(512))),
    );
    store.save().map_err(|e| anyhow!("store save: {e}"))
}

pub fn get_n_gpu_layers(app: &tauri::AppHandle) -> u32 {
    get_u32(app, KEY_N_GPU_LAYERS, DEFAULT_GPU_LAYERS)
}

pub fn set_n_gpu_layers(app: &tauri::AppHandle, v: u32) -> Result<()> {
    let store = app.store(STORE_FILE).map_err(|e| anyhow!("store: {e}"))?;
    store.set(
        KEY_N_GPU_LAYERS,
        serde_json::Value::Number(serde_json::Number::from(v)),
    );
    store.save().map_err(|e| anyhow!("store save: {e}"))
}

// -- Runtime singleton ------------------------------------------------------

/// Singleton runtime llama.cpp : backend + modele charge.
/// Le backend doit etre unique dans le process (cf llama.cpp ggml_init).
struct LoadedModel {
    path: PathBuf,
    n_gpu_layers: u32,
    context_size: u32,
    model: LlamaModel,
}

pub struct LlamaRuntime {
    backend: Arc<LlamaBackend>,
    current: Mutex<Option<LoadedModel>>,
}

impl LlamaRuntime {
    pub fn new() -> Result<Self> {
        let backend = LlamaBackend::init().map_err(|e| anyhow!("llama backend init: {e}"))?;
        Ok(Self {
            backend: Arc::new(backend),
            current: Mutex::new(None),
        })
    }

    /// Charge ou recharge le modele si necessaire. Bloquant (IO + mmap).
    fn ensure_loaded(&self, path: &Path, n_gpu_layers: u32, context_size: u32) -> Result<()> {
        let mut guard = self.current.lock();
        if let Some(cur) = guard.as_ref() {
            if cur.path == path
                && cur.n_gpu_layers == n_gpu_layers
                && cur.context_size == context_size
            {
                return Ok(());
            }
        }
        // Drop l'ancien avant de charger (libere la VRAM / RAM).
        *guard = None;

        let params = LlamaModelParams::default().with_n_gpu_layers(n_gpu_layers);
        info!(
            path = %path.display(),
            n_gpu_layers,
            context_size,
            "Chargement modele GGUF"
        );
        let model = LlamaModel::load_from_file(&self.backend, path, &params)
            .map_err(|e| anyhow!("llama load: {e}"))?;
        *guard = Some(LoadedModel {
            path: path.to_path_buf(),
            n_gpu_layers,
            context_size,
            model,
        });
        Ok(())
    }

    /// Genere du texte pour un prompt donne. Bloquant (CPU/GPU intensif).
    pub fn generate(
        &self,
        path: &Path,
        n_gpu_layers: u32,
        context_size: u32,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<String> {
        self.ensure_loaded(path, n_gpu_layers, context_size)?;
        let guard = self.current.lock();
        let loaded = guard.as_ref().ok_or_else(|| anyhow!("modele non charge"))?;
        let model = &loaded.model;

        let ctx_params = LlamaContextParams::default().with_n_ctx(NonZeroU32::new(context_size));
        let mut ctx = model
            .new_context(&self.backend, ctx_params)
            .map_err(|e| anyhow!("llama context: {e}"))?;

        let tokens_list = model
            .str_to_token(prompt, AddBos::Always)
            .map_err(|e| anyhow!("tokenize: {e}"))?;

        if tokens_list.is_empty() {
            return Err(anyhow!("prompt vide apres tokenization"));
        }

        if (tokens_list.len() as u32) + max_tokens > context_size {
            warn!(
                prompt_tokens = tokens_list.len(),
                max_tokens, context_size, "Prompt + generation risquent de depasser le contexte"
            );
        }

        let batch_capacity = tokens_list.len().max(512);
        let mut batch = LlamaBatch::new(batch_capacity, 1);
        let last_index = tokens_list.len() - 1;
        for (i, token) in tokens_list.iter().enumerate() {
            batch
                .add(*token, i as i32, &[0], i == last_index)
                .map_err(|e| anyhow!("batch add prompt: {e}"))?;
        }
        ctx.decode(&mut batch)
            .map_err(|e| anyhow!("decode prompt: {e}"))?;

        // Greedy sampling : on veut un output deterministe pour l'enhancement.
        let mut sampler = LlamaSampler::greedy();

        let mut generated: Vec<LlamaToken> = Vec::new();
        let mut n_cur = tokens_list.len() as i32;
        let n_max = n_cur + max_tokens as i32;

        while n_cur < n_max {
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            sampler.accept(token);
            if model.is_eog_token(token) {
                break;
            }
            generated.push(token);

            batch.clear();
            batch
                .add(token, n_cur, &[0], true)
                .map_err(|e| anyhow!("batch add tok: {e}"))?;
            ctx.decode(&mut batch)
                .map_err(|e| anyhow!("decode step: {e}"))?;
            n_cur += 1;
        }

        // Detokenize en un seul coup : rend correctement les sequences
        // multi-byte UTF-8 au lieu de risquer une troncature par token.
        let output = if generated.is_empty() {
            String::new()
        } else {
            #[allow(deprecated)]
            let out = model
                .tokens_to_str(&generated, llama_cpp_2::model::Special::Plaintext)
                .map_err(|e| anyhow!("detokenize: {e}"))?;
            out
        };

        Ok(output)
    }

    /// Libere le modele charge (utile quand l'utilisateur change de GGUF
    /// ou desactive l'enhancement).
    pub fn unload(&self) {
        let mut guard = self.current.lock();
        *guard = None;
    }
}

// -- Provider ---------------------------------------------------------------

pub struct LlamaCppProvider;

#[async_trait]
impl LLMProvider for LlamaCppProvider {
    fn id(&self) -> &'static str {
        "llamacpp"
    }
    fn label(&self) -> &'static str {
        "llama.cpp (local)"
    }
    fn default_models(&self) -> &'static [&'static str] {
        // Les modeles dispos sont telecharges via le GGUF model manager,
        // expose cote UI.
        &[]
    }
    fn default_model(&self) -> &'static str {
        ""
    }
    fn endpoint(&self) -> &'static str {
        "embedded: llama.cpp"
    }
    fn requires_api_key(&self) -> bool {
        false
    }
    fn rate_limited(&self) -> bool {
        false
    }

    async fn chat_completion(
        &self,
        _api_key: &str,
        req: &EnhancementRequest,
    ) -> Result<EnhancementResponse> {
        // endpoint_override porte le chemin GGUF + config packee JSON par le
        // service (pragmatique : un seul champ libre dans le trait).
        // Format : `{gguf_path}|{n_gpu_layers}|{context_size}|{max_tokens}`
        let config = req
            .endpoint_override
            .as_deref()
            .ok_or_else(|| anyhow!("llama.cpp: aucun modele GGUF selectionne"))?;
        let parts: Vec<&str> = config.splitn(4, '|').collect();
        if parts.len() != 4 {
            return Err(anyhow!("llama.cpp: config invalide"));
        }
        let path = PathBuf::from(parts[0]);
        let n_gpu_layers: u32 = parts[1].parse().unwrap_or(DEFAULT_GPU_LAYERS);
        let context_size: u32 = parts[2].parse().unwrap_or(DEFAULT_CONTEXT_SIZE);
        let max_tokens: u32 = parts[3].parse().unwrap_or(DEFAULT_MAX_TOKENS);

        // Prompt : on compose system + user en respectant le pattern
        // "ChatML-ish" generique (funcionne raisonnablement pour Qwen, Llama,
        // Gemma, Phi sans chat template dedie).
        let prompt = format!(
            "<|system|>\n{sys}\n<|user|>\n{usr}\n<|assistant|>\n",
            sys = req.system_prompt.trim(),
            usr = req.user_message.trim()
        );

        // Inference bloquante : spawn_blocking pour ne pas monopoliser le
        // runtime async Tokio.
        let runtime = crate::enhancement::service::llama_runtime()
            .ok_or_else(|| anyhow!("llama runtime absent"))?;
        let prompt_owned = prompt;
        let path_owned = path;
        tokio::task::spawn_blocking(move || {
            runtime.generate(
                &path_owned,
                n_gpu_layers,
                context_size,
                &prompt_owned,
                max_tokens,
            )
        })
        .await
        .map_err(|e| anyhow!("join: {e}"))?
        .map(|text| EnhancementResponse { text })
    }
}
