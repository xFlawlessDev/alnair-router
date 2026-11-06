//! Vendored LLM provider layer.
//!
//! A self-contained provider stack kept inside the crate so the router builds
//! with no path dependency on anything outside this repository. OpenAI
//! chat-completions, Anthropic Messages, Command Code, CodeBuddy Intl and the
//! ChatGPT/Codex Responses backend are supported — Ollama was removed along
//! with its provider module.
//!
//! Everything the router needs from upstream lives behind
//! `crate::upstream::chat_backend`.

pub mod model_config;
pub mod provider;
pub mod providers;
pub mod types;

pub use model_config::{
    AuthStyle, CacheRetention, LlmStreamOptions, ModelConfig, ModelCostRates, ThinkingLevel,
    WireApi, known_cost_rates,
};
pub use provider::{LlmProvider, ProviderRegistry, provider_type_to_key};
pub use providers::{
    AnthropicNativeProvider, CodeBuddyIntlProvider, CodexProvider, CommandCodeProvider,
    OpenAiProvider,
};
pub use types::{
    ChatError, ContentPart, GenerationOptions, ImageUrl, ImageUrlContentPart, LlmStreamChunk,
    MediaContentPart, Message, MessageContent, MessageToolCall, ProviderType, TextContentPart,
};
