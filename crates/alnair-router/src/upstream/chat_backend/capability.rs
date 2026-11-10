//! Which translation features each upstream protocol can honour.
//!
//! Capability is a property of the *wire API*, not of the vendor behind the
//! URL: whether `/responses` supports `parallel_tool_calls` is decided by the
//! protocol, not by which company's endpoint is answering. That keeps the
//! matrix small and, more importantly, keeps it honest — a row is only ✅
//! when a test proves a body carries the field.
//!
//! The matrix is consulted by the executor *after* resolution, so a combo can
//! fall through to a tier that can honour the request instead of failing the
//! whole call (see [`super::filter`]). It is never consulted by a handler: at
//! parse time the target is unknown.

use super::{ProviderType, WireApi};

use super::{GenerationOptions, ResponseFormat, ToolChoice};

/// What one upstream protocol can express.
///
/// A `false` here does not mean "reject every request that sets the field" —
/// the executor only rejects a request when *every* candidate tier has
/// `false` for a field the client actually set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranslationSupport {
    pub tool_choice_required: bool,
    pub tool_choice_named: bool,
    pub parallel_tool_calls: bool,
    pub response_format_json_object: bool,
    pub response_format_json_schema: bool,
    pub top_k: bool,
    pub logprobs: bool,
    pub seed: bool,
    pub metadata: bool,
    pub service_tier: bool,
    pub store: bool,
    pub prompt_cache_key: bool,
    pub verbosity: bool,
    /// Whether structured citations survive the round trip to the client.
    ///
    /// This is a property of the **client-facing renderer**, not of whether
    /// the upstream can talk about sources. An Anthropic-shaped client has no
    /// citation block type, so a citation there is flattened into inline text
    /// and the client receives a string, not a structured annotation. Only the
    /// OpenAI-shaped endpoints carry `annotations` as data, so only those can
    /// truthfully answer this `true`.
    ///
    /// Deliberately named for the client-facing artefact rather than the
    /// feature, because "citations" alone reads like an upstream capability and
    /// that is the reading which produced a wrong row here once already.
    pub citation_annotations: bool,
}

/// Every capability on. Used as the base for a protocol that supports the
/// whole union, so each row below only has to name its gaps.
const ALL_SUPPORTED: TranslationSupport = TranslationSupport {
    tool_choice_required: true,
    tool_choice_named: true,
    parallel_tool_calls: true,
    response_format_json_object: true,
    response_format_json_schema: true,
    top_k: true,
    logprobs: true,
    seed: true,
    metadata: true,
    service_tier: true,
    store: true,
    prompt_cache_key: true,
    verbosity: true,
    citation_annotations: true,
};

/// Nothing supported. Only the tests use it, to prove the predicate reports
/// every field a client can set.
#[cfg(test)]
const NOTHING_SUPPORTED: TranslationSupport = TranslationSupport {
    tool_choice_required: false,
    tool_choice_named: false,
    parallel_tool_calls: false,
    response_format_json_object: false,
    response_format_json_schema: false,
    top_k: false,
    logprobs: false,
    seed: false,
    metadata: false,
    service_tier: false,
    store: false,
    prompt_cache_key: false,
    verbosity: false,
    citation_annotations: false,
};

/// The capability set for one target's protocol.
///
/// Only the OpenAI-compatible family has two wire APIs in play; every other
/// provider speaks exactly one dialect, so the `wire_api` argument is ignored
/// for them (see [`effective_wire_api`]).
pub fn translation_support(provider_type: &ProviderType, wire_api: WireApi) -> TranslationSupport {
    match provider_type {
        // The chat-completions endpoint. `top_k` is vendor-specific rather than
        // part of the API; it is forwarded optimistically and the provider's
        // existing strip-and-retry path handles an upstream that refuses it.
        ProviderType::OpenaiCompatible => match wire_api {
            WireApi::Chat => TranslationSupport {
                store: false,
                ..ALL_SUPPORTED
            },
            // `seed` and `top_k` are not part of the Responses API, and the
            // endpoint rejects fields it does not define.
            WireApi::Responses => TranslationSupport {
                seed: false,
                top_k: false,
                ..ALL_SUPPORTED
            },
        },
        // The Messages API. Structured output is only reachable through
        // `output_config.format`, which not every Anthropic-compatible endpoint
        // implements, and the remaining passthrough fields have no equivalent.
        // Written out in full rather than as a spread, because most of these
        // are `false` and a spread would hide that.
        ProviderType::AnthropicNative => TranslationSupport {
            tool_choice_required: true,
            tool_choice_named: true,
            parallel_tool_calls: true,
            response_format_json_object: false,
            response_format_json_schema: false,
            top_k: true,
            logprobs: false,
            seed: false,
            metadata: true,
            service_tier: false,
            store: false,
            prompt_cache_key: false,
            verbosity: false,
            // A Messages client has no citation block type, so the renderer
            // flattens a citation into inline text. The upstream may still
            // report sources; the client just cannot receive them as data.
            citation_annotations: false,
        },
        // Codex speaks the Responses dialect, with two documented differences:
        // `store` is forced `false` by the backend contract, and it adds
        // `xhigh` rather than `max`.
        ProviderType::OpenAiCodex => TranslationSupport {
            seed: false,
            top_k: false,
            store: false,
            ..ALL_SUPPORTED
        },
        // Command Code and CodeBuddy Intl both ride the OpenAI chat body.
        ProviderType::CommandCode | ProviderType::CodeBuddyIntl => TranslationSupport {
            store: false,
            ..ALL_SUPPORTED
        },
    }
}

/// The wire API a provider actually speaks, for diagnostic messages and for
/// the matrix lookup.
pub(crate) fn effective_wire_api(provider_type: &ProviderType, wire_api: WireApi) -> WireApi {
    match provider_type {
        ProviderType::OpenaiCompatible => wire_api,
        ProviderType::AnthropicNative => WireApi::Chat,
        ProviderType::OpenAiCodex => WireApi::Responses,
        ProviderType::CommandCode | ProviderType::CodeBuddyIntl => WireApi::Chat,
    }
}

/// One option the client set that a target cannot honour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedFeature {
    /// The parameter name as the client spelled it, for the error body.
    pub param: &'static str,
    /// The value the client asked for, for the error body.
    pub requested: String,
}

/// Every option `options` sets that `support` cannot honour.
///
/// Pure, so it is trivially testable and cannot itself fail over incorrectly.
/// The empty vector means "this target can serve the request unmodified".
pub fn unsupported_features(
    options: &GenerationOptions,
    support: TranslationSupport,
) -> Vec<UnsupportedFeature> {
    let mut missing = Vec::new();

    match &options.tool_choice {
        Some(ToolChoice::Required) if !support.tool_choice_required => {
            missing.push(UnsupportedFeature {
                param: "tool_choice",
                requested: "required".to_string(),
            })
        }
        Some(ToolChoice::Named { name }) if !support.tool_choice_named => {
            missing.push(UnsupportedFeature {
                param: "tool_choice",
                requested: format!("function `{name}`"),
            });
        }
        _ => {}
    }

    if options.parallel_tool_calls.is_some() && !support.parallel_tool_calls {
        missing.push(UnsupportedFeature {
            param: "parallel_tool_calls",
            requested: options
                .parallel_tool_calls
                .map(|v| v.to_string())
                .unwrap_or_default(),
        });
    }

    match &options.response_format {
        Some(ResponseFormat::JsonObject) if !support.response_format_json_object => {
            missing.push(UnsupportedFeature {
                param: "response_format",
                requested: "json_object".to_string(),
            })
        }
        Some(ResponseFormat::JsonSchema { name, .. }) if !support.response_format_json_schema => {
            missing.push(UnsupportedFeature {
                param: "response_format",
                requested: format!("json_schema `{name}`"),
            })
        }
        _ => {}
    }

    // `top_k` is forwarded optimistically on chat by design, so the matrix's
    // `top_k: false` rows are the only ones that reject it.
    if options.top_k.is_some() && !support.top_k {
        missing.push(UnsupportedFeature {
            param: "top_k",
            requested: options.top_k.map(|v| v.to_string()).unwrap_or_default(),
        });
    }

    if options.logprobs == Some(true) && !support.logprobs {
        missing.push(UnsupportedFeature {
            param: "logprobs",
            requested: "true".to_string(),
        });
    }

    if options.seed.is_some() && !support.seed {
        missing.push(UnsupportedFeature {
            param: "seed",
            requested: options.seed.map(|v| v.to_string()).unwrap_or_default(),
        });
    }

    if options.metadata.is_some() && !support.metadata {
        missing.push(UnsupportedFeature {
            param: "metadata",
            requested: "<object>".to_string(),
        });
    }

    if options.service_tier.is_some() && !support.service_tier {
        missing.push(UnsupportedFeature {
            param: "service_tier",
            requested: options.service_tier.clone().unwrap_or_default(),
        });
    }

    if options.store.is_some() && !support.store {
        missing.push(UnsupportedFeature {
            param: "store",
            requested: options.store.map(|v| v.to_string()).unwrap_or_default(),
        });
    }

    if options.prompt_cache_key.is_some() && !support.prompt_cache_key {
        missing.push(UnsupportedFeature {
            param: "prompt_cache_key",
            requested: "<key>".to_string(),
        });
    }

    if options.verbosity.is_some() && !support.verbosity {
        missing.push(UnsupportedFeature {
            param: "verbosity",
            requested: options.verbosity.clone().unwrap_or_default(),
        });
    }

    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> GenerationOptions {
        GenerationOptions::default()
    }

    #[test]
    fn a_request_that_sets_nothing_is_supported_everywhere() {
        for provider_type in ProviderType::ALL {
            for wire_api in [WireApi::Chat, WireApi::Responses] {
                let support = translation_support(&provider_type, wire_api);
                assert!(
                    unsupported_features(&options(), support).is_empty(),
                    "{provider_type:?}/{wire_api:?} must accept a bare request"
                );
            }
        }
    }

    #[test]
    fn the_chat_wire_api_has_no_store() {
        let support = translation_support(&ProviderType::OpenaiCompatible, WireApi::Chat);
        assert!(!support.store);
        assert!(support.seed, "chat completions defines `seed`");
    }

    #[test]
    fn the_responses_wire_api_has_no_seed_or_top_k() {
        let support = translation_support(&ProviderType::OpenaiCompatible, WireApi::Responses);
        assert!(!support.seed);
        assert!(!support.top_k);
        assert!(support.parallel_tool_calls);
        assert!(support.response_format_json_schema);
    }

    #[test]
    fn anthropic_rejects_structured_output_and_bare_seed() {
        let support = translation_support(&ProviderType::AnthropicNative, WireApi::Chat);
        assert!(!support.response_format_json_object);
        assert!(!support.response_format_json_schema);
        assert!(!support.seed);
        assert!(support.top_k, "Anthropic defines top_k natively");
        assert!(support.tool_choice_required);
    }

    #[test]
    fn codex_cannot_store_a_turn() {
        let support = translation_support(&ProviderType::OpenAiCodex, WireApi::Chat);
        assert!(!support.store);
        // Codex speaks the Responses event sequence, so it can emit structured
        // citation annotations as data.
        assert!(support.citation_annotations);
    }

    #[test]
    fn only_the_openai_shaped_clients_receive_citations_as_data() {
        // The row that was wrong before: an Anthropic-shaped client gets a
        // citation flattened into inline text, not a structured annotation.
        for (provider_type, supports) in [
            (ProviderType::OpenaiCompatible, true),
            (ProviderType::OpenAiCodex, true),
            (ProviderType::CommandCode, true),
            (ProviderType::CodeBuddyIntl, true),
            (ProviderType::AnthropicNative, false),
        ] {
            let support = translation_support(&provider_type, WireApi::Chat);
            assert_eq!(
                support.citation_annotations, supports,
                "{provider_type:?} must report citation_annotations: {supports}"
            );
        }
    }

    #[test]
    fn citations_are_output_side_and_never_reject_a_request() {
        // `citation_annotations` is not something a client can ask for, so it
        // must never contribute to a 400 — a target that cannot render a
        // citation still serves the turn, just with flattened text.
        let options = GenerationOptions {
            include: vec!["citations".to_string()],
            ..Default::default()
        };

        for provider_type in ProviderType::ALL {
            let support = translation_support(&provider_type, WireApi::Chat);
            assert!(
                unsupported_features(&options, support).is_empty(),
                "{provider_type:?} must not refuse a turn over citations"
            );
        }
    }

    #[test]
    fn a_json_schema_request_names_the_parameter_and_the_value() {
        let support = translation_support(&ProviderType::AnthropicNative, WireApi::Chat);
        let options = GenerationOptions {
            response_format: Some(ResponseFormat::JsonSchema {
                name: "ticket".to_string(),
                schema: serde_json::json!({ "type": "object" }),
                strict: true,
            }),
            ..Default::default()
        };

        let missing = unsupported_features(&options, support);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].param, "response_format");
        assert!(missing[0].requested.contains("ticket"), "{missing:?}");
    }

    #[test]
    fn a_named_tool_choice_is_reported_with_the_function_name() {
        let support = TranslationSupport {
            tool_choice_named: false,
            ..ALL_SUPPORTED
        };
        let options = GenerationOptions {
            tool_choice: Some(ToolChoice::Named {
                name: "search".to_string(),
            }),
            ..Default::default()
        };

        let missing = unsupported_features(&options, support);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].param, "tool_choice");
        assert!(missing[0].requested.contains("search"), "{missing:?}");
    }

    #[test]
    fn auto_and_none_are_never_reported() {
        // Every protocol can express "let the model decide" and "do not call
        // tools", so neither may be turned into a rejection.
        for choice in [ToolChoice::Auto, ToolChoice::None] {
            let options = GenerationOptions {
                tool_choice: Some(choice),
                ..Default::default()
            };
            assert!(
                unsupported_features(&options, NOTHING_SUPPORTED).is_empty(),
                "auto/none must never be a capability failure"
            );
        }
    }

    #[test]
    fn every_field_is_reported_when_nothing_is_supported() {
        let options = GenerationOptions {
            tool_choice: Some(ToolChoice::Required),
            parallel_tool_calls: Some(true),
            response_format: Some(ResponseFormat::JsonObject),
            top_k: Some(40),
            logprobs: Some(true),
            seed: Some(7),
            metadata: Some(serde_json::json!({ "a": 1 })),
            service_tier: Some("flex".to_string()),
            store: Some(true),
            prompt_cache_key: Some("k".to_string()),
            verbosity: Some("low".to_string()),
            ..Default::default()
        };

        let missing = unsupported_features(&options, NOTHING_SUPPORTED);
        let params: Vec<&str> = missing.iter().map(|feature| feature.param).collect();
        assert_eq!(
            params,
            vec![
                "tool_choice",
                "parallel_tool_calls",
                "response_format",
                "top_k",
                "logprobs",
                "seed",
                "metadata",
                "service_tier",
                "store",
                "prompt_cache_key",
                "verbosity",
            ]
        );
    }

    #[test]
    fn codex_and_responses_share_the_responses_dialect() {
        assert_eq!(
            effective_wire_api(&ProviderType::OpenAiCodex, WireApi::Chat),
            WireApi::Responses
        );
        assert_eq!(
            effective_wire_api(&ProviderType::OpenaiCompatible, WireApi::Responses),
            WireApi::Responses
        );
    }
}
