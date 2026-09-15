//! `connection_accounts` CRUD: extra API keys behind one connection.
//!
//! The executor rotates across a connection's primary key and these accounts,
//! so a provider endpoint can back several keys / quota buckets. Every write
//! invalidates the routing catalog.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use crate::db::repos::connection_accounts::{
    ConnectionAccount, CreateConnectionAccount, UpdateConnectionAccount,
};
use crate::error::{Error, Result};
use crate::state::AppState;

/// `GET /api/connections/{id}/accounts`
pub async fn list_accounts(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
) -> Result<Json<Vec<ConnectionAccount>>> {
    Ok(Json(
        state.connection_accounts().list(&connection_id).await?,
    ))
}

/// `POST /api/connections/{id}/accounts`
pub async fn create_account(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(input): Json<CreateConnectionAccount>,
) -> Result<impl IntoResponse> {
    let account = state
        .connection_accounts()
        .create(&connection_id, input)
        .await?;
    state.invalidate_catalog().await;
    Ok((StatusCode::CREATED, Json(account)))
}

/// `PATCH /api/connections/{id}/accounts/{account_id}`
pub async fn update_account(
    State(state): State<AppState>,
    Path((connection_id, account_id)): Path<(String, String)>,
    Json(input): Json<UpdateConnectionAccount>,
) -> Result<impl IntoResponse> {
    let account = state
        .connection_accounts()
        .update(&connection_id, &account_id, input)
        .await?;
    state.invalidate_catalog().await;
    Ok(Json(account))
}

/// `DELETE /api/connections/{id}/accounts/{account_id}`
pub async fn delete_account(
    State(state): State<AppState>,
    Path((connection_id, account_id)): Path<(String, String)>,
) -> Result<impl IntoResponse> {
    if !state
        .connection_accounts()
        .delete(&connection_id, &account_id)
        .await?
    {
        return Err(Error::NotFound(format!("account '{account_id}' not found")));
    }
    state.invalidate_catalog().await;
    Ok(StatusCode::NO_CONTENT)
}
