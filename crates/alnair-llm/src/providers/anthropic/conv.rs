//! Alnair-to-Anthropic message, tool, and cache-marker conversion.

use super::{
    AnthropicCacheControl, AnthropicContentBlock, AnthropicImageSource, AnthropicMessage,
    AnthropicRequest, AnthropicSystemBlock, AnthropicThinking, AnthropicTool,
};
use crate::model_config::{CacheRetention, LlmStreamOptions, ThinkingLevel};
use crate::types::{ContentPart, Message, MessageContent};

#[allow(private_interfaces)]
pub(crate) fn convert_messages_to_anthropic(
    messages: &[Message],
    tools: Option<&[serde_json::Value]>,
    options: &LlmStreamOptions,
    supports_thinking: bool,
    supports_cache_control: bool,
) -> AnthropicRequest {
    let system_parts: Vec<String> = messages
        .iter()
        .filter(|message| message.role == "system")
        .map(|message| message.content.as_text())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect();

    // Thinking blocks may only be replayed while thinking stays enabled; the
    // API rejects them outright otherwise.
    let thinking = supports_thinking
        .then(|| anthropic_thinking_from_level(options.thinking_level))
        .flatten();
    let replay_thinking = thinking.is_some();

    let mut anthropic_messages = Vec::new();

    for message in messages {
        match message.role.as_str() {
            "system" => continue,
            "tool" => {
                anthropic_messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: vec![AnthropicContentBlock::ToolResult {
                        tool_use_id: message
                            .tool_call_id
                            .clone()
                            .unwrap_or_else(|| "unknown".to_string()),
                        content: message.content.as_text(),
                        cache_control: None,
                    }],
                });
            }
            "user" => {
                let content = content_blocks_from_message_content(&message.content);
                if !content.is_empty() {
                    anthropic_messages.push(AnthropicMessage {
                        role: "user".to_string(),
                        content,
                    });
                }
            }
            "assistant" => {
                let mut content = Vec::new();

                // Thinking blocks from a prior turn (signed, unmodified) must be
                // replayed first; Anthropic rejects a final assistant tool-use
                // message that does not start with thinking when thinking is on.
                if replay_thinking {
                    if let Some(redacted) = &message.redacted_thinking {
                        content.push(AnthropicContentBlock::RedactedThinking {
                            data: redacted.clone(),
                        });
                    }
                    if let (Some(thinking), Some(signature)) = (
                        message.thinking.as_ref(),
                        message.thinking_signature.as_ref(),
                    ) && !thinking.trim().is_empty()
                    {
                        content.push(AnthropicContentBlock::Thinking {
                            thinking: thinking.clone(),
                            signature: signature.clone(),
                        });
                    }
                }

                content.extend(content_blocks_from_message_content(&message.content));

                if let Some(tool_calls) = &message.tool_calls {
                    for tool_call in tool_calls {
                        let input = serde_json::from_str::<serde_json::Value>(&tool_call.arguments)
                            .unwrap_or_else(|_| serde_json::json!({}));
                        content.push(AnthropicContentBlock::ToolUse {
                            id: tool_call.id.clone(),
                            name: tool_call.name.clone(),
                            input,
                        });
                    }
                }

                if !content.is_empty() {
                    anthropic_messages.push(AnthropicMessage {
                        role: "assistant".to_string(),
                        content,
                    });
                }
            }
            _ => {}
        }
    }

    let mark_cache =
        supports_cache_control && !matches!(options.cache_retention, CacheRetention::None);
    let mark_final_tool = mark_cache && system_parts.is_empty() && anthropic_messages.is_empty();
    let final_tool_marker = mark_final_tool
        .then(|| anthropic_cache_control(supports_cache_control, options.cache_retention))
        .flatten();
    let mut max_tokens = options.max_tokens.unwrap_or(8192).max(1) as u32;
    if let Some(thinking) = &thinking
        && max_tokens <= thinking.budget_tokens
    {
        max_tokens = thinking.budget_tokens + 1024;
    }

    let anthropic_messages = merge_consecutive_anthropic_messages(anthropic_messages);
    let system = anthropic_system_blocks(
        system_parts,
        supports_cache_control,
        options.cache_retention,
    );
    let messages = if system.is_none() {
        apply_anthropic_cache_marker(
            anthropic_messages,
            supports_cache_control,
            options.cache_retention,
        )
    } else {
        anthropic_messages
    };

    // Extended thinking constrains sampling: Anthropic rejects `temperature`,
    // `top_p`, and `top_k` alongside a thinking budget.
    let (temperature, top_p, top_k) = if thinking.is_some() {
        (None, None, None)
    } else {
        (options.temperature, options.top_p, options.top_k)
    };

    AnthropicRequest {
        // Filled by provider stream() from ModelConfig.
        model: String::new(),
        max_tokens,
        temperature,
        top_p,
        top_k,
        stop_sequences: options.stop.clone(),
        system,
        messages,
        tools: convert_tools_to_anthropic(tools, final_tool_marker),
        tool_choice: options.tool_choice.as_ref().map(tool_choice_to_anthropic),
        metadata: options
            .user
            .as_ref()
            .map(|user| serde_json::json!({ "user_id": user })),
        output_config: options
            .response_format
            .as_ref()
            .map(output_config_from_format),
        stream: true,
        thinking,
    }
}

/// The Anthropic spelling of a tool choice.
///
/// `required` is `any` here, and a named function is `{"type":"tool","name":…}`
/// rather than the nested OpenAI shape.
fn tool_choice_to_anthropic(choice: &crate::model_config::ToolChoice) -> serde_json::Value {
    use crate::model_config::ToolChoice;
    match choice {
        ToolChoice::Auto => serde_json::json!({ "type": "auto" }),
        ToolChoice::Required => serde_json::json!({ "type": "any" }),
        ToolChoice::None => serde_json::json!({ "type": "none" }),
        ToolChoice::Named { name } => serde_json::json!({ "type": "tool", "name": name }),
    }
}

/// Maps structured output onto `output_config.format`.
///
/// `json_object` has no Anthropic equivalent, so it is not written: the caller
/// is guarded by the capability matrix, which refuses that request before it
/// reaches this provider.
fn output_config_from_format(format: &crate::model_config::ResponseFormat) -> serde_json::Value {
    use crate::model_config::ResponseFormat;
    match format {
        ResponseFormat::JsonSchema { schema, .. } => serde_json::json!({
            "format": { "type": "json_schema", "schema": schema }
        }),
        // A bare `text` request is the default; writing it would be noise.
        ResponseFormat::Text | ResponseFormat::JsonObject => serde_json::json!({}),
    }
}

/// Collapses consecutive same-role turns into one, concatenating their blocks.
///
/// Anthropic's API combines consecutive `user`/`assistant` turns, but
/// Anthropic-compatible and Bedrock endpoints still reject them, and a `tool`
/// message mapped to a `user` turn can land next to a real user turn.
pub(super) fn merge_consecutive_anthropic_messages(
    messages: Vec<AnthropicMessage>,
) -> Vec<AnthropicMessage> {
    let mut merged: Vec<AnthropicMessage> = Vec::with_capacity(messages.len());
    for message in messages {
        match merged.last_mut() {
            Some(last) if last.role == message.role => last.content.extend(message.content),
            _ => merged.push(message),
        }
    }
    merged
}

pub(super) fn anthropic_thinking_from_level(
    level: Option<ThinkingLevel>,
) -> Option<AnthropicThinking> {
    let budget_tokens = match level.unwrap_or(ThinkingLevel::None) {
        ThinkingLevel::None => return None,
        ThinkingLevel::Minimal => 1024,
        ThinkingLevel::Low => 4096,
        ThinkingLevel::Medium => 8192,
        ThinkingLevel::High => 16384,
        ThinkingLevel::Max => 32768,
    };

    Some(AnthropicThinking {
        kind: "enabled".to_string(),
        budget_tokens,
    })
}

pub(super) fn anthropic_cache_control(
    supports_cache_control: bool,
    cache_retention: CacheRetention,
) -> Option<AnthropicCacheControl> {
    if !supports_cache_control || matches!(cache_retention, CacheRetention::None) {
        return None;
    }
    Some(AnthropicCacheControl {
        kind: "ephemeral".to_string(),
        // Long retention uses Anthropic's 1-hour cache TTL, which requires the
        // `extended-cache-ttl-2025-04-11` beta header (sent when retention is Long).
        ttl: matches!(cache_retention, CacheRetention::Long).then(|| "1h".to_string()),
    })
}

pub(super) fn anthropic_system_blocks(
    system_parts: Vec<String>,
    supports_cache_control: bool,
    cache_retention: CacheRetention,
) -> Option<Vec<AnthropicSystemBlock>> {
    if system_parts.is_empty() {
        return None;
    }
    let marker = anthropic_cache_control(supports_cache_control, cache_retention);
    let last_index = system_parts.len().saturating_sub(1);
    Some(
        system_parts
            .into_iter()
            .enumerate()
            .map(|(index, text)| AnthropicSystemBlock {
                kind: "text".to_string(),
                text,
                cache_control: (index == last_index).then(|| marker.clone()).flatten(),
            })
            .collect(),
    )
}

pub(super) fn apply_anthropic_cache_marker(
    mut messages: Vec<AnthropicMessage>,
    supports_cache_control: bool,
    cache_retention: CacheRetention,
) -> Vec<AnthropicMessage> {
    let Some(marker) = anthropic_cache_control(supports_cache_control, cache_retention) else {
        return messages;
    };
    if messages.is_empty() {
        return messages;
    }
    if let Some(block) = messages
        .last_mut()
        .and_then(|message| message.content.last_mut())
    {
        match block {
            AnthropicContentBlock::Text { cache_control, .. }
            | AnthropicContentBlock::ToolResult { cache_control, .. }
            | AnthropicContentBlock::Image { cache_control, .. } => {
                *cache_control = Some(marker);
            }
            AnthropicContentBlock::Thinking { .. }
            | AnthropicContentBlock::RedactedThinking { .. }
            | AnthropicContentBlock::ToolUse { .. } => {}
        }
    }
    messages
}

pub(super) fn content_blocks_from_message_content(
    content: &MessageContent,
) -> Vec<AnthropicContentBlock> {
    match content {
        MessageContent::Text(text) => {
            if text.trim().is_empty() {
                Vec::new()
            } else {
                vec![AnthropicContentBlock::Text {
                    text: text.clone(),
                    cache_control: None,
                }]
            }
        }
        MessageContent::Parts(parts) => {
            let mut blocks = Vec::new();
            for part in parts {
                match part {
                    ContentPart::Text(text_part) => {
                        if !text_part.text.trim().is_empty() {
                            blocks.push(AnthropicContentBlock::Text {
                                text: text_part.text.clone(),
                                cache_control: None,
                            });
                        }
                    }
                    ContentPart::ImageUrl(image_part) => {
                        let url = image_part.image_url.url.as_str();
                        if let Some((media_type, data)) = parse_base64_data_url(url) {
                            blocks.push(AnthropicContentBlock::Image {
                                source: AnthropicImageSource::Base64 { media_type, data },
                                cache_control: None,
                            });
                        } else {
                            blocks.push(AnthropicContentBlock::Image {
                                source: AnthropicImageSource::Url {
                                    url: url.to_string(),
                                },
                                cache_control: None,
                            });
                        }
                    }
                    // The router rejects media parts before they reach a
                    // non-OpenAI provider; dropping one here would let the
                    // model answer a question it never saw.
                    ContentPart::Media(media) => {
                        debug_assert!(
                            false,
                            "media content part reached the Anthropic provider: {}",
                            media.kind()
                        );
                    }
                }
            }
            blocks
        }
    }
}

pub(super) fn parse_base64_data_url(value: &str) -> Option<(String, String)> {
    let prefix = "data:";
    if !value.starts_with(prefix) {
        return None;
    }

    let payload = &value[prefix.len()..];
    let sep = payload.find(";base64,")?;
    let media_type = payload[..sep].trim();
    let data = payload[(sep + ";base64,".len())..].trim();

    if media_type.is_empty() || data.is_empty() {
        return None;
    }

    Some((media_type.to_string(), data.to_string()))
}

pub(super) fn convert_tools_to_anthropic(
    tools: Option<&[serde_json::Value]>,
    final_tool_marker: Option<AnthropicCacheControl>,
) -> Option<Vec<AnthropicTool>> {
    let tools = tools?;
    if tools.is_empty() {
        return None;
    }

    let mut anthropic_tools = Vec::new();
    for tool in tools {
        let function = tool.get("function").unwrap_or(tool);
        let name = function
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        if name.is_empty() {
            continue;
        }

        let description = function
            .get("description")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();

        let mut input_schema = function
            .get("parameters")
            .or_else(|| function.get("input_schema"))
            .cloned()
            .unwrap_or_else(|| serde_json::json!({ "type": "object", "properties": {} }));
        if !matches!(
            input_schema.get("type").and_then(|value| value.as_str()),
            Some("object")
        ) {
            input_schema = serde_json::json!({ "type": "object", "properties": {} });
        }

        anthropic_tools.push(AnthropicTool {
            name,
            description,
            input_schema,
            cache_control: None,
        });
    }

    if let Some(marker) = final_tool_marker
        && let Some(tool) = anthropic_tools.last_mut()
    {
        tool.cache_control = Some(marker);
    }

    if anthropic_tools.is_empty() {
        None
    } else {
        Some(anthropic_tools)
    }
}
