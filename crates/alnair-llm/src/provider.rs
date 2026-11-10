use std::collections::HashMap;
use std::sync::Arc;

use futures::future::BoxFuture;
use futures::stream::BoxStream;

use crate::model_config::{LlmStreamOptions, ModelConfig};
use crate::providers::{
    AnthropicNativeProvider, CodeBuddyIntlProvider, CodexProvider, CommandCodeProvider,
    OpenAiProvider,
};
use crate::types::{ChatError, LlmStreamChunk, Message, ProviderType};

/// Hosted web tools available within a provider's model requests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProviderCapabilities {
    pub web_search: bool,
    /// Reading pages through the hosted web tool; not a raw HTTP fetch API.
    pub web_fetch: bool,
}

#[derive(Debug, Clone)]
pub enum WebRequest {
    Search { query: String, max_results: usize },
    Fetch { url: String, format: String },
}

/// A streaming LLM provider contract.
pub trait LlmProvider: Send + Sync {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::default()
    }

    fn web_request<'a>(
        &'a self,
        _config: &'a ModelConfig,
        _request: &'a WebRequest,
    ) -> BoxFuture<'a, Result<serde_json::Value, ChatError>> {
        Box::pin(async {
            Err(ChatError::BadRequest(
                "provider has no hosted web API".to_string(),
            ))
        })
    }

    fn stream<'a>(
        &'a self,
        config: &'a ModelConfig,
        messages: Vec<Message>,
        options: &'a LlmStreamOptions,
        tools: Option<Vec<serde_json::Value>>,
    ) -> BoxStream<'a, Result<LlmStreamChunk, ChatError>>;

    /// One-shot completion, still surfaced as a stream of chunks so the
    /// executor and collector can treat both paths uniformly.
    ///
    /// The default drains [`Self::stream`]; providers with a real
    /// non-streaming endpoint (no SSE overhead, no chunk reassembly) override it.
    fn complete<'a>(
        &'a self,
        config: &'a ModelConfig,
        messages: Vec<Message>,
        options: &'a LlmStreamOptions,
        tools: Option<Vec<serde_json::Value>>,
    ) -> BoxStream<'a, Result<LlmStreamChunk, ChatError>> {
        self.stream(config, messages, options, tools)
    }
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
        registry.register(
            &ProviderType::CommandCode,
            Arc::new(CommandCodeProvider::new()),
        );
        registry.register(
            &ProviderType::CodeBuddyIntl,
            Arc::new(CodeBuddyIntlProvider::new()),
        );
        registry.register(&ProviderType::OpenAiCodex, Arc::new(CodexProvider::new()));
        registry
    }
}

pub fn provider_type_to_key(provider_type: &ProviderType) -> &'static str {
    provider_type.as_key()
}
