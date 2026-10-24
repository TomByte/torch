use std::sync::Arc;

use anyhow::anyhow;
use parking_lot::RwLock;
use torch_errors::report_error;
use torch_graphql::object_permissions::OwnerType;
use torchui_core::{AppContext, Entity, SingletonEntity};
use uuid::Uuid;

use super::anonymous_id::get_or_create_anonymous_id;
use super::credentials::Credentials;
use super::user::FirebaseAuthTokens;

/// AuthState holds the client's server credentials and its local client id.
/// If you need to access AuthState, you can use the AuthStateProvider singleton model.
pub struct AuthState {
    /// A random UUID that identifies this installation.
    anonymous_id: Uuid,

    /// The current authentication credentials.
    credentials: RwLock<Option<Credentials>>,
}

impl AuthState {
    pub fn new(ctx: &AppContext) -> Self {
        Self {
            anonymous_id: get_or_create_anonymous_id(ctx),
            credentials: RwLock::new(None),
        }
    }

    #[cfg(any(test, feature = "integration_tests", feature = "test-util"))]
    pub fn new_for_test() -> Self {
        Self {
            anonymous_id: Uuid::new_v4(),
            credentials: RwLock::new(None),
        }
    }

    /// Returns the current credentials.
    pub fn credentials(&self) -> Option<Credentials> {
        self.credentials.read().clone()
    }

    /// Sets the credentials. Should only be called within the auth module.
    pub fn set_credentials(&self, credentials: Option<Credentials>) {
        *self.credentials.write() = credentials;
    }

    /// Applies bearer-token credentials received from the remote server daemon.
    #[cfg(any(not(target_family = "wasm"), test, feature = "test-util"))]
    pub fn set_remote_server_bearer_token(&self, auth_token: String) {
        if auth_token.is_empty() {
            self.set_credentials(None);
            return;
        }
        self.set_credentials(Some(Credentials::Bearer(auth_token)));
    }

    /// Updates the Firebase auth tokens within the current credentials.
    /// Reports an error if the current credentials are not Firebase.
    pub fn update_firebase_tokens(&self, new_auth_tokens: FirebaseAuthTokens) {
        let mut write_lock = self.credentials.write();
        if let Some(Credentials::Firebase(tokens)) = write_lock.as_mut() {
            *tokens = new_auth_tokens;
        } else {
            report_error!(anyhow!(
                "Tried to update Firebase tokens without Firebase credentials"
            ));
        }
    }

    /// Returns the cached access token, if any exists. This method *will not* check if the JWT is
    /// still valid! Usually, you want to use [`ServerApi::get_or_refresh_access_token`] instead!
    pub fn get_access_token_ignoring_validity(&self) -> Option<String> {
        let credentials = self.credentials.read();
        credentials.as_ref()?.bearer_token().bearer_token()
    }

    /// Returns the client's anonymous id.
    /// The anonymous id will be consistent across the app's lifetime. It is a random UUID.
    pub fn anonymous_id(&self) -> String {
        self.anonymous_id.to_string()
    }

    /// Returns the owner type of the currently-authenticated API key.
    pub fn api_key_owner_type(&self) -> Option<OwnerType> {
        self.credentials.read().as_ref()?.api_key_owner_type()
    }
}

// Adapter for the [`torch_managed_secrets`] crate, which needs to access the current user.
impl torch_managed_secrets::ActorProvider for AuthState {
    fn actor_uid(&self) -> Option<String> {
        None
    }
}

/// AuthStateProvider is a singleton model which provides a reference to the global AuthState.
pub struct AuthStateProvider {
    auth_state: Arc<AuthState>,
}

impl AuthStateProvider {
    pub fn new(auth_state: Arc<AuthState>) -> Self {
        Self { auth_state }
    }

    #[cfg(any(test, feature = "test-util"))]
    pub fn new_for_test() -> Self {
        Self {
            auth_state: Arc::new(AuthState::new_for_test()),
        }
    }

    pub fn get(&self) -> &Arc<AuthState> {
        &self.auth_state
    }
}

impl Entity for AuthStateProvider {
    type Event = ();
}

impl SingletonEntity for AuthStateProvider {}
