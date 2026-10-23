//! Integration tests for file-backed AI execution profiles.

use std::path::PathBuf;

use torch::features::FeatureFlag;
use torch::integration_testing::settings::{
    default_execution_profile, execution_profile, has_multiple_execution_profiles,
};
use torch::integration_testing::terminal::wait_until_bootstrapped_single_pane_for_tab;
use torchui_core::async_assert;

use super::{Builder, new_builder};

const INITIAL_PROFILES: &str = r#"
[agents.execution_profiles.default]
name = "Disk Default"
read_files = "always_allow"
execute_commands = "always_ask"
base_model = "disk-model"
command_allowlist = ["git status"]

[agents.execution_profiles.code-review]
name = "Code Review"
apply_code_diffs = "always_allow"
read_files = "always_ask"
command_denylist = ["rm .*"]
directory_allowlist = ["/repo"]
web_search_enabled = true
"#;

fn settings_file_path() -> PathBuf {
    torch::settings::user_preferences_toml_file_path()
}

fn write_settings_file(contents: &str) {
    let path = settings_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("should create settings directory");
    }
    std::fs::write(path, contents).expect("should write settings file");
}

fn enable_file_backed_profiles() {
    FeatureFlag::SettingsFile.set_enabled(true);
    FeatureFlag::FileBackedExecutionProfiles.set_enabled(true);
}

pub fn test_execution_profiles_load_from_settings_file() -> Builder {
    enable_file_backed_profiles();

    new_builder()
        .with_setup(|_utils| write_settings_file(INITIAL_PROFILES))
        .with_step(
            wait_until_bootstrapped_single_pane_for_tab(0).add_named_assertion(
                "Execution profile collection should be loaded from settings.toml",
                |app, _window_id| {
                    let default = default_execution_profile(app);
                    let review = execution_profile(app, "code-review");

                    async_assert!(
                        default.name == "Disk Default"
                            && default.read_files_always_allow
                            && default.execute_commands_always_ask
                            && default.base_model.as_deref() == Some("disk-model")
                            && default.command_allowlist == ["git status"]
                            && review.as_ref().is_some_and(|profile| {
                                profile.name == "Code Review"
                                    && profile.apply_code_diffs_always_allow
                                    && profile.directory_allowlist == [PathBuf::from("/repo")]
                                    && profile.web_search_enabled
                            })
                            && has_multiple_execution_profiles(app),
                        "Expected the execution profile model to expose the complete collection loaded from settings.toml"
                    )
                },
            ),
        )
}
