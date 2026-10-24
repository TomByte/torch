pub mod auth_manager;
mod user_properties;
use ::settings::SettingsManager;
pub use auth_manager::AuthManager;
pub use auth_state::AuthStateProvider;
use torch_core::user_preferences::GetUserPreferences as _;
use torch_errors::report_error;
pub use torch_server_auth::{auth_state, credentials, user, user_uid};
use torchui::{AppContext, SingletonEntity};
pub use user_uid::UserUid;

use crate::cloud_object::model::persistence::CloudModel;
use crate::env_vars::manager::EnvVarCollectionManager;
use crate::notebooks::manager::NotebookManager;
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::server::sync_queue::SyncQueue;
use crate::server::telemetry::TelemetryEvent;
use crate::settings::{
    CRASH_REPORTING_ENABLED_DEFAULTS_KEY, CloudPreferencesSettings, PrivacySettings,
    TELEMETRY_ENABLED_DEFAULTS_KEY,
};
use crate::workflows::manager::WorkflowManager;
use crate::workspaces::update_manager::TeamUpdateManager;
use crate::{GlobalResourceHandlesProvider, persistence, send_telemetry_sync_from_app_ctx};

// Log out the user, clears workspace state, stops running processes, and deletes database.
pub fn log_out(app: &mut AppContext) {
    send_telemetry_sync_from_app_ctx!(TelemetryEvent::LogOut, app);

    let global_resource_handles = GlobalResourceHandlesProvider::as_ref(app).get();

    // As part of Logout v0, we remove sqlite3 so sessions and cloud objects don't persist between accounts.
    // TODO: Implement per-user scoping of sqlite3.
    persistence::remove(&global_resource_handles.model_event_sender);

    AuthManager::handle(app).update(app, |auth_manager, ctx| {
        auth_manager.log_out(ctx);
    });
    CloudModel::handle(app).update(app, |cloud_model, _| {
        cloud_model.reset();
    });
    // Clear the sync queue so that we don't try to sync the old user's objects to the new user.
    SyncQueue::handle(app).update(app, |sync_queue, _| {
        sync_queue.clear();
    });

    // Stop the cloud object and workspace metadata polling loops that were started on login.
    UpdateManager::handle(app).update(app, |manager, _| {
        manager.stop_polling_for_updated_objects();
    });
    TeamUpdateManager::handle(app).update(app, |manager, _| {
        manager.stop_polling_for_workspace_metadata_updates();
    });
    remove_cloud_persisted_settings(app);

    NotebookManager::handle(app).update(app, |manager, _| manager.reset());
    EnvVarCollectionManager::handle(app).update(app, |manager, _| manager.reset());
    WorkflowManager::handle(app).update(app, |manager, _| manager.reset());

    #[cfg(target_family = "wasm")]
    crate::platform::wasm::emit_event(crate::platform::wasm::WarpEvent::LoggedOut);
}

// Remove the cloud persisted settings from user defaults.
// When a user signs out, we remove cloud persisted settings of their account.
// This is so they do not experience the old settings when they log in with a different account.
// Partial deletion of user defaults is a stopgap for Logout v0. The correct solution is:
fn remove_cloud_persisted_settings(app: &mut AppContext) {
    let is_settings_sync_enabled = *CloudPreferencesSettings::as_ref(app).settings_sync_enabled;
    if is_settings_sync_enabled {
        SettingsManager::handle(app).update(app, |settings_manager, ctx| {
            let errors = settings_manager.clear_cloud_settings_local_state(ctx);
            for e in errors {
                report_error!(
                    e.context("Failed to remove cloud synced setting from user defaults")
                );
            }
        });
    }

    if let Err(e) = app
        .private_user_preferences()
        .remove_value(TELEMETRY_ENABLED_DEFAULTS_KEY)
    {
        report_error!(
            anyhow::Error::new(e)
                .context("Failed to remove Telemetry Enabled Defaults Key from user defaults")
        );
    }

    if let Err(e) = app
        .private_user_preferences()
        .remove_value(CRASH_REPORTING_ENABLED_DEFAULTS_KEY)
    {
        report_error!(
            anyhow::Error::new(e).context(
                "Failed to remove Crash Reporting Enabled Defaults Key from user defaults"
            )
        );
    }

    // Reset the Privacy Settings in the login screen to default values.
    PrivacySettings::handle(app).update(app, |privacy_settings, _| {
        privacy_settings.refresh_to_default();
    });
}
