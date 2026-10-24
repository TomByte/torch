use remote_server::auth::RemoteServerAuthContext;
use torch_core::user_preferences::GetUserPreferences;
use uuid::Uuid;

/// Preferences key holding the client id. The name predates its use as the remote-server identity
/// and is kept so existing installs retain the same id and daemon directories.
const CLIENT_ID_KEY: &str = "ExperimentId";

/// Builds the app-wide context used by remote-server connections. Torch has no account, so the
/// connection carries no credential and is partitioned by the local client id.
pub fn local_auth_context(ctx: &dyn GetUserPreferences) -> RemoteServerAuthContext {
    let client_id = get_or_create_client_id(ctx).to_string();
    RemoteServerAuthContext::new(|| Box::pin(async { None }), move || client_id.clone())
}

/// Returns the persisted client id, generating and saving a new one if none is valid.
fn get_or_create_client_id(ctx: &dyn GetUserPreferences) -> Uuid {
    let persisted = ctx
        .private_user_preferences()
        .read_value(CLIENT_ID_KEY)
        .unwrap_or_default()
        .and_then(|value| match Uuid::parse_str(&value) {
            Ok(uuid) => Some(uuid),
            Err(e) => {
                log::warn!("Error parsing persisted client id from user defaults: {e:?}");
                None
            }
        });
    persisted.unwrap_or_else(|| {
        let uuid = Uuid::new_v4();
        let _ = ctx
            .private_user_preferences()
            .write_value(CLIENT_ID_KEY, uuid.to_string());
        uuid
    })
}
