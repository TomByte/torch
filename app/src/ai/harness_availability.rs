use std::collections::HashMap;
use std::time::Duration;

use instant::Instant;
use serde::{Deserialize, Serialize};
use torch_cli::agent::Harness;
use torch_core::features::FeatureFlag;
use torch_core::user_preferences::GetUserPreferences;
use torch_errors::report_error;
use torchui::{Entity, ModelContext, RequestState, SingletonEntity};

use crate::ai::harness_display;
use crate::auth::AuthStateProvider;
use crate::auth::auth_manager::{AuthManager, AuthManagerEvent};
use crate::network::{NetworkStatus, NetworkStatusEvent, NetworkStatusKind};
use crate::server::ids::ServerId;
use crate::server::retry_strategies::{
    OUT_OF_BAND_REQUEST_RETRY_STRATEGY, is_transient_graphql_or_http_error,
};
use crate::server::server_api::ServerApiProvider;
use crate::server::team_scope::RequestTeamScope;
use crate::workspaces::user_workspaces::{TeamScope, UserWorkspaces, UserWorkspacesEvent};

const CACHE_KEY: &str = "AvailableHarnesses";
const AUTH_SECRET_FETCH_FAILURE_COOLDOWN: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HarnessModelInfo {
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_level: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HarnessAvailability {
    pub harness: Harness,
    pub display_name: String,
    pub enabled: bool,
    #[serde(default)]
    pub available_models: Vec<HarnessModelInfo>,
}

/// Default fallback used before the server responds.
/// Oz is enabled by default so the UI is usable pre-fetch; the server
/// list (which respects admin overrides) replaces this once available.
fn default_harnesses() -> Vec<HarnessAvailability> {
    vec![HarnessAvailability {
        harness: Harness::Oz,
        display_name: harness_display::display_name(Harness::Oz).to_string(),
        enabled: true,
        available_models: vec![],
    }]
}

#[derive(Debug, Clone)]
pub enum AuthSecretFetchState {
    NotFetched,
    Loading,
    Loaded(Vec<AuthSecretEntry>),
    Failed(#[allow(dead_code)] String),
}

#[derive(Debug, Clone)]
pub struct AuthSecretEntry {
    pub name: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct AuthSecretCacheKey {
    team_uid: Option<ServerId>,
    harness: Harness,
}

impl AuthSecretCacheKey {
    fn new(team_scope: &(impl TeamScope + ?Sized), harness: Harness) -> Self {
        Self {
            team_uid: team_scope.team_uid(),
            harness,
        }
    }
}

pub enum HarnessAvailabilityEvent {
    Changed,
    AuthSecretsChanged,
}

pub struct HarnessAvailabilityModel {
    harnesses: Vec<HarnessAvailability>,
    auth_secrets: HashMap<AuthSecretCacheKey, AuthSecretFetchState>,
    auth_secret_retry_after: HashMap<AuthSecretCacheKey, Instant>,
}

impl HarnessAvailabilityModel {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let harnesses = get_cached(ctx).unwrap_or_else(default_harnesses);

        ctx.subscribe_to_model(&NetworkStatus::handle(ctx), |me, _, event, ctx| {
            if let NetworkStatusEvent::NetworkStatusChanged {
                new_status: NetworkStatusKind::Online,
            } = event
            {
                me.refresh(ctx);
            }
        });

        ctx.subscribe_to_model(&AuthManager::handle(ctx), |me, _, event, ctx| {
            if let AuthManagerEvent::AuthComplete = event {
                me.invalidate_auth_secrets();
                me.refresh(ctx);
            }
        });

        ctx.subscribe_to_model(&UserWorkspaces::handle(ctx), |me, _, event, ctx| {
            if let UserWorkspacesEvent::TeamsChanged = event {
                me.invalidate_auth_secrets();
                me.refresh(ctx);
            }
        });

        let me = Self {
            harnesses,
            auth_secrets: HashMap::new(),
            auth_secret_retry_after: HashMap::new(),
        };
        me.refresh(ctx);
        me
    }

    pub fn available_harnesses(&self) -> &[HarnessAvailability] {
        &self.harnesses
    }

    pub fn display_name_for(&self, harness: Harness) -> &str {
        self.harnesses
            .iter()
            .find(|h| h.harness == harness)
            .map(|h| h.display_name.as_str())
            .unwrap_or_else(|| harness_display::display_name(harness))
    }

    /// Whether the harness selector should be shown (>1 known harness, including disabled).
    pub fn should_show_harness_selector(&self) -> bool {
        FeatureFlag::AgentHarness.is_enabled() && self.harnesses.len() > 1
    }

    /// Whether a harness is both known and enabled.
    pub fn is_harness_enabled(&self, harness: Harness) -> bool {
        self.harnesses
            .iter()
            .any(|h| h.harness == harness && h.enabled)
    }

    pub fn models_for(&self, harness: Harness) -> Option<&[HarnessModelInfo]> {
        self.harnesses
            .iter()
            .find(|h| h.harness == harness)
            .map(|h| h.available_models.as_slice())
            .filter(|m| !m.is_empty())
    }

    pub fn auth_secrets_for<S: TeamScope + ?Sized>(
        &self,
        team_scope: &S,
        harness: Harness,
    ) -> &AuthSecretFetchState {
        self.auth_secrets
            .get(&AuthSecretCacheKey::new(team_scope, harness))
            .unwrap_or(&AuthSecretFetchState::NotFetched)
    }
    pub fn ensure_auth_secrets_fetched<S: TeamScope + ?Sized>(
        &mut self,
        team_scope: &S,
        harness: Harness,
        ctx: &mut ModelContext<Self>,
    ) {
        match self.auth_secrets_for(team_scope, harness) {
            AuthSecretFetchState::NotFetched => self.fetch_auth_secrets(team_scope, harness, ctx),
            AuthSecretFetchState::Failed(_)
                if self.can_retry_auth_secret_fetch(team_scope, harness) =>
            {
                self.fetch_auth_secrets(team_scope, harness, ctx);
            }
            AuthSecretFetchState::Failed(_)
            | AuthSecretFetchState::Loading
            | AuthSecretFetchState::Loaded(_) => {}
        }
    }

    fn fetch_auth_secrets<S: TeamScope + ?Sized>(
        &mut self,
        team_scope: &S,
        harness: Harness,
        ctx: &mut ModelContext<Self>,
    ) {
        let Some(agent_harness) = harness_to_graphql_harness(harness) else {
            return;
        };

        if !AuthStateProvider::as_ref(ctx).get().is_logged_in() {
            return;
        }

        let cache_key = AuthSecretCacheKey::new(team_scope, harness);
        let request_team_scope = RequestTeamScope::from_scope(team_scope);
        self.auth_secrets
            .insert(cache_key, AuthSecretFetchState::Loading);
        self.auth_secret_retry_after.remove(&cache_key);

        let api = ServerApiProvider::as_ref(ctx).get_managed_secrets_client();
        ctx.spawn_with_retry_on_error_when(
            move || {
                let api = api.clone();
                let agent_harness = agent_harness.clone();
                async move {
                    api.list_harness_auth_secrets(&request_team_scope, agent_harness)
                        .await
                }
            },
            OUT_OF_BAND_REQUEST_RETRY_STRATEGY,
            is_transient_graphql_or_http_error,
            move |me,
                  result: RequestState<Vec<torch_graphql::managed_secrets::ManagedSecret>>,
                  ctx| {
                match result {
                    RequestState::RequestSucceeded(secrets) => {
                        let entries = secrets
                            .into_iter()
                            .map(|s| AuthSecretEntry { name: s.name })
                            .collect();
                        me.auth_secrets
                            .insert(cache_key, AuthSecretFetchState::Loaded(entries));
                        me.auth_secret_retry_after.remove(&cache_key);
                        ctx.emit(HarnessAvailabilityEvent::AuthSecretsChanged);
                    }
                    RequestState::RequestFailedRetryPending(e) => {
                        log::warn!("Failed to fetch harness auth secrets; retrying: {e:#}");
                    }
                    RequestState::RequestFailed(e) => {
                        let msg = e.to_string();
                        report_error!(e.context("Failed to fetch harness auth secrets"));
                        me.auth_secrets
                            .insert(cache_key, AuthSecretFetchState::Failed(msg));
                        me.auth_secret_retry_after.insert(
                            cache_key,
                            Instant::now() + AUTH_SECRET_FETCH_FAILURE_COOLDOWN,
                        );
                        ctx.emit(HarnessAvailabilityEvent::AuthSecretsChanged);
                    }
                }
            },
        );
    }

    fn invalidate_auth_secrets(&mut self) {
        self.auth_secrets.clear();
        self.auth_secret_retry_after.clear();
    }

    fn can_retry_auth_secret_fetch(
        &self,
        team_scope: &(impl TeamScope + ?Sized),
        harness: Harness,
    ) -> bool {
        self.auth_secret_retry_after
            .get(&AuthSecretCacheKey::new(team_scope, harness))
            .map(|retry_after| Instant::now() >= *retry_after)
            .unwrap_or(true)
    }

    pub fn refresh(&self, ctx: &mut ModelContext<Self>) {
        // The endpoint queries `user`, which requires auth.
        if !AuthStateProvider::as_ref(ctx).get().is_logged_in() {
            return;
        }

        let ai_client = ServerApiProvider::as_ref(ctx).get_ai_client();
        ctx.spawn(
            async move { ai_client.get_available_harnesses().await },
            |me, result, ctx| match result {
                Ok(new_harnesses) => {
                    let new_harnesses = normalize_harness_display_names(new_harnesses);
                    if new_harnesses != me.harnesses {
                        me.harnesses = new_harnesses;
                        me.cache(ctx);
                        // Invalidate cached auth secrets so the next menu open refetches.
                        me.invalidate_auth_secrets();
                        ctx.emit(HarnessAvailabilityEvent::Changed);
                    }
                }
                Err(e) => {
                    report_error!(e.context("Failed to fetch available harnesses"));
                }
            },
        );
    }

    fn cache(&self, ctx: &ModelContext<Self>) {
        if let Ok(serialized) = serde_json::to_string(&self.harnesses)
            && let Err(e) = ctx
                .private_user_preferences()
                .write_value(CACHE_KEY, serialized)
        {
            report_error!(anyhow::anyhow!(e).context("Failed to cache available harnesses"));
        }
    }
}

fn get_cached(ctx: &ModelContext<HarnessAvailabilityModel>) -> Option<Vec<HarnessAvailability>> {
    let raw = ctx
        .private_user_preferences()
        .read_value(CACHE_KEY)
        .ok()??;
    serde_json::from_str::<Vec<HarnessAvailability>>(&raw)
        .ok()
        .map(normalize_harness_display_names)
}

fn normalize_harness_display_names(
    harnesses: Vec<HarnessAvailability>,
) -> Vec<HarnessAvailability> {
    harnesses
        .into_iter()
        .map(|mut harness| {
            if harness.harness == Harness::Oz {
                harness.display_name = harness_display::display_name(Harness::Oz).to_string();
            }
            harness
        })
        .collect()
}

fn harness_to_graphql_harness(harness: Harness) -> Option<torch_graphql::ai::AgentHarness> {
    match harness {
        Harness::Oz => Some(torch_graphql::ai::AgentHarness::Oz),
        Harness::Claude => Some(torch_graphql::ai::AgentHarness::ClaudeCode),
        Harness::Gemini => Some(torch_graphql::ai::AgentHarness::Gemini),
        Harness::Pi => Some(torch_graphql::ai::AgentHarness::Pi),
        Harness::Codex => Some(torch_graphql::ai::AgentHarness::Codex),
        Harness::OpenCode | Harness::Unknown => None,
    }
}

impl Entity for HarnessAvailabilityModel {
    type Event = HarnessAvailabilityEvent;
}

impl SingletonEntity for HarnessAvailabilityModel {}
