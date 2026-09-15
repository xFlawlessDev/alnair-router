//! SSE chunk parsing, tool-call repair, and embedded-reasoning extraction.

use tracing::warn;

use crate::providers::common::parse_tool_arguments_strict;
use crate::types::{ChatError, LlmStreamChunk};

#[cfg(test)]
pub(super) fn drain_complete_sse_lines(buffer: &mut String, chunk: &str) -> Vec<String> {
    buffer.push_str(chunk);

    let mut lines = Vec::new();
    while let Some(newline_index) = buffer.find('\n') {
        let mut line: String = buffer.drain(..=newline_index).collect();
        if line.ends_with('\n') {
            line.pop();
        }
        if line.ends_with('\r') {
            line.pop();
        }
        lines.push(line);
    }

    lines
}

pub(super) fn extract_tool_call_name(tool_call: &serde_json::Value) -> Option<&str> {
    tool_call
        .get("function")
        .and_then(|function| function.get("name"))
        .and_then(|name| name.as_str())
        .or_else(|| tool_call.get("name").and_then(|name| name.as_str()))
        .or_else(|| {
            tool_call
                .get("function_name")
                .and_then(|name| name.as_str())
        })
        .or_else(|| tool_call.get("tool_name").and_then(|name| name.as_str()))
        .filter(|name| !name.trim().is_empty())
}

pub(super) fn extract_tool_call_arguments_fragment(
    tool_call: &serde_json::Value,
) -> Option<String> {
    let arguments = tool_call
        .get("function")
        .and_then(|function| function.get("arguments"))
        .or_else(|| tool_call.get("arguments"))?;

    if let Some(arguments) = arguments.as_str() {
        return Some(arguments.to_string());
    }

    Some(arguments.to_string())
}

pub(super) fn build_tool_call_chunk(
    id: String,
    name: String,
    arguments: &str,
) -> Result<Option<LlmStreamChunk>, ChatError> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Ok(None);
    }

    Ok(Some(LlmStreamChunk::ToolCall {
        id,
        arguments: parse_tool_call_arguments_lossy(arguments, &name)?,
        name,
    }))
}

pub(super) fn parse_tool_call_arguments_lossy(
    raw: &str,
    tool_name: &str,
) -> Result<serde_json::Value, ChatError> {
    match parse_tool_arguments_strict(raw) {
        Ok(arguments) => Ok(arguments),
        Err(error) => {
            let escaped = escape_invalid_json_backslashes(raw);
            if escaped != raw
                && let Ok(arguments) = parse_tool_arguments_strict(&escaped)
            {
                warn!(
                    tool_name,
                    raw_preview = %truncate_for_log(raw, 240),
                    "repaired provider tool call JSON arguments with invalid backslash escapes"
                );
                return Ok(arguments);
            }

            let repaired = repair_incomplete_json(raw);
            if repaired != raw
                && let Ok(arguments) = parse_tool_arguments_strict(&repaired)
            {
                warn!(
                    tool_name,
                    raw_preview = %truncate_for_log(raw, 240),
                    "repaired incomplete provider tool call JSON arguments"
                );
                return Ok(arguments);
            }

            let escaped_repaired = escape_invalid_json_backslashes(&repaired);
            if escaped_repaired != repaired
                && let Ok(arguments) = parse_tool_arguments_strict(&escaped_repaired)
            {
                warn!(
                    tool_name,
                    raw_preview = %truncate_for_log(raw, 240),
                    "repaired provider tool call JSON arguments with incomplete JSON and invalid backslash escapes"
                );
                return Ok(arguments);
            }

            warn!(
                tool_name,
                error = %error,
                raw_preview = %truncate_for_log(raw, 240),
                "provider returned malformed tool call JSON arguments"
            );
            Err(error)
        }
    }
}

pub(super) fn escape_invalid_json_backslashes(raw: &str) -> String {
    let mut repaired = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    let mut changed = false;

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            repaired.push(ch);
            continue;
        }

        match chars.peek().copied() {
            Some('"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' | 'u') => {
                repaired.push(ch);
            }
            Some(_) | None => {
                repaired.push('\\');
                repaired.push('\\');
                changed = true;
            }
        }
    }

    if changed { repaired } else { raw.to_string() }
}

pub(super) fn repair_incomplete_json(raw: &str) -> String {
    let mut repaired = trim_to_complete_json_prefix(raw.trim());
    if repaired.is_empty() {
        repaired = raw.trim().to_string();
    }

    let mut stack = Vec::new();
    let mut in_string = false;
    let mut escaped = false;

    for ch in repaired.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' if stack.last().copied() == Some(ch) => {
                stack.pop();
            }
            _ => {}
        }
    }

    if in_string {
        repaired.push('"');
    }

    while matches!(repaired.chars().last(), Some(',' | ':')) {
        repaired.pop();
    }

    while let Some(ch) = stack.pop() {
        repaired.push(ch);
    }

    repaired
}

pub(super) fn trim_to_complete_json_prefix(raw: &str) -> String {
    let mut last_boundary = None;
    let mut stack = Vec::new();
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in raw.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
                if stack.last().copied() == Some(':') {
                    stack.pop();
                }
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            ':' => stack.push(':'),
            ',' => {
                while stack.last().copied() == Some(':') {
                    stack.pop();
                }
                last_boundary = Some(idx);
            }
            '}' | ']' => {
                while stack.last().copied() == Some(':') {
                    stack.pop();
                }
                if stack.last().copied() == Some(ch) {
                    stack.pop();
                    last_boundary = Some(idx + ch.len_utf8());
                }
            }
            _ => {}
        }
    }

    if stack.is_empty() && !in_string {
        return raw.to_string();
    }

    last_boundary
        .map(|idx| raw[..idx].trim_end_matches(',').trim().to_string())
        .unwrap_or_default()
}

pub(super) fn truncate_for_log(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

pub(super) fn extract_usage(value: &serde_json::Value) -> Option<&serde_json::Value> {
    value.get("usage").or_else(|| {
        value
            .get("choices")
            .and_then(|choices| choices.as_array())
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("usage"))
    })
}

pub(super) fn extract_reasoning_text(value: &serde_json::Value) -> Option<&str> {
    value
        .get("reasoning_content")
        .or_else(|| value.get("reasoning"))
        .or_else(|| value.get("reasoning_text"))
        .or_else(|| value.get("thinking"))
        .and_then(|v| v.as_str())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParsedEmbeddedThinking {
    pub(super) thinking: Option<String>,
    pub(super) text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ParsedEmbeddedThinkingChunk {
    Thinking(String),
    Text(String),
}

#[derive(Default)]
pub(super) struct EmbeddedThinkingStreamParser {
    buffer: String,
    buffering: bool,
    passthrough: bool,
}

impl EmbeddedThinkingStreamParser {
    pub(super) fn push(&mut self, chunk: &str) -> Vec<ParsedEmbeddedThinkingChunk> {
        if self.passthrough {
            return vec![ParsedEmbeddedThinkingChunk::Text(chunk.to_string())];
        }

        self.buffer.push_str(chunk);
        if !self.buffering {
            if starts_like_embedded_thinking(&self.buffer) {
                self.buffering = true;
            } else if could_be_embedded_thinking_prefix(&self.buffer) {
                return Vec::new();
            } else {
                self.passthrough = true;
                return vec![ParsedEmbeddedThinkingChunk::Text(std::mem::take(
                    &mut self.buffer,
                ))];
            }
        }

        if !self.buffer.to_ascii_lowercase().contains("</think>") {
            return Vec::new();
        }

        let parsed = split_embedded_thinking(&self.buffer);
        self.buffer.clear();
        self.buffering = false;
        self.passthrough = true;
        parsed.into_chunks()
    }

    pub(super) fn flush(&mut self) -> Vec<ParsedEmbeddedThinkingChunk> {
        if self.buffer.is_empty() {
            return Vec::new();
        }

        if self.buffering {
            let buffer = std::mem::take(&mut self.buffer);
            if buffer.to_ascii_lowercase().contains("</think>")
                || starts_like_untagged_reasoning(&buffer)
            {
                return split_embedded_thinking(&buffer).into_chunks();
            }
            return vec![ParsedEmbeddedThinkingChunk::Text(buffer)];
        }

        vec![ParsedEmbeddedThinkingChunk::Text(std::mem::take(
            &mut self.buffer,
        ))]
    }
}

impl ParsedEmbeddedThinking {
    fn into_chunks(self) -> Vec<ParsedEmbeddedThinkingChunk> {
        let mut chunks = Vec::new();
        if let Some(thinking) = self.thinking
            && !thinking.is_empty()
        {
            chunks.push(ParsedEmbeddedThinkingChunk::Thinking(thinking));
        }
        if !self.text.is_empty() {
            chunks.push(ParsedEmbeddedThinkingChunk::Text(self.text));
        }
        chunks
    }
}

pub(super) fn split_embedded_thinking(content: &str) -> ParsedEmbeddedThinking {
    let lower = content.to_ascii_lowercase();
    let Some(close_start) = lower.find("</think>") else {
        if starts_like_untagged_reasoning(content) {
            return split_untagged_reasoning(content);
        }

        return ParsedEmbeddedThinking {
            thinking: None,
            text: content.to_string(),
        };
    };

    let close_end = close_start + "</think>".len();
    let thinking = clean_embedded_thinking(&content[..close_start]);
    let text = content[close_end..].trim_start().to_string();
    ParsedEmbeddedThinking {
        thinking: if thinking.is_empty() {
            None
        } else {
            Some(thinking)
        },
        text,
    }
}

pub(super) fn clean_embedded_thinking(raw: &str) -> String {
    let mut value = raw.trim().to_string();
    let lower = value.to_ascii_lowercase();
    if let Some(open_start) = lower.find("<think>") {
        value = value[open_start + "<think>".len()..].trim().to_string();
    }

    for prefix in [
        "here's a thinking process:",
        "here is a thinking process:",
        "thinking process:",
    ] {
        if value.to_ascii_lowercase().starts_with(prefix) {
            value = value[prefix.len()..].trim_start().to_string();
            break;
        }
    }

    value
}

pub(super) fn starts_like_embedded_thinking(content: &str) -> bool {
    let lower = content.trim_start().to_ascii_lowercase();
    lower.starts_with("<think")
        || lower.starts_with("here's a thinking process")
        || lower.starts_with("here is a thinking process")
        || lower.starts_with("thinking process")
        || starts_like_untagged_reasoning(content)
}

pub(super) fn could_be_embedded_thinking_prefix(content: &str) -> bool {
    let lower = content.trim_start().to_ascii_lowercase();
    if lower.len() >= "here is a thinking process".len() {
        return false;
    }
    [
        "<think",
        "here's a thinking process",
        "here is a thinking process",
        "thinking process",
    ]
    .iter()
    .any(|marker| marker.starts_with(lower.as_str()))
}

pub(super) fn starts_like_untagged_reasoning(content: &str) -> bool {
    let lower = content.trim_start().to_ascii_lowercase();
    [
        "the user is ",
        "the user asks ",
        "user asks ",
        "we need to ",
        "i need to ",
        "i will ",
        "let me ",
    ]
    .iter()
    .any(|marker| lower.starts_with(marker))
}

pub(super) fn split_untagged_reasoning(content: &str) -> ParsedEmbeddedThinking {
    let trimmed = content.trim();
    let Some((thinking, answer)) = split_on_blank_line_before_final_answer(trimmed) else {
        return ParsedEmbeddedThinking {
            thinking: None,
            text: content.to_string(),
        };
    };

    ParsedEmbeddedThinking {
        thinking: Some(thinking.trim().to_string()),
        text: answer.trim_start().to_string(),
    }
}

pub(super) fn split_on_blank_line_before_final_answer(content: &str) -> Option<(&str, &str)> {
    let split_markers = ["\r\n\r\n", "\n\n"];
    let mut candidates = Vec::new();
    for marker in split_markers {
        let mut offset = 0;
        while let Some(position) = content[offset..].find(marker) {
            let split_at = offset + position + marker.len();
            let answer = &content[split_at..];
            if looks_like_final_answer(answer) {
                candidates.push((offset + position, split_at));
            }
            offset = split_at;
        }
    }

    candidates
        .into_iter()
        .max_by_key(|(_, split_at)| *split_at)
        .map(|(marker_start, split_at)| (&content[..marker_start], &content[split_at..]))
}

pub(super) fn looks_like_final_answer(value: &str) -> bool {
    let answer = value.trim_start();
    if answer.is_empty() || answer.len() > 600 {
        return false;
    }

    let lower = answer.to_ascii_lowercase();
    let starts_like_answer = [
        "hello", "hi", "the ", "a ", "an ", "yes", "no", "sure", "i can ", "i can't ", "here ",
    ]
    .iter()
    .any(|marker| lower.starts_with(marker));
    starts_like_answer && !starts_like_untagged_reasoning(answer)
}
