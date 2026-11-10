//! The built-in model catalog for providers that publish no `/models`.
//!
//! The ChatGPT/Codex backend has no listing endpoint, so a fresh connection
//! would show no models at all and the operator would have to guess an id. The
//! list below is compiled in from the backend's own catalog, which is why it
//! carries a notice everywhere it surfaces: it is a snapshot, not a live
//! answer. A model OpenAI adds after this build is reachable by typing its id —
//! it just is not offered.
//!
//! Ranked by an explicit `priority` rather than by name, because the ids
//! (`gpt-6.1-sol`, `gpt-5.5`, …) do not sort in the backend's own order.

/// One catalog model as the probe reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogModel {
    pub id: &'static str,
    pub display_name: &'static str,
    /// Position in the provider's own listing; lower sorts first.
    pub priority: u8,
    pub context_length: u32,
    /// True when the model accepts image input.
    pub vision: bool,
    /// The reasoning efforts it supports; empty when it has none.
    pub efforts: &'static [&'static str],
}

/// The notice shown whenever these models are listed.
///
/// Kept as one constant so the API, the README and the dashboard cannot drift
/// apart on the wording.
pub const CODEX_CATALOG_NOTICE: &str = "This list is built into the router, because the ChatGPT/Codex backend \
     publishes no model list. This is a snapshot: a newer model still works if \
     you type its id, and one that has been retired may still be listed here.";

/// Model choices in the order supplied for the Codex connection.
const CODEX_MODELS: &[CatalogModel] = &[
    CatalogModel {
        id: "gpt-6.1-sol",
        display_name: "GPT-6.1-Sol",
        priority: 0,
        context_length: 500_000,
        vision: true,
        efforts: &["low", "medium", "high", "xhigh"],
    },
    CatalogModel {
        id: "gpt-6-astra",
        display_name: "GPT-6-Astra",
        priority: 1,
        context_length: 272_000,
        vision: true,
        efforts: &["low", "medium", "high", "xhigh"],
    },
    CatalogModel {
        id: "gpt-6-sol",
        display_name: "GPT-6-Sol",
        priority: 2,
        context_length: 272_000,
        vision: true,
        efforts: &["low", "medium", "high", "xhigh"],
    },
    CatalogModel {
        id: "gpt-6-luna",
        display_name: "GPT-6-Luna",
        priority: 3,
        context_length: 272_000,
        vision: true,
        efforts: &["low", "medium", "high", "xhigh"],
    },
    CatalogModel {
        id: "gpt-5.6-sol",
        display_name: "GPT-5.6-Sol",
        priority: 4,
        context_length: 272_000,
        vision: true,
        efforts: &["low", "medium", "high", "xhigh"],
    },
    CatalogModel {
        id: "gpt-6.1-terra",
        display_name: "GPT-6.1 Terra",
        priority: 5,
        context_length: 500_000,
        vision: true,
        efforts: &["low", "medium", "high", "xhigh"],
    },
    CatalogModel {
        id: "gpt-5.6-terra",
        display_name: "GPT-5.6-Terra",
        priority: 6,
        context_length: 272_000,
        vision: true,
        efforts: &["low", "medium", "high", "xhigh"],
    },
    CatalogModel {
        id: "gpt-5.6-luna",
        display_name: "GPT-5.6-Luna",
        priority: 7,
        context_length: 272_000,
        vision: true,
        efforts: &["low", "medium", "high", "xhigh"],
    },
];

/// The built-in catalog for a provider type, in the provider's own order.
///
/// `None` means the provider has no compiled-in list and is expected to answer
/// `/models` itself.
pub fn models_for(provider_type: &str) -> Option<&'static [CatalogModel]> {
    match provider_type {
        "openai-codex" => Some(CODEX_MODELS),
        _ => None,
    }
}

/// The notice for a provider's built-in catalog, when it has one.
pub fn notice_for(provider_type: &str) -> Option<&'static str> {
    models_for(provider_type).map(|_| CODEX_CATALOG_NOTICE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_codex_catalog_is_ordered_and_unique() {
        let models = models_for("openai-codex").expect("codex catalog");

        assert_eq!(
            models.iter().map(|model| model.id).collect::<Vec<_>>(),
            [
                "gpt-6.1-sol",
                "gpt-6-astra",
                "gpt-6-sol",
                "gpt-6-luna",
                "gpt-5.6-sol",
                "gpt-6.1-terra",
                "gpt-5.6-terra",
                "gpt-5.6-luna"
            ]
        );

        let mut ids: Vec<&str> = models.iter().map(|model| model.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), models.len(), "a model id repeated");

        let priorities: Vec<u8> = models.iter().map(|model| model.priority).collect();
        assert!(
            priorities.windows(2).all(|pair| pair[0] < pair[1]),
            "the catalog must already be in priority order"
        );
    }

    #[test]
    fn every_codex_model_declares_its_shape() {
        for model in models_for("openai-codex").expect("codex catalog") {
            assert!(model.context_length > 0, "{}", model.id);
            assert!(!model.display_name.is_empty(), "{}", model.id);
            assert!(
                !model.efforts.is_empty(),
                "{} has no reasoning dial",
                model.id
            );
        }
    }

    #[test]
    fn only_codex_ships_a_built_in_catalog() {
        assert!(models_for("openai-codex").is_some());
        assert!(notice_for("openai-codex").is_some_and(|note| note.contains("built into")));

        for other in ["openai-compatible", "anthropic-native", "codebuddy-intl"] {
            assert!(models_for(other).is_none(), "{other}");
            assert!(notice_for(other).is_none(), "{other}");
        }
    }

    #[test]
    fn the_notice_explains_that_a_newer_model_still_works() {
        // The wording is the mitigation for the snapshot: without it, an
        // operator reads a missing model as "unsupported".
        assert!(CODEX_CATALOG_NOTICE.contains("type its id"));
        assert!(CODEX_CATALOG_NOTICE.contains("retired"));
    }
}
