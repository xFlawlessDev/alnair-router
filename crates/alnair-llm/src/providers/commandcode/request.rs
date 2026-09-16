//! Request shaping for both Command Code transports: the documented Provider
//! API (OpenAI body) and the CLI envelope used by `/alpha/generate`.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use crate::model_config::{LlmStreamOptions, ModelConfig, ThinkingLevel};
use crate::types::Message;

/// Server-side ceiling for a client-supplied `max_tokens`; the endpoint rejects
/// anything larger instead of truncating it.
pub(super) const MAX_TOKENS_CEILING: i32 = 200_000;

/// Version the CLI transport advertises. Command Code can require a newer CLI,
/// so a connection can override this header through `custom_headers`.
pub(super) const CLI_VERSION: &str = "1.15.1";

/// Tool names the CLI endpoint reserves for its own features.
const RESERVED_TOOL_NAMES: [&str; 1] = ["tool_search"];

/// Bare model ids the endpoint serves under a vendor prefix.
const BARE_MODEL_PREFIXES: [(&str, &str); 2] = [
    ("mimo-v2.5", "xiaomi/mimo-v2.5"),
    ("mimo-v2.5-pro", "xiaomi/mimo-v2.5-pro"),
];

/// Renames reserved tools out of the way, and back again for responses.
#[derive(Debug, Default, Clone)]
pub(super) struct ToolNames {
    client: HashMap<String, String>,
}

impl ToolNames {
    fn wire_name(&mut self, client_name: &str) -> String {
        if !RESERVED_TOOL_NAMES.contains(&client_name) {
            return client_name.to_string();
        }

        let wire = format!("alnair_{client_name}");
        self.client.insert(wire.clone(), client_name.to_string());
        wire
    }

    /// Maps a wire name back onto the name the client asked for.
    pub(super) fn client_name(&self, wire_name: &str) -> String {
        self.client
            .get(wire_name)
            .cloned()
            .unwrap_or_else(|| wire_name.to_string())
    }
}

/// Headers identifying the CLI transport. Overridden by connection headers.
pub(super) fn cli_headers() -> [(&'static str, String); 6] {
    [
        ("x-command-code-version", CLI_VERSION.to_string()),
        ("x-cli-environment", "external".to_string()),
        ("x-project-slug", "alnair-router".to_string()),
        ("x-taste-learning", "false".to_string()),
        ("x-co-flag", "false".to_string()),
        ("x-session-id", uuid::Uuid::new_v4().to_string()),
    ]
}

/// Provider API body: plain OpenAI chat completions, with the endpoint's
/// `max_tokens` ceiling applied.
pub(super) fn official_options(options: &LlmStreamOptions) -> LlmStreamOptions {
    let mut clamped = options.clone();
    clamped.max_tokens = clamped.max_tokens.map(|max| max.min(MAX_TOKENS_CEILING));
    clamped
}

/// CLI body: the CLI wraps the params in a session envelope.
pub(super) fn build_cli_body(
    config: &ModelConfig,
    messages: &[Message],
    options: &LlmStreamOptions,
    tools: Option<&[Value]>,
) -> (Value, ToolNames) {
    let mut names = ToolNames::default();
    let (system, messages) = convert_messages(messages, &mut names);

    let mut params = json!({
        "model": wire_model(&config.model_id),
        "messages": messages,
        "tools": convert_tools(tools, &mut names),
        "system": system,
        "stream": true,
    });
    if let Some(max_tokens) = options
        .max_tokens
        .filter(|max| *max > 0)
        .map(|max| max.min(MAX_TOKENS_CEILING))
    {
        params["max_tokens"] = json!(max_tokens);
    }
    if let Some(level) = options
        .thinking_level
        .filter(|level| !matches!(level, ThinkingLevel::None))
    {
        params["reasoning_effort"] = serde_json::to_value(level).unwrap_or(Value::Null);
    }

    let body = json!({
        "config": {
            "workingDir": "/workspace",
            "date": chrono::Utc::now().format("%Y-%m-%d").to_string(),
            "environment": "external",
            "structure": [],
            "isGitRepo": false,
            "currentBranch": "",
            "mainBranch": "",
            "gitStatus": "",
            "recentCommits": [],
        },
        "memory": "",
        "taste": "",
        "skills": "",
        "permissionMode": "standard",
        "params": params,
    });

    (body, names)
}

/// Maps a model id onto the vendor-prefixed id the endpoint serves.
pub(super) fn wire_model(model: &str) -> String {
    let trimmed = model.trim();
    let bare = trimmed
        .strip_prefix("command-code/")
        .or_else(|| trimmed.strip_prefix("cmd/"))
        .unwrap_or(trimmed);

    if bare.contains('/') {
        return bare.to_string();
    }

    BARE_MODEL_PREFIXES
        .iter()
        .find(|(id, _)| *id == bare)
        .map_or_else(|| bare.to_string(), |(_, wire)| (*wire).to_string())
}

/// Splits the system prompt out and converts the rest into CLI message parts.
fn convert_messages(messages: &[Message], names: &mut ToolNames) -> (String, Vec<Value>) {
    let calls = tool_calls_by_id(messages);
    let paired = paired_tool_calls(messages, &calls);

    let mut system: Vec<String> = Vec::new();
    let mut converted: Vec<Value> = Vec::new();

    for message in messages {
        match message.role.as_str() {
            "system" | "developer" => {
                let text = message.content.as_text();
                if !text.is_empty() {
                    system.push(text);
                }
            }
            "tool" => {
                let Some(id) = message.tool_call_id.as_deref() else {
                    continue;
                };
                if !paired.contains(id) {
                    continue;
                }
                let (name, arguments) = calls.get(id).cloned().unwrap_or_default();
                converted.push(json!({
                    "role": "tool",
                    "content": [{
                        "type": "tool-result",
                        "toolCallId": id,
                        "toolName": names.wire_name(&name),
                        "arguments": arguments,
                        "output": { "type": "text", "value": message.content.as_text() },
                    }],
                }));
            }
            "assistant" => {
                let mut parts: Vec<Value> = Vec::new();
                let text = message.content.as_text();
                if !text.is_empty() {
                    parts.push(json!({ "type": "text", "text": text }));
                }
                for call in message.tool_calls.iter().flatten() {
                    if !paired.contains(&call.id) {
                        continue;
                    }
                    parts.push(json!({
                        "type": "tool-call",
                        "toolCallId": call.id,
                        "toolName": names.wire_name(&call.name),
                        "input": parse_arguments(&call.arguments),
                        "arguments": call.arguments,
                    }));
                }
                if !parts.is_empty() {
                    converted.push(json!({ "role": "assistant", "content": parts }));
                }
            }
            _ => converted.push(json!({
                "role": "user",
                "content": message.content.as_text(),
            })),
        }
    }

    (system.join("\n\n"), converted)
}

/// `tool_call_id` → `(name, arguments)` for every assistant tool call.
fn tool_calls_by_id(messages: &[Message]) -> HashMap<String, (String, String)> {
    messages
        .iter()
        .filter(|message| message.role == "assistant")
        .filter_map(|message| message.tool_calls.as_ref())
        .flatten()
        .map(|call| (call.id.clone(), (call.name.clone(), call.arguments.clone())))
        .collect()
}

/// Tool calls are only forwarded when a matching tool result is in the
/// conversation, and vice versa — the endpoint rejects dangling ones.
fn paired_tool_calls(
    messages: &[Message],
    calls: &HashMap<String, (String, String)>,
) -> HashSet<String> {
    let results: HashSet<&str> = messages
        .iter()
        .filter(|message| message.role == "tool")
        .filter_map(|message| message.tool_call_id.as_deref())
        .collect();

    calls
        .keys()
        .filter(|id| results.contains(id.as_str()))
        .cloned()
        .collect()
}

fn convert_tools(tools: Option<&[Value]>, names: &mut ToolNames) -> Vec<Value> {
    tools
        .unwrap_or_default()
        .iter()
        .filter_map(|tool| {
            let function = tool.get("function").unwrap_or(tool);
            let name = function.get("name").and_then(Value::as_str)?;
            Some(json!({
                "type": "function",
                "name": names.wire_name(name),
                "description": function
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                "input_schema": function
                    .get("parameters")
                    .filter(|parameters| parameters.is_object())
                    .cloned()
                    .unwrap_or_else(|| json!({})),
            }))
        })
        .collect()
}

fn parse_arguments(arguments: &str) -> Value {
    serde_json::from_str(arguments)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}
