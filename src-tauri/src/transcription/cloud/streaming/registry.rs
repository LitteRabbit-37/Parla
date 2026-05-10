// Registry des providers streaming.

use std::sync::Arc;

use super::cartesia::CartesiaStreaming;
use super::deepgram::DeepgramStreaming;
use super::elevenlabs::ElevenLabsStreaming;
use super::mistral::MistralStreaming;
use super::session::StreamingProvider;
use super::soniox::SonioxStreaming;
use super::speechmatics::SpeechmaticsStreaming;
use super::xai::XaiStreaming;

pub struct StreamingRegistry {
    providers: Vec<Arc<dyn StreamingProvider>>,
}

impl Default for StreamingRegistry {
    fn default() -> Self {
        Self {
            providers: vec![
                Arc::new(DeepgramStreaming),
                Arc::new(ElevenLabsStreaming),
                Arc::new(MistralStreaming),
                Arc::new(SonioxStreaming),
                Arc::new(SpeechmaticsStreaming),
                Arc::new(XaiStreaming),
                Arc::new(CartesiaStreaming),
            ],
        }
    }
}

impl StreamingRegistry {
    pub fn find(&self, provider_id: &str) -> Option<Arc<dyn StreamingProvider>> {
        let id = provider_id.to_lowercase();
        self.providers.iter().find(|p| p.id() == id).cloned()
    }
}
