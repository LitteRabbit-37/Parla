// Catalogue des modeles Whisper supportes.
//
// Reference VoiceInk : VoiceInk/Models/PredefinedModels.swift
// - Memes noms, memes tailles, meme source HuggingFace.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperModelInfo {
    /// Identifiant stable (ex: "ggml-large-v3-turbo").
    pub id: &'static str,
    /// Libelle affiche a l'utilisateur.
    pub display_name: &'static str,
    /// Taille approximative en octets (pour barre de progression).
    pub size_bytes: u64,
    /// Vrai si le modele est multilingue, faux si anglais-seul (.en).
    pub multilingual: bool,
    /// URL HuggingFace de telechargement.
    pub url: &'static str,
    /// Commentaire indicatif sur les performances / cas d'usage.
    pub notes: &'static str,
    /// Note de vitesse de 0 a 1 (alignee VoiceInk TranscriptionModelRegistry.swift).
    pub speed: f32,
    /// Note de precision de 0 a 1 (idem VoiceInk).
    pub accuracy: f32,
}

/// Catalogue strictement aligne sur VoiceInk TranscriptionModelRegistry.swift.
pub const WHISPER_MODELS: &[WhisperModelInfo] = &[
    WhisperModelInfo {
        id: "ggml-tiny",
        display_name: "Tiny (multilingue)",
        size_bytes: 75_000_000,
        multilingual: true,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        notes: "Tres rapide, precision limitee. Bon pour tests.",
        speed: 0.95,
        accuracy: 0.6,
    },
    WhisperModelInfo {
        id: "ggml-tiny.en",
        display_name: "Tiny (anglais)",
        size_bytes: 75_000_000,
        multilingual: false,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin",
        notes: "Anglais uniquement, un peu meilleur que tiny multilingue sur EN.",
        speed: 0.95,
        accuracy: 0.65,
    },
    WhisperModelInfo {
        id: "ggml-base",
        display_name: "Base (multilingue)",
        size_bytes: 142_000_000,
        multilingual: true,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        notes: "Bon compromis taille/precision pour CPU.",
        speed: 0.85,
        accuracy: 0.72,
    },
    WhisperModelInfo {
        id: "ggml-base.en",
        display_name: "Base (anglais)",
        size_bytes: 142_000_000,
        multilingual: false,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
        notes: "Anglais uniquement, meilleur que base multilingue sur EN.",
        speed: 0.85,
        accuracy: 0.75,
    },
    WhisperModelInfo {
        id: "ggml-large-v2",
        display_name: "Large v2 (multilingue)",
        size_bytes: 2_900_000_000,
        multilingual: true,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v2.bin",
        notes: "Tres bonne precision, necessite beaucoup de RAM.",
        speed: 0.3,
        accuracy: 0.96,
    },
    WhisperModelInfo {
        id: "ggml-large-v3",
        display_name: "Large v3 (multilingue)",
        size_bytes: 2_900_000_000,
        multilingual: true,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin",
        notes: "Derniere generation, precision maximale en CPU lourd.",
        speed: 0.3,
        accuracy: 0.98,
    },
    WhisperModelInfo {
        id: "ggml-large-v3-turbo",
        display_name: "Large v3 Turbo",
        size_bytes: 1_500_000_000,
        multilingual: true,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
        notes: "Large v3 optimise pour la vitesse, recommande avec CUDA.",
        speed: 0.75,
        accuracy: 0.97,
    },
    WhisperModelInfo {
        id: "ggml-large-v3-turbo-q5_0",
        display_name: "Large v3 Turbo (Q5_0)",
        size_bytes: 547_000_000,
        multilingual: true,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        notes: "Quantise, tres bon rapport qualite/taille pour CPU.",
        speed: 0.75,
        accuracy: 0.95,
    },
];

pub fn find_model(id: &str) -> Option<&'static WhisperModelInfo> {
    WHISPER_MODELS.iter().find(|m| m.id == id)
}
