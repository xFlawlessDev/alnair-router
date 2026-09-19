//! In-flight login sessions: PKCE authorizations and device-code polls.
//!
//! A login starts as an admin call and finishes out of band — in a browser
//! redirect for PKCE, or in a background poll for a device code. This registry
//! is the bridge: the dialog polls a status by `login_id`, and the callback
//! looks a session up by its `state` nonce.
//!
//! Sessions are held in memory only. An interrupted login costs a retry, never
//! a stale credential on disk.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::Mutex;

use crate::oauth::credential::EndpointConfig;

/// How long a started login stays valid before it is pruned.
const LOGIN_TTL: Duration = Duration::from_secs(15 * 60);

/// Where a login has got to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LoginState {
    /// Waiting for the user to authorize in the browser.
    Pending,
    /// The credential was stored as this account.
    Completed { account_id: String },
    /// The login cannot complete; the reason is shown to the operator.
    Failed { error: String },
}

impl LoginState {
    pub fn as_str(&self) -> &'static str {
        match self {
            LoginState::Pending => "pending",
            LoginState::Completed { .. } => "completed",
            LoginState::Failed { .. } => "failed",
        }
    }
}

/// A device-code challenge the operator has to complete in a browser.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceChallenge {
    pub user_code: String,
    pub verification_uri: Option<String>,
    pub expires_in: Option<i64>,
}

/// One in-flight login.
#[derive(Debug, Clone)]
pub struct PendingLogin {
    pub id: String,
    pub connection_id: String,
    pub label: String,
    pub provider_key: String,
    pub endpoints: EndpointConfig,
    /// PKCE `state`; also the lookup key for a browser callback.
    pub state: String,
    /// PKCE verifier, consumed by the code exchange.
    pub verifier: String,
    pub redirect_uri: String,
    pub device: Option<DeviceChallenge>,
    pub state_value: LoginState,
    started: Instant,
}

/// A snapshot handed to the status endpoint and the callback.
///
/// The state is flattened so the wire shape reads `{"status":"pending", …}`
/// rather than nesting the tag under a field of the same name.
#[derive(Debug, Clone, Serialize)]
pub struct LoginView {
    pub id: String,
    #[serde(flatten)]
    pub status: LoginState,
    pub provider_key: String,
    pub label: String,
    pub device: Option<DeviceChallenge>,
}

/// Registry of pending logins, shared through `AppState`.
#[derive(Default)]
pub struct LoginRegistry {
    inner: Mutex<HashMap<String, PendingLogin>>,
}

impl LoginRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Stores a new session and returns it.
    pub async fn insert(&self, login: PendingLogin) -> PendingLogin {
        let mut inner = self.inner.lock().await;
        Self::prune(&mut inner);
        inner.insert(login.id.clone(), login.clone());
        login
    }

    /// Looks a session up by its id.
    pub async fn get(&self, id: &str) -> Option<PendingLogin> {
        let inner = self.inner.lock().await;
        inner.get(id).cloned()
    }

    /// Looks a session up by its PKCE `state` nonce.
    ///
    /// The callback route is public, so the nonce is the only thing standing
    /// between a stray request and a stored credential. A blank nonce never
    /// matches: a device login carries no state, and an empty `state` query
    /// parameter must not resolve to one.
    pub async fn by_state(&self, state: &str) -> Option<PendingLogin> {
        if state.is_empty() {
            return None;
        }

        let inner = self.inner.lock().await;
        inner
            .values()
            .find(|login| login.state == state)
            .filter(|login| login.started.elapsed() < LOGIN_TTL)
            .cloned()
    }

    /// Marks a session finished. A completed or failed session is left in place
    /// so the dialog can read the outcome, but the verifier is no longer usable
    /// because `by_state` only matters while pending.
    pub async fn settle(&self, id: &str, state: LoginState) {
        let mut inner = self.inner.lock().await;
        if let Some(login) = inner.get_mut(id) {
            login.state_value = state;
        }
    }

    /// Forgets a session, e.g. when the operator cancels.
    pub async fn remove(&self, id: &str) -> bool {
        let mut inner = self.inner.lock().await;
        inner.remove(id).is_some()
    }

    /// A view of one session for the status endpoint.
    pub async fn view(&self, id: &str) -> Option<LoginView> {
        let login = self.get(id).await?;
        Some(LoginView {
            id: login.id,
            status: login.state_value,
            provider_key: login.provider_key,
            label: login.label,
            device: login.device,
        })
    }

    fn prune(inner: &mut HashMap<String, PendingLogin>) {
        inner.retain(|_, login| login.started.elapsed() < LOGIN_TTL);
    }
}

impl PendingLogin {
    /// A login waiting on a browser redirect.
    #[allow(clippy::too_many_arguments)]
    pub fn pkce(
        id: String,
        connection_id: String,
        label: String,
        provider_key: String,
        endpoints: EndpointConfig,
        state: String,
        verifier: String,
        redirect_uri: String,
    ) -> Self {
        Self {
            id,
            connection_id,
            label,
            provider_key,
            endpoints,
            state,
            verifier,
            redirect_uri,
            device: None,
            state_value: LoginState::Pending,
            started: Instant::now(),
        }
    }

    /// A login waiting on a device-code approval.
    pub fn device(
        id: String,
        connection_id: String,
        label: String,
        provider_key: String,
        endpoints: EndpointConfig,
        device: DeviceChallenge,
    ) -> Self {
        Self {
            id,
            connection_id,
            label,
            provider_key,
            endpoints,
            state: String::new(),
            verifier: String::new(),
            redirect_uri: String::new(),
            device: Some(device),
            state_value: LoginState::Pending,
            started: Instant::now(),
        }
    }
}
