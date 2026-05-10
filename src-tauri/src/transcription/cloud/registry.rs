// Registry des providers cloud : dispatch par id canonique lowercase.

use std::sync::Arc;

use super::deepgram::DeepgramProvider;
use super::elevenlabs::ElevenLabsProvider;
use super::gemini::GeminiProvider;
use super::groq::GroqProvider;
use super::mistral::MistralProvider;
use super::provider::CloudTranscriptionProvider;
use super::soniox::SonioxProvider;
use super::speechmatics::SpeechmaticsProvider;
use super::xai::XaiProvider;

pub struct CloudRegistry {
    providers: Vec<Arc<dyn CloudTranscriptionProvider>>,
}

impl Default for CloudRegistry {
    fn default() -> Self {
        Self {
            providers: vec![
                Arc::new(GroqProvider),
                Arc::new(ElevenLabsProvider),
                Arc::new(DeepgramProvider),
                Arc::new(MistralProvider),
                Arc::new(SonioxProvider),
                Arc::new(SpeechmaticsProvider),
                Arc::new(GeminiProvider),
                Arc::new(XaiProvider),
            ],
        }
    }
}

impl CloudRegistry {
    pub fn find(&self, provider_id: &str) -> Option<Arc<dyn CloudTranscriptionProvider>> {
        let id = provider_id.to_lowercase();
        self.providers.iter().find(|p| p.id() == id).cloned()
    }
}
