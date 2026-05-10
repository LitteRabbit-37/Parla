// Streaming WebSocket pour les providers cloud qui le supportent.
//
// Reference VoiceInk :
// - VoiceInk/Transcription/Streaming/*StreamingProvider.swift (wrappers)
// - LLMkit : DeepgramStreamingClient, ElevenLabsStreamingClient,
//   MistralStreamingClient, SonioxStreamingClient, SpeechmaticsStreamingClient.
//
// Audio format commun impose par VoiceInk StreamingTranscriptionProvider.swift
// L41 : 16-bit PCM, 16 kHz, mono, little-endian. Deja le format natif de
// AudioRecorder de Parla.

pub mod deepgram;
pub mod elevenlabs;
pub mod mistral;
pub mod registry;
pub mod service;
pub mod session;
pub mod soniox;
pub mod speechmatics;
pub mod xai;

pub use registry::StreamingRegistry;
pub use service::start_streaming;
pub use session::{StreamingConfig, StreamingHandle};
