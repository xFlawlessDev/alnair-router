use super::{
    AuthStyle, ProviderRegistry, WebRequest, WireApi, default_registry, model_config,
    provider_type_from_str,
};
use crate::{
    error::{Error, Result},
    model::ResolvedTarget,
};
use std::sync::LazyLock;

static REGISTRY: LazyLock<ProviderRegistry> = LazyLock::new(default_registry);

pub fn supports_hosted_web(provider_type: &str, fetch: bool) -> bool {
    provider_type_from_str(provider_type)
        .ok()
        .and_then(|kind| REGISTRY.get(&kind))
        .is_some_and(|provider| {
            let capabilities = provider.capabilities();
            if fetch {
                capabilities.web_fetch
            } else {
                capabilities.web_search
            }
        })
}

pub async fn hosted_web(
    target: &ResolvedTarget,
    token: Option<&str>,
    operation: &WebRequest,
) -> Result<serde_json::Value> {
    let kind = provider_type_from_str(&target.provider_type)?;
    let provider = REGISTRY
        .get(&kind)
        .ok_or_else(|| Error::UnsupportedProviderType(target.provider_type.clone()))?;
    let config = model_config(
        kind,
        &target.base_url,
        &target.model,
        token,
        AuthStyle::Bearer,
        WireApi::Responses,
        target.custom_headers.clone(),
        None,
        false,
    );
    provider
        .web_request(&config, operation)
        .await
        .map_err(|error| Error::Upstream(error.to_string()))
}
