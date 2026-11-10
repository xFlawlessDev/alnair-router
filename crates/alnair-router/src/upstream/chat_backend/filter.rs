//! Capability filtering over a resolved tier list.
//!
//! The filter runs *after* resolution and *before* execution. Two reasons that
//! order is not negotiable:
//!
//! 1. The handler does not know the target at parse time, so it cannot decide
//!    whether `response_format` is servable.
//! 2. Combos. Tier 1 may be `anthropic-native` and tier 2 `openai-compatible`;
//!    rejecting at parse time would fail a request that would have succeeded.
//!
//! So: drop the tiers that cannot honour the request, run the survivors as
//! usual, and reject only when none survive.

use crate::error::{Error, Result};
use crate::model::ResolvedTarget;

use super::capability::{
    effective_wire_api, translation_support, unsupported_features as features_for,
};
use super::{GenerationOptions, provider_type_from_str, wire_api_from_str};

/// Drops every target whose protocol cannot honour `options`.
///
/// Returns the surviving targets in their original order, so fallback behaviour
/// is unchanged for every request that sets nothing unusual.
///
/// An error is returned only when no tier survives. That error names the
/// parameter, the value the client asked for, and the protocols that refused,
/// because a bare "unsupported" costs the caller the same debugging round trip
/// as silence would.
pub(crate) fn filter_targets(
    targets: Vec<ResolvedTarget>,
    options: &GenerationOptions,
) -> Result<Vec<ResolvedTarget>> {
    // An empty option set cannot be refused by anyone, so skip the matrix work
    // entirely: this is the overwhelmingly common path.
    if is_empty(options) {
        return Ok(targets);
    }

    let mut survivors = Vec::with_capacity(targets.len());
    let mut refusals: Vec<(String, String)> = Vec::new();

    for target in targets {
        // A malformed provider type is a configuration fault the executor
        // reports loudly; the filter must not hide it by dropping the tier.
        let Ok(provider_type) = provider_type_from_str(&target.provider_type) else {
            return Ok(vec![target]);
        };
        let wire_api = effective_wire_api(&provider_type, wire_api_from_str(&target.wire_api));
        let support = translation_support(&provider_type, wire_api);

        let missing = features_for(options, support);
        if missing.is_empty() {
            survivors.push(target);
        } else {
            refusals.push((target.provider_type.clone(), missing[0].param.to_string()));
        }
    }

    if !survivors.is_empty() {
        return Ok(survivors);
    }

    // Nothing can serve it. Report the first parameter the client set, with
    // every refusing protocol named, so the message is actionable rather than
    // a bare refusal.
    refuse(options, &refusals)
}

/// True when the request sets nothing the matrix looks at.
fn is_empty(options: &GenerationOptions) -> bool {
    options.tool_choice.is_none()
        && options.parallel_tool_calls.is_none()
        && options.response_format.is_none()
        && options.top_k.is_none()
        && options.logprobs.is_none()
        && options.seed.is_none()
        && options.metadata.is_none()
        && options.service_tier.is_none()
        && options.store.is_none()
        && options.prompt_cache_key.is_none()
        && options.verbosity.is_none()
}

/// Builds the `400` for a request no surviving tier could serve.
fn refuse(
    options: &GenerationOptions,
    refusals: &[(String, String)],
) -> Result<Vec<ResolvedTarget>> {
    // The first parameter the client set is the one worth naming: it is the one
    // they are most likely to remove first.
    let param = refusals
        .first()
        .map(|(_, param)| param.clone())
        .unwrap_or_else(|| "generation options".to_string());

    let protocols: Vec<String> = {
        let mut seen: Vec<String> = refusals
            .iter()
            .map(|(protocol, _)| protocol.clone())
            .collect();
        seen.sort_unstable();
        seen.dedup();
        seen
    };

    Err(Error::UnsupportedParameter {
        param: param.clone(),
        message: format!(
            "{} is not supported by the upstream protocol(s) available for this request ({}). \
             Remove `{param}`, or route the request to a model whose connection supports it.",
            requested_value(options, &param),
            protocols.join(", "),
        ),
    })
}

/// Describes the value the client asked for, for the error message.
fn requested_value(options: &GenerationOptions, param: &str) -> String {
    match param {
        "tool_choice" => match &options.tool_choice {
            Some(super::ToolChoice::Named { name }) => {
                format!("`tool_choice` for function `{name}`")
            }
            _ => "`tool_choice`".to_string(),
        },
        "response_format" => match &options.response_format {
            Some(super::ResponseFormat::JsonSchema { name, .. }) => {
                format!("`response_format` with a JSON schema (`{name}`)")
            }
            Some(super::ResponseFormat::JsonObject) => {
                "`response_format` with a JSON object".to_string()
            }
            _ => "`response_format`".to_string(),
        },
        other => format!("`{other}`"),
    }
}

#[cfg(test)]
mod tests;
