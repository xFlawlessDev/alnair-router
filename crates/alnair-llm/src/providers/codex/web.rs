//! Hosted search and page reading use the Codex Responses tool protocol.
use super::request;
use crate::{
    model_config::{LlmStreamOptions, ModelConfig},
    provider::WebRequest,
    types::{ChatError, Message},
};
use futures::StreamExt;
use serde_json::{Value, json};

pub(super) async fn execute(
    client: &reqwest::Client,
    config: &ModelConfig,
    operation: &WebRequest,
) -> Result<Value, ChatError> {
    let bearer = config
        .api_key
        .as_deref()
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| {
            ChatError::BadRequest("Codex web tools require an OAuth account".to_string())
        })?;
    let prompt = match operation {
        WebRequest::Search { query, .. } => {
            format!("Search the web for this query and cite the sources: {query}")
        }
        WebRequest::Fetch { url, format } => format!(
            "Open this exact URL using the web tool and return the readable page content in {format} format with citations: {url}"
        ),
    };
    let options = LlmStreamOptions {
        max_tokens: Some(4096),
        ..Default::default()
    };
    let mut body = request::build_request_body(
        config,
        &[Message::new("user", prompt)],
        &options,
        Some(&[json!({"type":"web_search"})]),
    );
    body["tool_choice"] = json!({"type":"web_search"});
    body["include"] = json!([
        "reasoning.encrypted_content",
        "web_search_call.action.sources"
    ]);
    let endpoint = format!("{}/responses", config.base_url.trim_end_matches('/'));
    let response = request::send(
        client,
        &endpoint,
        bearer,
        config.chatgpt_account_id.as_deref(),
        &config.custom_headers,
        &body,
        &options,
    )
    .await?;
    if !response.status().is_success() {
        return Err(request::failure(response).await);
    }
    let response = read_response(response).await?;
    normalize(&response, operation)
}

async fn read_response(response: reqwest::Response) -> Result<Value, ChatError> {
    let mut bytes = Vec::new();
    let mut output = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(
            &chunk.map_err(|_| ChatError::Provider("Codex web stream interrupted".to_string()))?,
        );
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(ChatError::Provider(
                "Codex web response exceeded size limit".to_string(),
            ));
        }
        // Parse only complete lines; keeping bytes intact preserves split UTF-8.
        while let Some(end) = bytes.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = bytes.drain(..=end).collect();
            if let Some(response) = response_event(&line, &mut output)? {
                return Ok(response);
            }
        }
    }
    if let Some(response) = response_event(&bytes, &mut output)? {
        return Ok(response);
    }
    Err(ChatError::Provider(
        "Codex web stream ended without a completed response".to_string(),
    ))
}

fn response_event(line: &[u8], output: &mut Vec<Value>) -> Result<Option<Value>, ChatError> {
    let line = std::str::from_utf8(line)
        .map_err(|_| ChatError::Provider("invalid Codex web stream".to_string()))?;
    let Some(data) = line.trim().strip_prefix("data:") else {
        return Ok(None);
    };
    let Ok(event) = serde_json::from_str::<Value>(data.trim()) else {
        return Ok(None);
    };
    match event["type"].as_str() {
        Some("response.output_item.done") => {
            if event["item"].is_object() {
                merge_item(output, event["item"].clone());
            }
            Ok(None)
        }
        Some("response.completed") if event["response"]["status"] == "completed" => {
            let mut response = event["response"].clone();
            // Codex can deliver output only in item events, leaving the final
            // response's output empty or limited to a subset of those items.
            for item in response["output"].as_array().into_iter().flatten() {
                merge_item(output, item.clone());
            }
            response["output"] = json!(output);
            Ok(Some(response))
        }
        Some("response.failed" | "response.incomplete" | "error") => Err(ChatError::Provider(
            "Codex hosted web request failed".to_string(),
        )),
        _ => Ok(None),
    }
}

fn merge_item(output: &mut Vec<Value>, item: Value) {
    if let Some(existing) = output.iter_mut().find(|existing| {
        item["id"].as_str().is_some_and(|id| existing["id"] == id) || **existing == item
    }) {
        if let (Some(existing), Some(item)) = (existing.as_object_mut(), item.as_object()) {
            existing.extend(item.iter().map(|(key, value)| (key.clone(), value.clone())));
        }
    } else {
        output.push(item);
    }
}

fn normalize(response: &Value, operation: &WebRequest) -> Result<Value, ChatError> {
    let output = response["output"]
        .as_array()
        .ok_or_else(|| ChatError::Provider("Codex web response has no output".to_string()))?;
    let calls: Vec<_> = output
        .iter()
        .filter(|item| item["type"] == "web_search_call" && item["status"] == "completed")
        .collect();
    if calls.is_empty() {
        let output_types: Vec<_> = output
            .iter()
            .filter_map(|item| item["type"].as_str())
            .collect();
        let web_statuses: Vec<_> = output
            .iter()
            .filter(|item| item["type"] == "web_search_call")
            .filter_map(|item| item["status"].as_str())
            .collect();
        tracing::warn!(
            ?output_types,
            ?web_statuses,
            "Codex returned no completed hosted web call"
        );
        return Err(ChatError::Provider(
            "Codex did not execute a web tool".to_string(),
        ));
    }
    let mut answer = String::new();
    let mut sources = Vec::new();
    for item in output {
        for source in item["action"]["sources"].as_array().into_iter().flatten() {
            add_source(&mut sources, source);
        }
        for part in item["content"].as_array().into_iter().flatten() {
            if let Some(text) = part["text"].as_str() {
                answer.push_str(text);
            }
            for citation in part["annotations"].as_array().into_iter().flatten() {
                if citation["type"] == "url_citation" {
                    add_source(&mut sources, citation);
                }
            }
        }
    }
    match operation {
        WebRequest::Search { query, max_results } => {
            sources.truncate(*max_results);
            Ok(json!({"query":query,"results":sources,"answer":answer,"provider":"openai-codex"}))
        }
        WebRequest::Fetch { url, format } => {
            if !calls
                .iter()
                .any(|call| call["action"]["type"] == "open_page" && call["action"]["url"] == *url)
            {
                return Err(ChatError::Provider(
                    "Codex did not open the requested page".to_string(),
                ));
            }
            Ok(
                json!({"url":url,"content":{"format":format,"text":answer,"length":answer.chars().count()},
                "sources":sources,"provider":"openai-codex","mode":"hosted_web_tool"}),
            )
        }
    }
}

fn add_source(sources: &mut Vec<Value>, source: &Value) {
    let Some(url) = source["url"].as_str() else {
        return;
    };
    if !sources.iter().any(|item| item["url"] == url) {
        sources.push(json!({"url":url,"title":source["title"].as_str().unwrap_or(url)}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_without_a_web_call_is_not_a_search_result() {
        let response =
            json!({"output":[{"type":"message", "content":[{"text":"I searched the web"}]}]});
        let result = normalize(
            &response,
            &WebRequest::Search {
                query: "test".to_string(),
                max_results: 5,
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn a_search_call_does_not_count_as_opening_the_requested_page() {
        let response = json!({"output":[{"type":"web_search_call","status":"completed","action":{"type":"search"}}]});
        let result = normalize(
            &response,
            &WebRequest::Fetch {
                url: "https://example.com".to_string(),
                format: "markdown".to_string(),
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn failed_and_incomplete_events_cannot_return_success() {
        for kind in ["response.failed", "response.incomplete", "error"] {
            assert!(
                response_event(
                    format!("data: {}", json!({"type":kind})).as_bytes(),
                    &mut Vec::new()
                )
                .is_err()
            );
        }
    }

    #[test]
    fn item_events_survive_an_empty_terminal_output_and_are_not_duplicated() {
        let item = json!({"id":"ws_1", "type":"web_search_call", "status":"completed",
            "action":{"type":"search", "sources":[{"url":"https://example.com", "title":"Example"}]}});
        for terminal_output in [
            json!([]),
            json!([item.clone()]),
            json!([{"id":"ws_1", "type":"web_search_call", "status":"completed"}]),
        ] {
            let mut output = Vec::new();
            response_event(
                format!(
                    "data: {}",
                    json!({"type":"response.output_item.done", "item":item})
                )
                .as_bytes(),
                &mut output,
            )
            .unwrap();
            let response = response_event(
                format!(
                    "data: {}",
                    json!({"type":"response.completed", "response":{
                        "status":"completed", "output":terminal_output
                    }})
                )
                .as_bytes(),
                &mut output,
            )
            .unwrap()
            .unwrap();
            assert_eq!(response["output"].as_array().unwrap().len(), 1);
            let result = normalize(
                &response,
                &WebRequest::Search {
                    query: "test".to_string(),
                    max_results: 3,
                },
            )
            .unwrap();
            assert_eq!(result["results"][0]["url"], "https://example.com");
        }
    }
}
