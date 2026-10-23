//! Frontend-neutral orchestration domain: edit state, transitions,
//! validation, and catalog providers shared by the GUI orchestration
//! controls and the TUI orchestration card.
//!
//! Nothing in this module may depend on `torchui::elements` or any other
//! GUI rendering types; it only reads/writes app singletons through
//! `AppContext`.

mod config_state;
mod edit_state;
mod providers;
mod remote_child;
mod snapshots;
mod validation;

pub use config_state::{AuthSecretSelection, OrchestrationConfigState};
pub use edit_state::OrchestrationEditState;
pub use providers::{
    ORCHESTRATION_WARP_WORKER_HOST, persist_environment_selection, persist_host_selection,
    resolve_auth_secret_selection_for_harness, resolve_default_environment_id,
    resolve_default_host_slug,
};
pub(crate) use providers::{
    can_execute_with_auth_secret, persist_auth_secret_selection,
    populate_default_auth_secret_for_execution,
};
pub(crate) use remote_child::should_disable_snapshot;
pub use remote_child::{
    CloudAgentStartupAuthFlow, CloudAgentStartupBlocker, CloudAgentStartupFailure,
    CloudAgentStartupIssue, CloudAgentStartupPresentation, RemoteChildLaunchConfig,
    classify_cloud_agent_startup_error, prepare_remote_child_launch,
};
pub(crate) use snapshots::AUTH_SECRET_INHERIT_LABEL;
pub use snapshots::{
    OptionBadge, OptionFooter, OptionRow, OptionSnapshot, OptionSourceStatus, api_key_snapshot,
    build_runner_snapshot, environment_snapshot, harness_snapshot, host_snapshot, model_snapshot,
};
pub use validation::{
    accept_disabled_reason_with_auth, empty_env_recommendation_message,
    should_show_auth_secret_picker,
};
