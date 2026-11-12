//! Vendored LLM provider layer.
//!
//! A self-contained provider stack kept inside the crate so the router builds
//! with no path dependency on anything outside this repository. OpenAI
//! chat-completions, Anthropic Messages, Command Code, CodeBuddy Intl and the
//! ChatGPT/Codex Responses backend are supported.
//!
//! Everything the router needs from upstream lives behind
//! `crate::upstream::chat_backend`.

pub mod model_config;
pub mod provider;
pub mod providers;
pub mod types;

pub use model_config::{
    AuthStyle, CacheRetention, Citation, CostTier, EffectiveCostRates, LlmStreamOptions,
    ModelConfig, ModelCostRates, ResponseFormat, ThinkingLevel, ToolChoice, WireApi,
    response_format_to_openai, tool_choice_to_openai,
};
pub use provider::{
    LlmProvider, ProviderCapabilities, ProviderRegistry, WebRequest, provider_type_to_key,
};
pub use providers::{
    AnthropicNativeProvider, CodeBuddyIntlProvider, CodexProvider, CommandCodeProvider,
    OpenAiProvider,
};
pub use types::{
    ChatError, ContentPart, GenerationOptions, ImageUrl, ImageUrlContentPart, LlmStreamChunk,
    MediaContentPart, Message, MessageContent, MessageToolCall, ProviderType, TextContentPart,
};
