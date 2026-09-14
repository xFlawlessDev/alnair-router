use std::collections::HashMap;
use std::sync::Arc;

use futures::stream::BoxStream;

use crate::llm::model_config::{LlmStreamOptions, ModelConfig};
use crate::llm::providers::{AnthropicNativeProvider, OpenAiProvider};
use crate::llm::types::{ChatError, LlmStreamChunk, Message, ProviderType};

/// A streaming LLM provider contract.
pub trait LlmProvider: Send + Sync {
    fn stream<'a>(
        &'a self,
        config: &'a ModelConfig,
        messages: Vec<Message>,
        options: &'a LlmStreamOptions,
        tools: Option<Vec<serde_json::Value>>,
    ) -> BoxStream<'a, Result<LlmStreamChunk, ChatError>>;
}

/// Registry mapping provider type to an implementation.
#[derive(Clone, Default)]
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn LlmProvider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, provider_type: &ProviderType, provider: Arc<dyn LlmProvider>) {
        self.providers
            .insert(provider_type_to_key(provider_type).to_string(), provider);
    }

    pub fn get(&self, provider_type: &ProviderType) -> Option<&dyn LlmProvider> {
        self.providers
            .get(provider_type_to_key(provider_type))
            .map(|provider| provider.as_ref())
    }

    /// Build a registry with the supported providers registered.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(
            &ProviderType::OpenaiCompatible,
            Arc::new(OpenAiProvider::new()),
        );
        registry.register(
            &ProviderType::AnthropicNative,
            Arc::new(AnthropicNativeProvider::new()),
        );
        registry
    }
}

pub fn provider_type_to_key(provider_type: &ProviderType) -> &'static str {
    match provider_type {
        ProviderType::OpenaiCompatible => "openai-compatible",
        ProviderType::AnthropicNative => "anthropic-native",
    }
}
