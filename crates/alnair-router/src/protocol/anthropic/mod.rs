//! Anthropic Messages API translation.
//!
//! - [`request`] — `MessagesRequest`, its conversion onto the hub, and the
//!   token-count estimate.
//! - [`response`] — `MessagesResponse`, its content blocks, and the
//!   `count_tokens` reply.

mod request;
mod response;

pub use request::{
    AnthropicContent, AnthropicContentBlock, AnthropicImageSource, AnthropicMessage,
    AnthropicThinkingRequest, MessagesRequest, SystemPrompt, estimate_messages, estimate_tokens,
};
pub use response::{AnthropicResponseBlock, AnthropicUsage, CountTokensResponse, MessagesResponse};

#[cfg(test)]
mod tests;
