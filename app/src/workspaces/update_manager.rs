use std::sync::Arc;
use std::sync::mpsc::SyncSender;
#[cfg(not(target_family = "wasm"))]
use std::time::Duration;

use anyhow::{Context, Result};
use futures::channel::oneshot::{self, Receiver};
use futures::stream::AbortHandle;
use torch_errors::{report_error, report_if_error};
use torchui::r#async::{Spawnable, Timer};
use torchui::{
    Entity, ModelContext, ModelHandle, RequestState, SingletonEntity, duration_with_jitter,
};

use super::team_tester::{TeamTesterStatus, TeamTesterStatusEvent};
use super::user_workspaces::{
    UserWorkspaces, WorkspacesMetadataResponse, WorkspacesMetadataWithPricing,
};
use super::workspace::WorkspaceUid;
use crate::ai::request_usage_model::AIRequestUsageModel;
use crate::auth::AuthStateProvider;
use crate::network::{NetworkStatus, NetworkStatusEvent, NetworkStatusKind};
use crate::persistence::ModelEvent;
use crate::server::retry_strategies::{
    OUT_OF_BAND_REQUEST_RETRY_STRATEGY, PERIODIC_POLL, PERIODIC_POLL_RETRY_STRATEGY, with_retry,
};
use crate::server::server_api::ServerApiProvider;
use crate::server::server_api::team::TeamClient;

#[derive(Clone, Copy)]
pub(crate) enum WorkspaceMetadataRetryPolicy {
    OutOfBand,
    #[cfg(not(target_family = "wasm"))]
    TimeBudget(Duration),
}

/// TeamUpdateManager is a singleton model responsible for communicating with the server and local
/// database regarding teams' metadata.
/// It emits events that are later processed by UserWorkspaces model (which is an in-memory store for
/// the workspace metadata).
/// TeamUpdateManager is used when sending a team-related request to the server and processing the
/// response, but also controls the periodic polling from the server (also controlled by calling
/// `force_refresh` method).
pub struct TeamUpdateManager {
    team_client: Arc<dyn TeamClient>,
    model_event_sender: Option<SyncSender<ModelEvent>>,
    should_poll_for_workspace_metadata_updates: bool,

    /// The abort handle for the timer that waits a fixed duration
    /// before making an outbound request for workspace metadata, if any.
    next_poll_abort_handle: Option<AbortHandle>,

    /// The abort handle for the in flight request of workspace metadata,
    /// if any.
    in_flight_request_abort_handle: Option<AbortHandle>,
}

impl TeamUpdateManager {
    pub fn new(
        team_client: Arc<dyn TeamClient>,
        model_event_sender: Option<SyncSender<ModelEvent>>,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        let network_status = NetworkStatus::handle(ctx);
        ctx.subscribe_to_model(&network_status, Self::handle_network_status_changed);

        let team_tester_status = TeamTesterStatus::handle(ctx);
        ctx.subscribe_to_model(&team_tester_status, Self::handle_team_tester_status_changed);

        Self {
            team_client,
            model_event_sender,
            should_poll_for_workspace_metadata_updates: false,
            next_poll_abort_handle: None,
            in_flight_request_abort_handle: None,
        }
    }

    fn handle_network_status_changed(
        &mut self,
        _: ModelHandle<NetworkStatus>,
        network_status: &NetworkStatusEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        match network_status {
            NetworkStatusEvent::NetworkStatusChanged { new_status } => match new_status {
                NetworkStatusKind::Online => {
                    // TODO: this will cause us to reset our polling very frequently
                    // if the client's network conn is repeatedly flipping between on and off.
                    self.start_polling_for_workspace_metadata_updates(ctx);
                }
                NetworkStatusKind::Offline => self.stop_polling_for_workspace_metadata_updates(),
            },
        }
    }

    fn handle_team_tester_status_changed(
        &mut self,
        _: ModelHandle<TeamTesterStatus>,
        event: &TeamTesterStatusEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        let TeamTesterStatusEvent::InitiateDataPollers { force_refresh } = event;
        if *force_refresh {
            std::mem::drop(self.refresh_workspace_metadata(ctx));
        }

        self.start_polling_for_workspace_metadata_updates(ctx);
    }

    #[cfg(test)]
    pub fn mock(ctx: &mut ModelContext<Self>) -> Self {
        use crate::server::server_api::team::MockTeamClient;

        // This mock API is used in test contexts where we don't care which teams the user is on.
        // Since the mocked `TeamClient` is inaccessible to tests, stub the metadata polling to
        // avoid noisy `No matching expectation found` errors.
        let mut team_client = MockTeamClient::new();
        team_client.expect_workspaces_metadata().returning(|| {
            Ok(WorkspacesMetadataWithPricing {
                metadata: WorkspacesMetadataResponse {
                    workspaces: vec![],
                    joinable_teams: vec![],
                    experiments: None,
                    ai_credit_availability: None,
                    user_tier: Default::default(),
                },
                pricing_info: None,
            })
        });

        Self::new(Arc::new(team_client), Default::default(), ctx)
    }

    /// Starts a periodic poll for workspace metadata changes, if there isn't already
    /// an existing poll queued up.
    pub fn start_polling_for_workspace_metadata_updates(&mut self, ctx: &mut ModelContext<Self>) {
        let is_online = NetworkStatus::as_ref(ctx).is_online();
        if !self.should_poll_for_workspace_metadata_updates && is_online {
            self.should_poll_for_workspace_metadata_updates = true;
            self.poll_for_workspace_metadata_changes(ctx);
        }
    }

    pub fn stop_polling_for_workspace_metadata_updates(&mut self) {
        self.should_poll_for_workspace_metadata_updates = false;
        self.abort_existing_poll();
    }

    /// Out-of-band (from the regular poll) refresh of workspace metadata.
    /// Returns a oneshot Receiver that resolves with the final refresh result.
    pub fn refresh_workspace_metadata(
        &mut self,
        ctx: &mut ModelContext<Self>,
    ) -> Receiver<Result<()>> {
        // Skip the refresh when logged out to avoid noisy auth errors.
        if !AuthStateProvider::as_ref(ctx).get().is_logged_in() {
            let (tx, rx) = oneshot::channel::<Result<()>>();
            let _ = tx.send(Ok(()));
            return rx;
        }

        let (tx, rx) = oneshot::channel::<Result<()>>();
        let refresh = self
            .refresh_workspace_metadata_with_retry(WorkspaceMetadataRetryPolicy::OutOfBand, ctx);
        ctx.spawn(refresh, move |_, result, _| {
            let _ = tx.send(result);
        });
        rx
    }

    pub(crate) fn refresh_workspace_metadata_with_retry(
        &mut self,
        retry_policy: WorkspaceMetadataRetryPolicy,
        ctx: &mut ModelContext<Self>,
    ) -> impl Spawnable<Output = Result<()>> + use<> {
        let team_client = AuthStateProvider::as_ref(ctx)
            .get()
            .is_logged_in()
            .then(|| self.team_client.clone());
        let foreground = ctx.spawner();

        async move {
            let Some(team_client) = team_client else {
                return Ok(());
            };
            let mut retry_option = OUT_OF_BAND_REQUEST_RETRY_STRATEGY;
            let refresh = with_retry(
                "Workspace metadata",
                || team_client.workspaces_metadata(),
                // The workload token may be rejected until the execution claim is committed.
                |_| true,
                |delay| async move {
                    match retry_policy {
                        WorkspaceMetadataRetryPolicy::OutOfBand => {
                            Timer::after(delay).await;
                        }
                        #[cfg(not(target_family = "wasm"))]
                        WorkspaceMetadataRetryPolicy::TimeBudget(_) => {
                            tokio::time::sleep(delay).await
                        }
                    }
                },
                |_| match retry_policy {
                    WorkspaceMetadataRetryPolicy::OutOfBand => {
                        if retry_option.remaining_retries() == 0 {
                            return None;
                        }
                        let delay = retry_option.duration();
                        retry_option.advance();
                        Some(delay)
                    }
                    #[cfg(not(target_family = "wasm"))]
                    WorkspaceMetadataRetryPolicy::TimeBudget(_) => {
                        Some(duration_with_jitter(Duration::from_secs(1), 0.5))
                    }
                },
            );
            let response = match retry_policy {
                WorkspaceMetadataRetryPolicy::OutOfBand => refresh.await,
                #[cfg(not(target_family = "wasm"))]
                WorkspaceMetadataRetryPolicy::TimeBudget(timeout) => {
                    tokio::time::timeout(timeout, refresh)
                        .await
                        .map_err(|_| anyhow::anyhow!("Timed out refreshing team metadata"))?
                }
            };

            foreground
                .spawn(move |manager, ctx| {
                    manager
                        .handle_workspace_metadata_with_request_state(
                            match response {
                                Ok(response) => RequestState::RequestSucceeded(response),
                                Err(error) => RequestState::RequestFailed(error),
                            },
                            ctx,
                        )
                        .expect("Metadata refresh has a terminal result")
                })
                .await
                .context("Team metadata refresh was canceled")?
        }
    }

    fn abort_existing_poll(&mut self) {
        if let Some(abort_handle) = self.in_flight_request_abort_handle.take() {
            abort_handle.abort();
        }

        if let Some(abort_handle) = self.next_poll_abort_handle.take() {
            abort_handle.abort();
        }
    }

    /// Only call this method if you need to restart the poll and force a refresh.
    /// Currently called when
    /// - we decide that a feature flag needs to change
    /// - find out that a user is a team tester
    /// - a network status changes (we go from offline to online state)
    ///
    /// Note: the gql query for this poll also pulls in experiment state. If we change
    /// the behaviour for polling workspace metadata, we should consider what ramifications
    /// that has on querying experiment state.
    fn poll_for_workspace_metadata_changes(&mut self, ctx: &mut ModelContext<Self>) {
        self.abort_existing_poll();

        if !self.should_poll_for_workspace_metadata_updates {
            return;
        }

        // Don't poll when the user is logged out to avoid spamming auth errors in the logs.
        // Polling will be restarted when the user logs in via `initiate_data_pollers`.
        if !AuthStateProvider::as_ref(ctx).get().is_logged_in() {
            self.should_poll_for_workspace_metadata_updates = false;
            return;
        }

        let team_client = self.team_client.clone();
        // We retry a few times here in case there are any transient network errors.
        let spawn_handle = ctx.spawn_with_retry_on_error(
            move || {
                let team_client = team_client.clone();
                async move {
                    team_client
                        .workspaces_metadata()
                        .await
                        .context("Error polling for workspace metadata changes")
                }
            },
            PERIODIC_POLL_RETRY_STRATEGY,
            |update_manager, res, ctx| {
                // Only poll if `spawn_with_retry_on_error` is not going to retry again so we don't end up with multiple
                // polls running simultaneously.
                let should_poll_again = !res.has_pending_retries();
                let _ = update_manager.handle_workspace_metadata_with_request_state(res, ctx);

                if should_poll_again {
                    let next_poll_handle = ctx.spawn(
                        async move {
                            Timer::after(duration_with_jitter(
                                PERIODIC_POLL,
                                0.2, /* max_jitter_multiplier */
                            ))
                            .await
                        },
                        |update_manager, _, ctx| {
                            update_manager.poll_for_workspace_metadata_changes(ctx);
                        },
                    );
                    update_manager.next_poll_abort_handle = Some(next_poll_handle.abort_handle());
                }
            },
        );

        self.in_flight_request_abort_handle = Some(spawn_handle.abort_handle());
    }

    fn save_to_db(&self, events: impl IntoIterator<Item = ModelEvent>) {
        let model_event_sender = self.model_event_sender.clone();
        if let Some(model_event_sender) = &model_event_sender {
            for event in events {
                report_if_error!(
                    model_event_sender
                        .send(event)
                        .context("Unable to save teams metadata to sqlite")
                );
            }
        }
    }

    fn handle_workspace_metadata_with_request_state(
        &mut self,
        request_state: RequestState<WorkspacesMetadataWithPricing>,
        ctx: &mut ModelContext<Self>,
    ) -> Option<Result<()>> {
        match request_state {
            RequestState::RequestSucceeded(response) => {
                // Right now, this function is coupled with how we handle leaving a team.
                // TODO(zheng) refactor so we can separate these two cases and have clearer logic.
                self.on_workspaces_updated(Ok(response.metadata), ctx);
                Some(Ok(()))
            }
            RequestState::RequestFailedRetryPending(err) => {
                log::info!(
                    "get_workspaces_metadata_for_user: request failed with error {err:#}. Trying again."
                );
                None
            }
            RequestState::RequestFailed(err) => {
                log::info!(
                    "get_workspaces_metadata_for_user: request failed with error {err:#}. Retries exhausted."
                );
                Some(Err(err))
            }
        }
    }

    fn on_workspaces_updated(
        &mut self,
        result: Result<WorkspacesMetadataResponse>,
        ctx: &mut ModelContext<Self>,
    ) {
        match result {
            Ok(user_workspaces_access) => {
                let workspaces = user_workspaces_access.workspaces;
                let joinable_teams = user_workspaces_access.joinable_teams;
                let experiments = user_workspaces_access.experiments;
                let user_tier = user_workspaces_access.user_tier;

                if let Some(availability) = user_workspaces_access.ai_credit_availability {
                    AIRequestUsageModel::handle(ctx).update(ctx, |usage_model, ctx| {
                        usage_model.apply_server_availability(Ok(availability), ctx);
                    });
                }

                UserWorkspaces::handle(ctx).update(ctx, |user_workspaces, ctx| {
                    user_workspaces.set_user_tier(user_tier);
                    user_workspaces.update_workspaces(workspaces.clone(), ctx);
                    user_workspaces.update_joinable_teams(joinable_teams.clone(), ctx);
                });

                // Check if the current workspace is still in the list of workspaces.
                // If it's not, then set the current workspace to the first workspace in the list.
                if let Some(current_workspace) = UserWorkspaces::as_ref(ctx).current_workspace() {
                    if !workspaces.iter().any(|w| w.uid == current_workspace.uid)
                        && let Some(workspace_uid) = workspaces.first().map(|w| w.uid)
                    {
                        self.set_current_workspace_uid(workspace_uid, ctx);
                    };
                } else if let Some(workspace_uid) = workspaces.first().map(|w| w.uid) {
                    self.set_current_workspace_uid(workspace_uid, ctx);
                }

                if let Some(experiments) = experiments {
                    ServerApiProvider::handle(ctx).update(ctx, |provider, ctx| {
                        provider.handle_experiments_fetched(experiments, ctx);
                    });
                }

                // Update sqlite
                self.save_to_db([ModelEvent::UpsertWorkspaces { workspaces }]);
            }
            Err(e) => {
                report_error!(e);
            }
        }
    }

    pub fn set_current_workspace_uid(
        &mut self,
        workspace_uid: WorkspaceUid,
        ctx: &mut ModelContext<Self>,
    ) {
        UserWorkspaces::handle(ctx).update(ctx, |user_workspaces, ctx| {
            user_workspaces.set_current_workspace_uid(workspace_uid, ctx);
        });

        // Update sqlite
        self.save_to_db([ModelEvent::SetCurrentWorkspace { workspace_uid }]);
    }
}

impl Entity for TeamUpdateManager {
    type Event = ();
}

impl SingletonEntity for TeamUpdateManager {}
