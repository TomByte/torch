use std::path::PathBuf;

use torchui::{App, SingletonEntity};

use crate::ai::execution_profiles::profiles::AIExecutionProfilesModel;
use crate::ai::execution_profiles::{AIExecutionProfile, ActionPermission, ExecutionProfileId};

#[derive(Debug)]
pub struct ExecutionProfileSnapshot {
    pub name: String,
    pub apply_code_diffs_always_allow: bool,
    pub read_files_always_allow: bool,
    pub execute_commands_always_ask: bool,
    pub base_model: Option<String>,
    pub command_allowlist: Vec<String>,
    pub directory_allowlist: Vec<PathBuf>,
    pub web_search_enabled: bool,
}

fn snapshot(profile: &AIExecutionProfile) -> ExecutionProfileSnapshot {
    ExecutionProfileSnapshot {
        name: profile.name.clone(),
        apply_code_diffs_always_allow: profile.apply_code_diffs == ActionPermission::AlwaysAllow,
        read_files_always_allow: profile.read_files == ActionPermission::AlwaysAllow,
        execute_commands_always_ask: profile.execute_commands == ActionPermission::AlwaysAsk,
        base_model: profile.base_model.as_ref().map(ToString::to_string),
        command_allowlist: profile
            .command_allowlist
            .iter()
            .map(ToString::to_string)
            .collect(),
        directory_allowlist: profile.directory_allowlist.clone(),
        web_search_enabled: profile.web_search_enabled,
    }
}

pub fn default_execution_profile(app: &App) -> ExecutionProfileSnapshot {
    app.read(|ctx| {
        snapshot(
            AIExecutionProfilesModel::as_ref(ctx)
                .default_profile(ctx)
                .data(),
        )
    })
}

pub fn execution_profile(app: &App, profile_id: &str) -> Option<ExecutionProfileSnapshot> {
    let profile_id = ExecutionProfileId::parse(profile_id)?;
    app.read(|ctx| {
        AIExecutionProfilesModel::as_ref(ctx)
            .get_profile_by_id(&profile_id, ctx)
            .map(|profile| snapshot(profile.data()))
    })
}

pub fn has_multiple_execution_profiles(app: &App) -> bool {
    app.read(|ctx| AIExecutionProfilesModel::as_ref(ctx).has_multiple_profiles())
}
