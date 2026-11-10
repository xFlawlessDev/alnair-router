use crate::{
    error::{Error, Result},
    model::ResolvedTarget,
    state::AppState,
};

pub(super) async fn select(
    state: &AppState,
    model: Option<&str>,
) -> Result<Option<(ResolvedTarget, String, Option<String>)>> {
    let mut candidates = Vec::new();
    if let Some(model) = model {
        match super::resolve_target(state, Some(model)).await {
            Ok(target) => candidates.push(target),
            Err(error) if super::is_reference_error(&error) => return Err(error),
            Err(_) => {}
        }
    }
    let selected_id = candidates
        .first()
        .map(|target| target.connection_id.clone());
    candidates.extend(
        super::search_fallbacks(state)
            .await?
            .into_iter()
            .filter(|target| Some(&target.connection_id) != selected_id.as_ref()),
    );
    for target in candidates {
        let Some(connection) = state.connections().get(&target.connection_id).await? else {
            continue;
        };
        let preset = crate::providers::search_preset(connection.provider_id.as_deref());
        let path = connection.web_fetch_path.or_else(|| {
            preset
                .as_ref()
                .and_then(|preset| preset.default_web_fetch_path.map(str::to_string))
        });
        if connection.web_fetch_mode != "provider" && preset.is_none() {
            continue;
        }
        let path = path.ok_or_else(|| {
            Error::BadRequest(format!(
                "connection `{}` is set to fetch through its provider but has no web_fetch_path",
                target.connection_name
            ))
        })?;
        let shape = connection.web_fetch_shape.or_else(|| {
            preset
                .as_ref()
                .and_then(|preset| preset.default_web_fetch_shape.map(str::to_string))
        });
        return Ok(Some((target, path, shape)));
    }
    Ok(None)
}
