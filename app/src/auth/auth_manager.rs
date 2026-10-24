use std::result::Result as StdResult;
use std::sync::Arc;

use settings::Setting as _;
use torch_core::channel::ChannelState;
use torch_core::features::FeatureFlag;
use torch_errors::{report_error, report_if_error};
use torch_server_auth::user::persistence::PersistedUser;
use torchui::{Entity, ModelContext, SingletonEntity, UpdateModel};
#[cfg(target_family = "wasm")]
use url::Url;

use super::AuthStateProvider;
use super::auth_state::{AuthState, PersistAction};
use super::credentials::Credentials;
use super::user::User;
use super::user_properties::UserProperties;
use crate::autoupdate::AutoupdateState;
use crate::persistence::ModelEvent;
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::server::server_api::auth::{AuthClient, FetchUserResult, UserAuthenticationError};
use crate::server::server_api::{ServerApi, ServerApiProvider};
use crate::settings::PrivacySettings;
use crate::settings::cloud_preferences_syncer::CloudPreferencesSyncer;
use crate::settings::initializer::SettingsInitializer;
use crate::terminal::general_settings::GeneralSettings;
#[cfg(target_family = "wasm")]
use crate::uri::browser_url_handler::{parse_current_url, update_browser_url};
use crate::workspaces::team_tester::TeamTesterStatus;
use crate::{GlobalResourceHandlesProvider, TelemetryEvent, persistence, send_telemetry_from_ctx};

#[derive(Debug)]
pub enum AuthManagerEvent {
    /// Successfully authenticated a user with no errors.
    AuthComplete,
    /// Failed to authenticate a user.
    AuthFailed,
    /// The user now needs to reauthenticate. If the user needs to reauth, an `AuthFailed`
    /// event might be triggered instead, but there are some code paths where we don't
    /// refresh the entire user, only their token, which is when this event might be emitted.
    NeedsReauth,
}

/// AuthManager is a singleton model which manages the currently logged-in user's state.
/// If you need to access the state, use `AuthStateProvider`.
pub struct AuthManager {
    auth_state: Arc<AuthState>,
    server_api: Arc<ServerApi>,
    auth_client: Arc<dyn AuthClient>,
}

impl AuthManager {
    /// Creates a new instance of the AuthManager. The auth state must already be initialized through
    /// [`AuthStateProvider`].
    pub fn new(
        server_api: Arc<ServerApi>,
        auth_client: Arc<dyn AuthClient>,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        let auth_state = AuthStateProvider::as_ref(ctx).get().clone();

        Self {
            auth_state,
            server_api,
            auth_client,
        }
    }

    #[cfg(test)]
    pub fn new_for_test(ctx: &mut ModelContext<Self>) -> Self {
        use crate::server::server_api::ServerApiProvider;

        let server_api_provider = ServerApiProvider::as_ref(ctx);
        let server_api = server_api_provider.get();
        let auth_client = server_api_provider.get_auth_client();
        let auth_state = AuthStateProvider::as_ref(ctx).get().clone();

        Self {
            auth_state,
            server_api,
            auth_client,
        }
    }

    #[cfg(target_family = "wasm")]
    pub fn initialize_user_from_session_cookie(&self, ctx: &mut ModelContext<Self>) {
        if ChannelState::is_local_only() {
            return;
        }

        let auth_client = self.auth_client.clone();
        let _ = ctx.spawn(
            async move {
                auth_client
                    .fetch_user(LoginToken::SessionCookie, false)
                    .await
            },
            Self::on_user_fetched,
        );
    }

    /// Refreshes the user's auth state using their existing credentials.
    pub fn refresh_user(&self, ctx: &mut ModelContext<Self>) {
        if ChannelState::is_local_only() {
            return;
        }

        let Some(credentials) = self.auth_state.credentials() else {
            log::warn!("Attempted to refresh user without credentials");
            return;
        };

        let Some(token) = credentials.login_token() else {
            log::info!("Attempted to refresh a user with no login token, skipping");
            return;
        };

        let auth_client = self.auth_client.clone();
        let _ = ctx.spawn(
            async move { auth_client.fetch_user(token, true).await },
            Self::on_user_fetched,
        );
    }

    /// Callback for handling a successful fetch of a user from warp-server and Firebase.
    /// This does the heavy-lifting of setting up all components of the application that depend
    /// on a user's authenticated state, and emits events to subscribers that let them know
    /// an auth event has occurred.
    fn on_user_fetched(
        &mut self,
        fetch_user_result: StdResult<FetchUserResult, UserAuthenticationError>,
        ctx: &mut ModelContext<Self>,
    ) {
        match fetch_user_result {
            Ok(fetch_user_result) => {
                let FetchUserResult {
                    user_output,
                    credentials,
                    from_refresh,
                } = fetch_user_result;
                let UserProperties {
                    user,
                    server_experiments,
                } = user_output.into();

                self.complete_authentication(user.clone(), credentials, ctx);

                self.set_needs_reauth(false, ctx);

                ServerApiProvider::handle(ctx).update(ctx, |provider, ctx| {
                    provider.handle_experiments_fetched(server_experiments, ctx);
                });

                SettingsInitializer::handle(ctx).update(ctx, |initializer, ctx| {
                    initializer.handle_user_fetched(self.auth_state.clone(), ctx);
                });

                // Reset the initial-load condition so that any cloud preference
                // sync waits for the *new* user's cloud objects rather than
                // resolving immediately against stale data from a prior session.
                // Only do this for non-refresh fetches (login/signup), not for
                // token refreshes where the user identity hasn't changed.
                if !from_refresh {
                    UpdateManager::handle(ctx).update(ctx, |manager, _| {
                        manager.reset_initial_load();
                    });
                }

                // Now that we have a user, start polling for team and cloud object information.
                // The polling loop's first tick fires immediately, so there is no need for a
                // separate out-of-band refresh here.
                TeamTesterStatus::handle(ctx).update(ctx, |model, ctx| {
                    model.initiate_data_pollers(false, ctx);
                });

                CloudPreferencesSyncer::handle(ctx).update(ctx, |model, ctx| {
                    model.handle_user_fetched(self.auth_state.clone(), ctx)
                });

                if !user.is_user_anonymous() {
                    GeneralSettings::handle(ctx).update(ctx, |settings, ctx| {
                        report_if_error!(
                            settings.did_non_anonymous_user_log_in.set_value(true, ctx)
                        );
                    });
                }

                let global_resource_handles =
                    GlobalResourceHandlesProvider::as_ref(ctx).get().clone();

                // As part of Logout v0:
                // Reconstruct the database if it was removed.
                // Do nothing if the database was not removed.
                persistence::reconstruct(&global_resource_handles.model_event_sender);
                if let Some(model_event_sender) = &global_resource_handles.model_event_sender
                    && let Err(e) =
                        model_event_sender.send(ModelEvent::UpsertCurrentUserInformation {
                            user_information: PersistedCurrentUserInformation {
                                email: self.auth_state.user_email().unwrap_or_default(),
                            },
                        })
                {
                    report_error!(
                        anyhow::Error::new(e)
                            .context("Error persisting user information to database")
                    );
                };

                // Fetch the user's privacy settings from the server if any or update the server settings.
                let privacy_settings_handle = PrivacySettings::handle(ctx);
                ctx.update_model(&privacy_settings_handle, |privacy_settings, ctx| {
                    privacy_settings.fetch_or_update_settings(ctx);
                });

                // Now that the user is logged in, do the daily version check.
                if FeatureFlag::Autoupdate.is_enabled() {
                    AutoupdateState::handle(ctx).update(ctx, |autoupdate_state, ctx| {
                        autoupdate_state.maybe_daily_check_for_update(ctx);
                    });
                }

                let server_api = self.server_api.clone();
                let _ = ctx.spawn(
                    async move {
                        server_api.notify_login().await;
                    },
                    |_, _, _| {},
                );

                ctx.emit(AuthManagerEvent::AuthComplete);
            }
            Err(error) => {
                match error {
                    UserAuthenticationError::DeniedAccessToken(_) => {
                        self.set_needs_reauth(true, ctx);
                    }
                    UserAuthenticationError::UserAccountDisabled(_) => {}
                    UserAuthenticationError::DeviceCodeRequestTimedOut { .. } => {}
                    UserAuthenticationError::Unexpected(_) => {}
                    UserAuthenticationError::InvalidStateParameter => {}
                    UserAuthenticationError::MissingStateParameter => {}
                }

                ctx.emit(AuthManagerEvent::AuthFailed);
            }
        }
    }

    /// Sets the user and credentials in auth state and persists to secure storage.
    /// Persistence depends on the credential type - currently, we only persist
    /// state if authenticated via a Firebase token.
    fn complete_authentication(
        &self,
        user: User,
        credentials: Credentials,
        ctx: &mut ModelContext<Self>,
    ) {
        self.set_and_persist(Some(user), Some(credentials), ctx);
    }
    fn set_and_persist(
        &self,
        user: Option<User>,
        credentials: Option<Credentials>,
        ctx: &mut ModelContext<Self>,
    ) {
        self.auth_state.set_user(user);
        self.auth_state.set_credentials(credentials);
        self.persist(ctx);
    }

    /// Persists (or removes) the current user and credentials to/from secure storage,
    /// based on the current auth state.
    fn persist(&self, ctx: &mut ModelContext<Self>) {
        match self.auth_state.persist_action() {
            PersistAction::Persist(persisted_user) => {
                if persisted_user.auth_tokens.refresh_token.is_empty() {
                    log::warn!("Skipping user persistence due to empty refresh token");
                    return;
                }
                let _ = persisted_user.write_to_secure_storage(ctx).map_err(|err| {
                    log::warn!("Unable to persist user to secure storage: {err:?}");
                });
            }
            PersistAction::Remove => {
                let _ = PersistedUser::remove_from_secure_storage(ctx).map_err(|err| {
                    log::warn!("Unable to clear user from secure storage: {err:?}");
                });
            }
            PersistAction::DoNothing => {}
        }
    }

    /// Helper function for logging out the user.
    /// NOTE: You probably want to call auth::log_out instead; this only manages the auth state,
    /// it doesn't shut down any other user-dependent parts of the app.
    /// TODO(jeff): Can we move those pieces in here?
    pub(super) fn log_out(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_and_persist(None, None, ctx);
    }

    /// Sets whether or not this user's Firebase credentials are invalid and thus needs to reauth.
    pub fn set_needs_reauth(&self, needs_reauth: bool, ctx: &mut ModelContext<Self>) {
        let became_true = self.auth_state.set_needs_reauth(needs_reauth);

        if became_true {
            send_telemetry_from_ctx!(TelemetryEvent::NeedsReauth, ctx);
            ctx.emit(AuthManagerEvent::NeedsReauth);
        }
    }
}

#[derive(Clone, Debug)]
pub struct PersistedCurrentUserInformation {
    pub email: String,
}

impl Entity for AuthManager {
    type Event = AuthManagerEvent;
}

impl SingletonEntity for AuthManager {}
