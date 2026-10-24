use std::sync::Arc;

use remote_server::auth::RemoteServerAuthContext;

use crate::auth::auth_state::AuthState;

/// Builds the app-wide context used by remote-server connections. Torch has no account, so the
/// connection carries no credential and is partitioned by the local client id.
pub fn server_api_auth_context(auth_state: Arc<AuthState>) -> RemoteServerAuthContext {
    RemoteServerAuthContext::new(
        || Box::pin(async { None }),
        move || auth_state.anonymous_id(),
        String::new(),
        String::new(),
        false,
    )
}
