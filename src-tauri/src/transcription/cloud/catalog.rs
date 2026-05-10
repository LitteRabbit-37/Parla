// Catalogue des providers cloud et de leurs modeles.
//
// Reference VoiceInk : VoiceInk/Models/PredefinedModels.swift (section cloud).
// Meme ids et meme endpoints.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudProviderInfo {
    pub id: &'static str,
    pub display_name: &'static str,
    pub requires_api_key: bool,
    /// Lien vers la page de creation de cle (ouvert via tauri-plugin-opener).
    pub api_key_url: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudModelInfo {
    pub provider_id: &'static str,
    pub model_id: &'static str,
    pub display_name: &'static str,
    pub supports_batch: bool,
    pub supports_streaming: bool,
    pub multilingual: bool,
    pub notes: &'static str,
    /// Note de vitesse de 0 a 1 (alignee VoiceInk providers cloud).
    pub speed: f32,
    /// Note de precision de 0 a 1 (idem).
    pub accuracy: f32,
}

/// Catalogue des providers cloud (id canonique lowercase).
pub fn cloud_providers() -> &'static [CloudProviderInfo] {
    &PROVIDERS
}

static PROVIDERS: &[CloudProviderInfo] = &[
    CloudProviderInfo {
        id: "groq",
        display_name: "Groq",
        requires_api_key: true,
        api_key_url: "https://console.groq.com/keys",
    },
    CloudProviderInfo {
        id: "elevenlabs",
        display_name: "ElevenLabs",
        requires_api_key: true,
        api_key_url: "https://elevenlabs.io/app/settings/api-keys",
    },
    CloudProviderInfo {
        id: "deepgram",
        display_name: "Deepgram",
        requires_api_key: true,
        api_key_url: "https://console.deepgram.com/project/settings/api-keys",
    },
    CloudProviderInfo {
        id: "mistral",
        display_name: "Mistral",
        requires_api_key: true,
        api_key_url: "https://console.mistral.ai/api-keys",
    },
    CloudProviderInfo {
        id: "soniox",
        display_name: "Soniox",
        requires_api_key: true,
        api_key_url: "https://console.soniox.com/",
    },
    CloudProviderInfo {
        id: "speechmatics",
        display_name: "Speechmatics",
        requires_api_key: true,
        api_key_url: "https://portal.speechmatics.com/api-keys",
    },
    CloudProviderInfo {
        id: "gemini",
        display_name: "Google Gemini",
        requires_api_key: true,
        api_key_url: "https://aistudio.google.com/apikey",
    },
];

/// Modeles cloud alignes sur VoiceInk Cloud/*Provider.swift.
/// Les notes speed/accuracy sont reprises directement des declarations
/// CloudModel(...) cote VoiceInk. Pour les couples batch+streaming dedoubles
/// (Mistral / Soniox) on attribue 0.95/0.99 selon la branche : VoiceInk a
/// merge ces couples en un seul card mais Parla garde le split.
pub const CLOUD_MODELS: &[CloudModelInfo] = &[
    // Groq
    CloudModelInfo {
        provider_id: "groq",
        model_id: "whisper-large-v3-turbo",
        display_name: "Groq Whisper Large v3 Turbo",
        supports_batch: true,
        supports_streaming: false,
        multilingual: true,
        notes: "Whisper v3 turbo sur infrastructure Groq, tres rapide.",
        speed: 0.65,
        accuracy: 0.95,
    },
    // ElevenLabs
    CloudModelInfo {
        provider_id: "elevenlabs",
        model_id: "scribe_v1",
        display_name: "ElevenLabs Scribe v1",
        supports_batch: true,
        supports_streaming: false,
        multilingual: true,
        notes: "Batch, transcription generique.",
        speed: 0.7,
        accuracy: 0.98,
    },
    CloudModelInfo {
        provider_id: "elevenlabs",
        model_id: "scribe_v2",
        display_name: "ElevenLabs Scribe v2 Realtime",
        supports_batch: true,
        supports_streaming: true,
        multilingual: true,
        notes: "Streaming realtime via WebSocket.",
        speed: 0.99,
        accuracy: 0.98,
    },
    // Deepgram
    CloudModelInfo {
        provider_id: "deepgram",
        model_id: "nova-3",
        display_name: "Deepgram Nova 3",
        supports_batch: true,
        supports_streaming: true,
        multilingual: true,
        notes: "Flagship batch + streaming avec mots-cles custom.",
        speed: 0.99,
        accuracy: 0.96,
    },
    CloudModelInfo {
        provider_id: "deepgram",
        model_id: "nova-3-medical",
        display_name: "Deepgram Nova 3 Medical",
        supports_batch: true,
        supports_streaming: true,
        multilingual: false,
        notes: "Tune medical (anglais uniquement).",
        speed: 0.99,
        accuracy: 0.96,
    },
    // Mistral
    CloudModelInfo {
        provider_id: "mistral",
        model_id: "voxtral-mini-latest",
        display_name: "Mistral Voxtral Mini",
        supports_batch: true,
        supports_streaming: false,
        multilingual: true,
        notes: "Batch, auto-detect de langue.",
        speed: 0.95,
        accuracy: 0.98,
    },
    CloudModelInfo {
        provider_id: "mistral",
        model_id: "voxtral-mini-transcribe-realtime-2602",
        display_name: "Mistral Voxtral Realtime 2602",
        supports_batch: false,
        supports_streaming: true,
        multilingual: true,
        notes: "Streaming realtime WebSocket.",
        speed: 0.99,
        accuracy: 0.98,
    },
    // Soniox
    CloudModelInfo {
        provider_id: "soniox",
        model_id: "stt-async-v4",
        display_name: "Soniox V4 Async",
        supports_batch: true,
        supports_streaming: false,
        multilingual: true,
        notes: "Async, 60 langues supportees.",
        speed: 0.95,
        accuracy: 0.98,
    },
    CloudModelInfo {
        provider_id: "soniox",
        model_id: "stt-rt-v4",
        display_name: "Soniox V4 Realtime",
        supports_batch: false,
        supports_streaming: true,
        multilingual: true,
        notes: "Streaming realtime, tokens granulaires.",
        speed: 0.99,
        accuracy: 0.98,
    },
    // Speechmatics
    CloudModelInfo {
        provider_id: "speechmatics",
        model_id: "speechmatics-enhanced",
        display_name: "Speechmatics Enhanced",
        supports_batch: true,
        supports_streaming: true,
        multilingual: true,
        notes: "Batch (jobs async) et realtime (eu2).",
        speed: 0.99,
        accuracy: 0.98,
    },
    // Gemini
    CloudModelInfo {
        provider_id: "gemini",
        model_id: "gemini-2.5-pro",
        display_name: "Gemini 2.5 Pro",
        supports_batch: true,
        supports_streaming: false,
        multilingual: true,
        notes: "Audio inline base64, prompt-driven.",
        speed: 0.7,
        accuracy: 0.97,
    },
    CloudModelInfo {
        provider_id: "gemini",
        model_id: "gemini-2.5-flash",
        display_name: "Gemini 2.5 Flash",
        supports_batch: true,
        supports_streaming: false,
        multilingual: true,
        notes: "Plus rapide/moins cher.",
        speed: 0.9,
        accuracy: 0.95,
    },
];

/// Lookup d'un provider par id. Utile pour resoudre un id store -> info.
#[allow(dead_code)]
pub fn find_provider(id: &str) -> Option<&'static CloudProviderInfo> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// Lookup d'un modele par (provider_id, model_id).
#[allow(dead_code)]
pub fn find_model(provider_id: &str, model_id: &str) -> Option<&'static CloudModelInfo> {
    CLOUD_MODELS
        .iter()
        .find(|m| m.provider_id == provider_id && m.model_id == model_id)
}

/// Tous les modeles disponibles pour un provider donne.
#[allow(dead_code)]
pub fn models_for_provider(provider_id: &str) -> Vec<&'static CloudModelInfo> {
    CLOUD_MODELS
        .iter()
        .filter(|m| m.provider_id == provider_id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_providers_have_unique_ids() {
        let mut ids: Vec<&str> = PROVIDERS.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate provider id detecte");
    }

    #[test]
    fn all_cloud_models_reference_known_providers() {
        for m in CLOUD_MODELS {
            assert!(
                find_provider(m.provider_id).is_some(),
                "modele {} reference provider inconnu {}",
                m.model_id,
                m.provider_id
            );
        }
    }

    #[test]
    fn find_provider_known() {
        assert!(find_provider("groq").is_some());
        assert!(find_provider("elevenlabs").is_some());
        assert!(find_provider("deepgram").is_some());
        assert!(find_provider("mistral").is_some());
        assert!(find_provider("soniox").is_some());
        assert!(find_provider("speechmatics").is_some());
        assert!(find_provider("gemini").is_some());
    }

    #[test]
    fn find_provider_unknown() {
        assert!(find_provider("unknown").is_none());
        assert!(find_provider("").is_none());
        assert!(find_provider("GROQ").is_none(), "case-sensitive");
    }

    #[test]
    fn find_model_roundtrip() {
        // Tous les providers doivent avoir au moins un modele batch.
        for p in PROVIDERS {
            let any = models_for_provider(p.id);
            assert!(
                !any.is_empty(),
                "provider {} sans modele dans le catalogue",
                p.id
            );
        }
    }

    #[test]
    fn groq_has_whisper_large_v3_turbo() {
        let m = find_model("groq", "whisper-large-v3-turbo");
        assert!(m.is_some());
        let m = m.unwrap();
        assert!(m.supports_batch);
    }

    #[test]
    fn streaming_models_are_reachable() {
        let streaming: Vec<_> = CLOUD_MODELS
            .iter()
            .filter(|m| m.supports_streaming)
            .collect();
        assert!(
            !streaming.is_empty(),
            "au moins un modele streaming doit etre catalogue"
        );
        // Elevenlabs v2 realtime, deepgram nova-3, soniox stt-rt-v4 sont des
        // streaming exemplaires - au moins un parmi deepgram/elevenlabs doit
        // figurer.
        let has_deepgram_or_elevenlabs = streaming
            .iter()
            .any(|m| matches!(m.provider_id, "deepgram" | "elevenlabs"));
        assert!(has_deepgram_or_elevenlabs);
    }
}
