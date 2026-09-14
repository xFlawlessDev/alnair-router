//! Wire-protocol translation: OpenAI and Anthropic shapes ↔ router types.

pub mod anthropic;
pub mod openai;

pub use openai::ChatCompletionRequest;
