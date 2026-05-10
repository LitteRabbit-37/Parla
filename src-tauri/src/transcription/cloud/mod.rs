// Module cloud - providers de transcription cloud.
//
// Reference VoiceInk : VoiceInk/Transcription/Batch/CloudTranscriptionService.swift
// + implementations LLMkit pour chaque provider.

pub mod cartesia;
pub mod catalog;
pub mod deepgram;
pub mod elevenlabs;
pub mod gemini;
pub mod groq;
pub mod http;
pub mod mistral;
pub mod provider;
pub mod registry;
pub mod soniox;
pub mod speechmatics;
pub mod streaming;
pub mod xai;

pub use registry::CloudRegistry;
