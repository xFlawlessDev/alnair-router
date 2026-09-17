use futures::stream::BoxStream;

use crate::model_config::{LlmStreamOptions, ModelConfig};
use crate::provider::LlmProvider;
use crate::providers::openai::OpenAiProvider;
use crate::types::{ChatError, LlmStreamChunk, Message};

mod request;

#[cfg(test)]
mod tests;

pub struct CodeBuddyIntlProvider {
    openai: OpenAiProvider,
}

impl CodeBuddyIntlProvider {
    pub fn new() -> Self {
        Self {
            openai: OpenAiProvider::new(),
        }
    }
}

impl Default for CodeBuddyIntlProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmProvider for CodeBuddyIntlProvider {
    fn stream<'a>(
        &'a self,
        config: &'a ModelConfig,
        messages: Vec<Message>,
        options: &'a LlmStreamOptions,
        tools: Option<Vec<serde_json::Value>>,
    ) -> BoxStream<'a, Result<LlmStreamChunk, ChatError>> {
        let messages = request::transform_messages(messages);
        self.openai.stream_with_transform(
            config,
            messages,
            options,
            tools,
            true,
            request::transform_body,
        )
    }
}
