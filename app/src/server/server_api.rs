pub mod auth;
#[cfg(not(target_family = "wasm"))]
pub(crate) mod download;
pub mod managed_secrets;
pub mod team;
pub mod workspace;

use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;

use ::http::header::CONTENT_LENGTH;
use anyhow::{Result, anyhow};
use auth::AuthClient;
use channel_versions::ChannelVersions;
use chrono::{DateTime, FixedOffset};
use instant::Instant;
use managed_secrets::AppManagedSecretsClient;
use parking_lot::Mutex;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use team::TeamClient;
use torch_core::context_flag::ContextFlag;
use torch_errors::report_error;
use torch_server_client::auth::{AuthClientImpl, AuthEvent, EXPERIMENT_ID_HEADER};
use torch_server_client::base_client::{
    AuthenticatedGraphqlConfig, BaseClient, GraphqlRoutingConfig,
};
use torch_server_client::iap::{IapManager, IapState};
use torch_server_client::network_logging::NetworkLogModel;
use torchui::r#async::BoxFuture;
use torchui::{Entity, ModelContext, SingletonEntity};
use url::Url;
use workspace::WorkspaceClient;

use super::experiments::{ServerExperiment, ServerExperiments};
use crate::ChannelState;
use crate::auth::auth_manager::AuthManager;
use crate::auth::auth_state::AuthState;
use crate::server::team_scope::RequestTeamScope;

pub const FETCH_CHANNEL_VERSIONS_TIMEOUT: std::time::Duration = Duration::from_secs(60);

/// ResponseType received by Client
#[derive(thiserror::Error, Debug, Serialize, Deserialize)]
#[error("{error}")]
pub struct ClientError {
    pub error: String,
    // We unconditionally check for GitHub auth errors in any public API response. It'd be much better
    // to have the server return error codes that we can parse, but this isn't yet supported.
    // See REMOTE-666
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_url: Option<String>,
}

impl Deref for ServerApi {
    type Target = BaseClient;

    fn deref(&self) -> &Self::Target {
        &self.base_client
    }
}

#[derive(Deserialize, Debug)]
struct TimeResponse {
    current_time: DateTime<FixedOffset>,
}

#[derive(Debug, Clone)]
pub struct ServerTime {
    time_at_fetch: DateTime<FixedOffset>,
    fetched_at: Instant,
}

impl ServerTime {
    pub fn current_time(&self) -> DateTime<FixedOffset> {
        let elapsed = chrono::Duration::from_std(self.fetched_at.elapsed())
            .expect("duration should not be bigger than limit");
        self.time_at_fetch + elapsed
    }
}

/// An API wrapper struct with methods to requests to warp-server.
///
/// Prefer NOT adding new methods directly on this struct; instead, add to one of the existing
/// client trait objects, or create your own. This helps keep `ServerApi` from being overloaded
/// with disparate types of calls, and allows you to mock methods in tests.
pub struct ServerApi {
    base_client: Arc<BaseClient>,
    last_server_time: Arc<Mutex<Option<ServerTime>>>,
}

impl ServerApi {
    fn new(
        auth_state: Arc<AuthState>,
        event_sender: async_channel::Sender<AuthEvent>,
        iap_state: Option<Arc<IapState>>,
        ctx: &mut ModelContext<ServerApiProvider>,
    ) -> Self {
        let mut client = http_client::Client::new();
        let mut external_client = http_client::Client::new_external();
        let iap_token_provider = iap_state.map(|state| {
            client.set_iap_token_provider(state.clone());
            state as Arc<dyn http_client::iap::IapTokenProvider>
        });
        if ContextFlag::NetworkLogConsole.is_enabled() {
            NetworkLogModel::handle(ctx).update(ctx, |model, model_ctx| {
                model.install_on_clients([&mut client, &mut external_client], model_ctx);
            });
        }
        Self::new_with_parts(
            Arc::new(client),
            Arc::new(external_client),
            auth_state,
            event_sender,
            iap_token_provider,
        )
    }

    fn new_with_parts(
        client: Arc<http_client::Client>,
        external_client: Arc<http_client::Client>,
        auth_state: Arc<AuthState>,
        event_sender: async_channel::Sender<AuthEvent>,
        iap_token_provider: Option<Arc<dyn http_client::iap::IapTokenProvider>>,
    ) -> Self {
        let graphql_routing = GraphqlRoutingConfig {
            #[cfg(feature = "agent_mode_evals")]
            path_prefix: Some("/agent-mode-evals".to_string()),
            #[cfg(not(feature = "agent_mode_evals"))]
            path_prefix: None,
        };
        let authenticated_graphql = AuthenticatedGraphqlConfig::default();
        let base_client = Arc::new(BaseClient::new(
            client,
            external_client,
            auth_state,
            event_sender,
            None,
            graphql_routing,
            authenticated_graphql,
            iap_token_provider,
        ));

        Self {
            base_client,
            last_server_time: Arc::new(Mutex::new(None)),
        }
    }

    #[cfg(test)]
    fn new_for_test() -> Self {
        let (tx, _) = async_channel::unbounded();
        let auth_state = Arc::new(AuthState::new_for_test());
        let client = Arc::new(http_client::Client::new_for_test());

        Self::new_with_parts(
            client,
            Arc::new(http_client::Client::new_external()),
            auth_state,
            tx,
            None,
        )
    }

    #[cfg(all(test, feature = "skip_login"))]
    fn new_for_test_with_bearer_token(
        bearer_token: Option<String>,
        event_sender: async_channel::Sender<AuthEvent>,
    ) -> Self {
        let auth_state = Arc::new(AuthState::new_logged_out_for_test());
        if let Some(bearer_token) = bearer_token {
            auth_state.set_remote_server_bearer_token(bearer_token);
        }
        Self::new_with_parts(
            Arc::new(http_client::Client::new_for_test()),
            Arc::new(http_client::Client::new_external()),
            auth_state,
            event_sender,
            None,
        )
    }

    pub fn send_graphql_request<'a, QF, O: torch_graphql::client::Operation<QF> + Send + 'a>(
        &'a self,
        operation: O,
        timeout: Option<Duration>,
    ) -> BoxFuture<'a, Result<QF>>
    where
        QF: 'a,
    {
        torch_server_client::graphql_helpers::send_graphql_request(
            &self.base_client,
            operation,
            timeout,
        )
    }

    fn send_graphql_request_for_team<'a, QF, O: torch_graphql::client::Operation<QF> + Send + 'a>(
        &'a self,
        operation: O,
        team_scope: RequestTeamScope,
    ) -> BoxFuture<'a, Result<QF>>
    where
        QF: 'a,
    {
        match Self::team_uid_header_value(team_scope) {
            Some(team_uid) => {
                torch_server_client::graphql_helpers::send_team_scoped_graphql_request(
                    &self.base_client,
                    operation,
                    None,
                    team_uid,
                )
            }
            None => torch_server_client::graphql_helpers::send_graphql_request(
                &self.base_client,
                operation,
                None,
            ),
        }
    }

    fn team_uid_header_value(team_scope: RequestTeamScope) -> Option<String> {
        team_scope
            .team_uid()
            .map(|team_uid| team_uid.uid().to_string())
    }

    /// Sends an authenticated empty POST request to /client/login, which signals to the server
    /// that the user is logged in.
    pub async fn notify_login(&self) {
        match self.get_or_refresh_access_token().await {
            Ok(auth_token) => {
                let url = format!("{}/client/login", ChannelState::server_root_url());
                let mut request = self.base_client.http_client().post(&url);
                if let Some(token) = auth_token.as_bearer_token() {
                    request = request.bearer_auth(token);
                }
                request = request
                    // Set the content-length header to 0 because the request has no body.
                    // Otherwise, the server will return a 411 error. (In other cases, setting
                    // content-type is sufficient (elides the content-length requirement), but
                    // since this request has no body, it makes more sense to set content-length.
                    .header(CONTENT_LENGTH, 0)
                    .header(EXPERIMENT_ID_HEADER, self.anonymous_id());

                let response = request.send().await;
                if let Err(err) = response {
                    report_error!(
                        anyhow::Error::new(err)
                            .context("Failed to send POST request to /client/login")
                    );
                }
            }
            Err(err) => {
                report_error!(
                    err.context("Could not retrieve access token for notifying user login")
                );
            }
        }
    }

    fn set_server_time(&self, server_time: ServerTime) {
        let mut last_server_time = self.last_server_time.lock();
        *last_server_time = Some(server_time);
    }

    fn cached_server_time(&self) -> Option<ServerTime> {
        let last_server_time = self.last_server_time.lock();
        last_server_time.as_ref().cloned()
    }

    pub async fn server_time(&self) -> Result<ServerTime> {
        if let Some(cached) = self.cached_server_time() {
            return Ok(cached);
        }

        let time_endpoint = format!("{}/current_time", ChannelState::server_root_url());
        log::info!("Sending server time request to {}", &time_endpoint);
        let res = self
            .base_client
            .http_client()
            .get(&time_endpoint)
            .send()
            .await?;

        if !res.status().is_success() {
            self.observe_iap_challenge(&res);
        }

        match res.status() {
            StatusCode::OK => {
                let time_response: TimeResponse = res.json().await?;
                log::info!(
                    "Received current time from server: {:?}",
                    &time_response.current_time
                );
                let server_time = ServerTime {
                    time_at_fetch: time_response.current_time,
                    fetched_at: Instant::now(),
                };
                let res = Ok(server_time.clone());
                self.set_server_time(server_time);

                res
            }
            _ => {
                let payload: ClientError = res.json().await?;
                Err(anyhow!(payload).context("fetching time from server failed"))
            }
        }
    }

    /// Fetches updated Torch Channel Versions from Torch Server. If it is the first such request of
    /// the current calendar day, first attempts to call the '/client_version/daily'. If that call
    /// fails or if it not the first request of the calendar day, returns the result of a call to
    /// `/client_version'. The caller can specify whether or not changelog information should be
    /// included in the response based on whether or not it will be used.
    pub async fn fetch_channel_versions(
        &self,
        include_changelogs: bool,
        is_daily: bool,
    ) -> Result<ChannelVersions> {
        let mut url = Url::parse(&ChannelState::server_root_url())
            .expect("Should not fail to parse server root URL");
        if is_daily {
            url.set_path("/client_version/daily");
        } else {
            url.set_path("/client_version");
        }
        url.query_pairs_mut()
            .append_pair("include_changelogs", &include_changelogs.to_string());

        if include_changelogs {
            log::info!("Fetching channel versions and changelogs from Torch server");
        } else {
            log::info!("Fetching channel versions (without changelogs) from Torch server");
        }

        let mut request_builder = self
            .base_client
            .http_client()
            .get(url.as_str())
            .timeout(FETCH_CHANNEL_VERSIONS_TIMEOUT)
            .header(EXPERIMENT_ID_HEADER, self.anonymous_id());

        // Authorization for /client_version is optional. Attach authorization header if an access
        // token is present. First, try to get a valid token. If our cached one is expired, try to
        // refresh. Failing that, send the expired token.
        let auth_token = self
            .get_or_refresh_access_token()
            .await
            .ok()
            .and_then(|token| token.bearer_token())
            .or_else(|| self.access_token_ignoring_validity());
        if let Some(token_str) = auth_token {
            request_builder = request_builder.bearer_auth(token_str);
        }

        let response = request_builder.send().await?;
        if !response.status().is_success() {
            self.observe_iap_challenge(&response);
        }
        let versions: ChannelVersions = response.json().await?;
        log::info!("Received channel versions from Torch server: {versions}");
        Ok(versions)
    }
}

/// A singleton entity that provides access to the global [`ServerApi`] instance,
/// or any of its implemented trait objects.
pub struct ServerApiProvider {
    server_api: Arc<ServerApi>,
    auth_client: Arc<dyn AuthClient>,
}

impl ServerApiProvider {
    /// Constructs a new ServerApiProvider.
    #[cfg_attr(target_family = "wasm", allow(unused_variables))]
    pub fn new(
        auth_state: Arc<AuthState>,
        iap_state: Option<Arc<IapState>>,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        let (event_sender, event_receiver) = async_channel::bounded(10);

        let server_api = ServerApi::new(auth_state.clone(), event_sender, iap_state, ctx);

        ctx.spawn_stream_local(
            event_receiver,
            move |_, event, ctx| {
                match event {
                    AuthEvent::UserAccountDisabled => {
                        // We dispatch a global action here because the log out code requires
                        // `server_api`, causing a circular model reference panic when it calls
                        // `ServerApiProvider` to get access.
                        // TODO: We should remove this pattern where `ServerApiProvider` responds
                        // to events; it's prone to these sorts of circular reference issues.
                        ctx.dispatch_global_action("app:log_out", ());
                    }
                    AuthEvent::NeedsReauth => {
                        // AuthManager depends on a reference to ServerApi, so ServerApi can't easily
                        // hold a ref to AuthManager. To get around this, we emit an event on ServerApi
                        // and handle calling the AuthManager here instead.
                        AuthManager::handle(ctx).update(ctx, |auth_manager, ctx| {
                            auth_manager.set_needs_reauth(true, ctx);
                        });
                    }
                    AuthEvent::IapChallengeReceived => {
                        IapManager::handle(ctx)
                            .update(ctx, |manager, ctx| manager.handle_challenge(ctx));
                    }
                    // Re-emit the event for subscribers.
                    // TODO: we probably want a different type for the event emitted to subscribers
                    // from the one that's used for the async channel.
                    _ => ctx.emit(event),
                }
            },
            |_, _| {},
        );
        let server_api = Arc::new(server_api);
        let auth_client = Arc::new(AuthClientImpl::new(server_api.base_client.clone()));
        Self {
            server_api,
            auth_client,
        }
    }

    /// Handles fetching server-side experiments by updating the appropriate app state.
    pub fn handle_experiments_fetched(
        &self,
        experiments: Vec<ServerExperiment>,
        ctx: &mut ModelContext<Self>,
    ) {
        ServerExperiments::handle(ctx).update(ctx, |state, ctx| {
            state.apply_latest_state(experiments, ctx);
        });
    }

    /// Constructs a new SeverApiProvider for tests.
    #[cfg(test)]
    pub fn new_for_test() -> Self {
        let server_api = Arc::new(ServerApi::new_for_test());
        let auth_client = Arc::new(AuthClientImpl::new(server_api.base_client.clone()));
        Self {
            server_api,
            auth_client,
        }
    }

    /// Returns a handle to the underlying [`ServerApi`] object.
    /// Prefer retrieving a specific trait object related to the methods you're calling.
    pub fn get(&self) -> Arc<ServerApi> {
        self.server_api.clone()
    }

    pub fn get_auth_client(&self) -> Arc<dyn AuthClient> {
        self.auth_client.clone()
    }

    pub fn get_workspace_client(&self) -> Arc<dyn WorkspaceClient> {
        self.server_api.clone()
    }

    pub fn get_team_client(&self) -> Arc<dyn TeamClient> {
        self.server_api.clone()
    }

    pub fn get_managed_secrets_client(&self) -> Arc<AppManagedSecretsClient> {
        self.server_api.clone()
    }

    /// Returns the shared HTTP client. This client is wired into network logging
    /// and includes standard Torch request headers.
    pub fn get_http_client(&self) -> Arc<http_client::Client> {
        self.server_api.owned_http_client()
    }
}

impl Entity for ServerApiProvider {
    type Event = AuthEvent;
}

impl SingletonEntity for ServerApiProvider {}
