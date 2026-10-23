//! Frontend-neutral validation predicates for orchestration edit flows.

use ai::agent::action::RunAgentsExecutionMode;
use torch_cli::agent::Harness;
use torchui::{AppContext, SingletonEntity as _};

use super::config_state::{AuthSecretSelection, OrchestrationConfigState};
use crate::ai::auth_secret_types::auth_secret_types_for_harness;
use crate::ai::harness_availability::HarnessAvailabilityModel;
use crate::ai::local_harness_setup::{LocalHarnessSetupState, local_harness_setup_state};
use crate::ai::orchestration::providers::ORCHESTRATION_WARP_WORKER_HOST;

/// Returns `true` when the auth secret picker should be visible: Cloud +
/// non-Oz + a harness with at least one supported auth-secret type. Local
/// non-Oz children inherit auth from the user's shell environment.
pub fn should_show_auth_secret_picker(state: &OrchestrationConfigState) -> bool {
    if !state.execution_mode.is_remote() {
        return false;
    }
    let Some(harness) = Harness::parse_orchestration_harness(&state.harness_type) else {
        return false;
    };
    if harness == Harness::Oz {
        return false;
    }
    !auth_secret_types_for_harness(harness).is_empty()
}

/// `true` when the user must pick an API key (or Inherit) before Accept is
/// allowed. Fires on `Unset` for any non-Oz cloud harness with managed-secret
/// types, regardless of fetch state — dispatching with an unintended
/// `Inherit` while secrets are still loading would fail downstream.
pub fn auth_secret_selection_required(state: &OrchestrationConfigState, _ctx: &AppContext) -> bool {
    if !should_show_auth_secret_picker(state) {
        return false;
    }
    if !matches!(
        state.auth_secret_selection,
        AuthSecretSelection::Unset | AuthSecretSelection::CreatingNew
    ) {
        return false;
    }
    let Some(harness) = Harness::parse_orchestration_harness(&state.harness_type) else {
        return false;
    };
    if harness == Harness::Oz || auth_secret_types_for_harness(harness).is_empty() {
        return false;
    }
    true
}

/// [`OrchestrationConfigState::accept_disabled_reason`] plus the
/// auth-secret-selection gate. Card views should prefer this.
pub fn accept_disabled_reason_with_auth(
    state: &OrchestrationConfigState,
    ctx: &AppContext,
) -> Option<String> {
    if Harness::parse_orchestration_harness(&state.harness_type) == Some(Harness::Pi) {
        if !state.execution_mode.is_remote() {
            return Some("Pi child agents require Cloud execution.".to_string());
        }
        if !HarnessAvailabilityModel::as_ref(ctx).is_harness_enabled(Harness::Pi) {
            return Some("The pi harness is not available.".to_string());
        }
    }
    if let Some(reason) = state.accept_disabled_reason() {
        return Some(reason.to_string());
    }
    if matches!(state.execution_mode, RunAgentsExecutionMode::Local)
        && let Some(harness) = Harness::parse_local_child_harness(&state.harness_type)
    {
        match local_harness_setup_state(harness) {
            LocalHarnessSetupState::MissingHarness { tooltip } => {
                return Some(tooltip.to_string());
            }
            LocalHarnessSetupState::ProductDisabled { message } => {
                return Some(message.to_string());
            }
            LocalHarnessSetupState::Ready => {}
        }
    }
    if auth_secret_selection_required(state, ctx) {
        return Some("Select an API key for this harness to continue.".to_string());
    }
    None
}

/// Soft recommendation copy shown when a Torch-hosted Cloud run has no
/// environment selected. `None` when not applicable.
pub fn empty_env_recommendation_message(
    execution_mode: &RunAgentsExecutionMode,
    has_visible_environment: bool,
) -> Option<String> {
    let RunAgentsExecutionMode::Remote {
        environment_id,
        worker_host,
        ..
    } = execution_mode
    else {
        return None;
    };
    if !environment_id.trim().is_empty() {
        return None;
    }
    if !worker_host.eq_ignore_ascii_case(ORCHESTRATION_WARP_WORKER_HOST) {
        return None;
    }
    Some(if has_visible_environment {
        "We recommend selecting an environment for cloud agents.".to_string()
    } else {
        "We recommend creating an environment for cloud agents.".to_string()
    })
}
