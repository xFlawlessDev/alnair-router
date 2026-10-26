//! Normalization of search responses into one provider-agnostic shape.
//!
//! OpenAI-compatible search upstreams disagree on both the request and the
//! response wire format: Tavily returns `results[].content`, Perplexity
//! `results[].snippet`, Brave `web.results[].description`, and SearXNG puts the
//! results in `results[]` with a different field set again. Proxying them
//! verbatim forces every client to learn each dialect.
//!
//! This module maps whatever the upstream returned onto a single
//! [`SearchResponse`]. Normalization is **best-effort and additive**: when the
//! body does not look like a search response it is returned untouched, so an
//! upstream the router does not know still works exactly as before.

use serde::Serialize;
use serde_json::Value;

/// One result, in the router's canonical shape.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    /// Relevance score, when the upstream reports one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// Publication or last-updated date, as the upstream spelled it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    /// Full page text, when the upstream returned it alongside the snippet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// The upstream that produced this result, when the router knows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

/// A normalized search response.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchResponse {
    pub object: String,
    pub results: Vec<SearchResult>,
    /// The query the upstream echoed back, when it did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// An LLM-generated answer, when the upstream produced one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
}

impl SearchResponse {
    fn new(query: Option<String>, answer: Option<String>, results: Vec<SearchResult>) -> Self {
        Self {
            object: "search".to_string(),
            results,
            query,
            answer,
        }
    }
}

/// Rewrites `body` into the canonical shape, or returns `None` when it does
/// not look like a search response.
///
/// A body counts as a search response when it is a JSON object carrying a
/// `results` array whose entries have a URL, or when it nests one under the
/// `web` key the way Brave does. Anything else — an error envelope, an
/// embedding payload, a shape from an upstream the router has never seen — is
/// left to the caller to forward verbatim.
pub fn normalize_search_response(body: &Value, provider: Option<&str>) -> Option<Value> {
    let results = extract_results(body)?;
    if results.is_empty() && !looks_like_search_envelope(body) {
        return None;
    }

    let normalized = SearchResponse::new(
        string_field(body, &["query", "q"]),
        string_field(body, &["answer", "summary"]),
        results
            .iter()
            .filter_map(|item| normalize_result(item, provider))
            .collect(),
    );

    serde_json::to_value(normalized).ok()
}

/// Finds the result array, following the `web` wrapper Brave uses.
fn extract_results(body: &Value) -> Option<&Vec<Value>> {
    if let Some(results) = body.get("results").and_then(Value::as_array) {
        return Some(results);
    }
    if let Some(results) = body
        .get("web")
        .and_then(|web| web.get("results"))
        .and_then(Value::as_array)
    {
        return Some(results);
    }
    None
}

/// True when the body is a search envelope with no hits, so an empty result
/// list is the honest answer rather than a sign the shape is unknown.
fn looks_like_search_envelope(body: &Value) -> bool {
    body.get("results").and_then(Value::as_array).is_some()
        || body.get("web").is_some()
        || body.get("query").is_some()
}

/// Maps one upstream result onto [`SearchResult`], or `None` when it has no URL.
fn normalize_result(item: &Value, provider: Option<&str>) -> Option<SearchResult> {
    let url = string_field(item, &["url", "link", "href"])?;

    Some(SearchResult {
        title: string_field(item, &["title", "name"]).unwrap_or_default(),
        url,
        snippet: string_field(
            item,
            &["snippet", "content", "description", "text", "raw_content"],
        )
        .unwrap_or_default(),
        score: item.get("score").and_then(Value::as_f64),
        published_at: string_field(
            item,
            &[
                "published_at",
                "published_date",
                "date",
                "page_age",
                "last_updated",
            ],
        ),
        content: string_field(item, &["content", "raw_content", "full_text"]),
        provider: provider.map(str::to_string),
    })
}

/// Reads the first present, non-empty string among `keys`.
fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tavily_results_are_mapped() {
        let body = json!({
            "query": "rust",
            "results": [{
                "title": "Rust",
                "url": "https://rust-lang.org",
                "content": "A language.",
                "score": 0.91,
                "published_date": "2024-01-01"
            }]
        });

        let normalized = normalize_search_response(&body, Some("tavily")).expect("normalized");
        assert_eq!(normalized["object"], "search");
        assert_eq!(normalized["query"], "rust");
        assert_eq!(normalized["results"][0]["url"], "https://rust-lang.org");
        assert_eq!(normalized["results"][0]["snippet"], "A language.");
        assert_eq!(normalized["results"][0]["score"], 0.91);
        assert_eq!(normalized["results"][0]["provider"], "tavily");
    }

    #[test]
    fn perplexity_snippets_and_brave_links_are_mapped() {
        let perplexity = json!({
            "results": [{ "title": "T", "url": "https://a", "snippet": "S", "date": "2024-02-02" }]
        });
        let normalized =
            normalize_search_response(&perplexity, Some("perplexity")).expect("perplexity");
        assert_eq!(normalized["results"][0]["snippet"], "S");
        assert_eq!(normalized["results"][0]["published_at"], "2024-02-02");

        let brave = json!({
            "web": { "results": [{ "title": "T", "url": "https://b", "description": "D" }] }
        });
        let normalized = normalize_search_response(&brave, Some("brave")).expect("brave");
        assert_eq!(normalized["results"][0]["url"], "https://b");
        assert_eq!(normalized["results"][0]["snippet"], "D");
    }

    #[test]
    fn a_result_without_a_url_is_dropped() {
        let body = json!({ "results": [{ "title": "no url" }, { "url": "https://ok" }] });
        let normalized = normalize_search_response(&body, None).expect("normalized");
        assert_eq!(normalized["results"].as_array().map(Vec::len), Some(1));
        assert!(normalized["results"][0].get("provider").is_none());
    }

    #[test]
    fn an_empty_envelope_is_a_search_response_with_no_hits() {
        let normalized = normalize_search_response(&json!({ "results": [] }), None)
            .expect("an empty results array is still a search response");
        assert_eq!(normalized["results"].as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn unrelated_bodies_are_left_alone() {
        for body in [
            json!({ "error": { "message": "nope" } }),
            json!({ "data": [{ "embedding": [0.1, 0.2] }] }),
            json!([1, 2, 3]),
            json!("plain text"),
        ] {
            assert!(
                normalize_search_response(&body, None).is_none(),
                "{body} should not be normalized"
            );
        }
    }
}
