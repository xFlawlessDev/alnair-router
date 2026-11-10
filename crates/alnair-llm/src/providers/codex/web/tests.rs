use super::*;

#[test]
fn citations_enrich_url_only_sources_without_erasing_metadata() {
    let mut sources = Vec::new();
    add_source(&mut sources, &json!({"url":"https://example.com"}));
    add_source(
        &mut sources,
        &json!({"url":"https://example.com", "title":"Example",
        "snippet":"Source summary", "published_at":"2026-10-01"}),
    );
    add_source(
        &mut sources,
        &json!({"url":"https://example.com", "title":"", "snippet":null}),
    );
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0]["title"], "Example");
    assert_eq!(sources[0]["snippet"], "Source summary");
    assert_eq!(sources[0]["published_at"], "2026-10-01");
}

#[test]
fn search_metadata_is_ranked_and_limited_to_tool_sources() {
    let response = json!({"output":[
        {"type":"web_search_call", "status":"completed", "action":{"type":"search", "sources":[
            {"url":"https://unused.example"}, {"url":"https://rust-lang.org/"},
            {"url":"https://blog.rust-lang.org/", "published_at":"2026-10-01"}
        ]}},
        {"type":"message", "content":[{"text":json!({"answer":"Rust is a programming language.", "results":[
            {"url":"https://unobserved.example", "title":"Invented", "snippet":"Not a tool source"},
            {"url":"https://rust-lang.org/", "title":"Rust Programming Language", "snippet":"Official Rust website.", "published_at":null},
            {"url":"https://rust-lang.org/", "title":"Duplicate"},
            {"url":"https://blog.rust-lang.org/", "title":"Rust Blog", "snippet":"Rust news.", "published_at":"2099-01-01"}
        ]}).to_string(), "annotations":[
            {"type":"url_citation", "url":"https://rust-lang.org/", "title":"Rust"}
        ]}]}
    ]});
    let result = normalize(
        &response,
        &WebRequest::Search {
            query: "Rust".to_string(),
            max_results: 5,
        },
    )
    .unwrap();
    assert_eq!(result["answer"], "Rust is a programming language.");
    assert_eq!(result["results"].as_array().unwrap().len(), 2);
    assert_eq!(result["results"][0]["url"], "https://rust-lang.org/");
    assert_eq!(result["results"][0]["title"], "Rust");
    assert_eq!(result["results"][0]["snippet"], "Official Rust website.");
    assert!(result["results"][0].get("published_at").is_none());
    assert_eq!(result["results"][1]["snippet"], "Rust news.");
    assert_eq!(result["results"][1]["published_at"], "2026-10-01");
}

#[test]
fn fenced_search_json_and_trailing_citations_are_accepted() {
    let response = json!({"answer":"An answer", "results":[
        {"url":"https://example.com", "title":"Example", "snippet":"A summary", "published_at":"2026-10-01"}
    ]});
    for text in [
        response.to_string(),
        format!("```json\n{response}\n```"),
        format!("{response}\nCitation"),
    ] {
        let mut sources = vec![json!({"url":"https://example.com", "title":"https://example.com"})];
        assert_eq!(enrich_search_results(&mut sources, &text), "An answer");
        assert_eq!(sources[0]["title"], "Example");
        assert_eq!(sources[0]["snippet"], "A summary");
        assert_eq!(sources[0]["published_at"], "2026-10-01");
    }
}

#[test]
fn invalid_search_json_preserves_prose_and_tool_sources() {
    for text in [
        "A normal cited answer",
        "{invalid",
        "{}",
        r#"{"answer":"Only an answer"}"#,
    ] {
        let original = vec![json!({"url":"https://example.com", "title":"Example"})];
        let mut sources = original.clone();
        assert_eq!(enrich_search_results(&mut sources, text), text);
        assert_eq!(sources, original);
    }
}

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
