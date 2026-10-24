use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
#[cfg(feature = "local_fs")]
use repo_metadata::DirectoryWatcher;
use settings::Setting as _;
use torch_completer::completer::{CommandExitStatus, CommandOutput};
use torch_core::command::ExitCode;
use torchui::{App, SingletonEntity};
use torchui_extras::user_preferences;

use super::{ActiveChipSurfaces, ChipUpdateStatus, CurrentPrompt, PromptContext};
use crate::CLIAgentSessionsModel;
use crate::agent_status::footer::toolbar_item::AgentToolbarItemKind;
use crate::auth::AuthStateProvider;
use crate::auth::auth_manager::AuthManager;
#[cfg(feature = "local_fs")]
use crate::code_review::diff_state::DiffStats;
#[cfg(feature = "local_fs")]
use crate::code_review::git_repo_model::{GitRepoStatusModel, GitStatusMetadata};
#[cfg(feature = "local_fs")]
use crate::code_review::github_repo_model::GitHubRepoModel;
use crate::context_chips::context_chip::{Environment, PromptGenerator};
#[cfg(feature = "local_fs")]
use crate::context_chips::display_chip::GitBranchTrackingStatus;
use crate::context_chips::prompt::Prompt;
use crate::context_chips::{ChipAvailability, ChipDisabledReason, ContextChipKind};
use crate::features::FeatureFlag;
use crate::server::server_api::ServerApiProvider;
use crate::server::telemetry::context_provider::AppTelemetryContextProvider;
use crate::settings::WarpPromptSeparator;
#[cfg(windows)]
use crate::system::SystemInfo;
use crate::terminal::cli_agent_sessions::{
    CLIAgentInputState, CLIAgentSession, CLIAgentSessionContext, CLIAgentSessionStatus,
};
use crate::terminal::model::block::BlockMetadata;
use crate::terminal::model::session::{
    CommandExecutor, ExecuteCommandOptions, SessionId, SessionInfo, Sessions,
};
use crate::terminal::session_settings::{
    CLIAgentToolbarChipSelection, SessionSettings, ToolbarChipSelection,
};
use crate::terminal::shell::Shell;
use crate::terminal::{CLIAgent, History};
#[cfg(feature = "local_fs")]
use crate::util::git::PrInfo;

#[cfg(feature = "local_fs")]
fn git_status_metadata(branch: &str) -> GitStatusMetadata {
    GitStatusMetadata {
        current_branch_name: branch.to_string(),
        main_branch_name: "main".to_string(),
        stats_against_head: DiffStats::default(),
        branch_tracking_status: GitBranchTrackingStatus::new(branch.to_string(), None, 0, 0),
    }
}

/// Registers the models a [`CurrentPrompt`] needs to show the CLI agent footer.
fn add_cli_agent_footer_singletons(app: &mut App) {
    app.add_singleton_model(|_| Prompt::mock());
    app.add_singleton_model(SessionSettings::new_with_defaults);
    app.add_singleton_model(|_| History::new(vec![]));
    app.add_singleton_model(|_ctx| {
        settings::PublicPreferences::new(
            Box::<user_preferences::in_memory::InMemoryPreferences>::default(),
        )
    });
    app.add_singleton_model(|_| {
        settings::PrivatePreferences::new(
            Box::<user_preferences::in_memory::InMemoryPreferences>::default(),
        )
    });
    app.add_singleton_model(|_| ServerApiProvider::new_for_test());
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());
    app.add_singleton_model(AppTelemetryContextProvider::new_context_provider);
    app.add_singleton_model(AuthManager::new_for_test);
    app.add_singleton_model(|_| crate::settings::manager::SettingsManager::default());
    crate::settings::InputSettings::register(app);
    app.update(crate::settings::CLIAgentSettings::register_and_subscribe_to_events);
    app.add_singleton_model(crate::workspaces::user_workspaces::UserWorkspaces::default_mock);
    app.add_singleton_model(|_| CLIAgentSessionsModel::new());
    #[cfg(windows)]
    app.add_singleton_model(SystemInfo::new);
}

fn cli_agent_session(agent: CLIAgent) -> CLIAgentSession {
    CLIAgentSession {
        agent,
        status: CLIAgentSessionStatus::InProgress,
        session_context: CLIAgentSessionContext::default(),
        input_state: CLIAgentInputState::Closed,
        should_auto_toggle_input: false,
        listener: None,
        plugin_version: None,
        remote_host: None,
        draft_text: None,
        custom_command_prefix: None,
        received_rich_notification: false,
    }
}

/// Shows the CLI agent footer, holding `chips`, for `current_prompt`'s terminal, so that the
/// prompt runs those chips.
fn show_cli_agent_footer(
    app: &mut App,
    current_prompt: &torchui::ModelHandle<CurrentPrompt>,
    chips: impl IntoIterator<Item = ContextChipKind>,
) {
    let left = chips
        .into_iter()
        .map(AgentToolbarItemKind::ContextChip)
        .collect();
    SessionSettings::handle(app).update(app, |settings, ctx| {
        settings
            .cli_agent_footer_chip_selection
            .set_value(
                CLIAgentToolbarChipSelection::Custom {
                    left,
                    right: vec![],
                },
                ctx,
            )
            .unwrap();
    });
    let terminal_view_id = current_prompt.id();
    current_prompt.update(app, |current_prompt, _| {
        current_prompt.terminal_view_id = Some(terminal_view_id);
    });
    CLIAgentSessionsModel::handle(app).update(app, |sessions, ctx| {
        sessions.set_session(terminal_view_id, cli_agent_session(CLIAgent::Claude), ctx);
    });
}

#[test]
fn test_fingerprint_skips_contextual_chip_recompute_when_context_is_unchanged() {
    App::test((), |mut app| async move {
        let session_id = SessionId::from(777);
        add_cli_agent_footer_singletons(&mut app);

        let sessions = app.add_model(|_| Sessions::new_for_test());
        let current_prompt = app.add_model(move |ctx| CurrentPrompt::new(sessions, ctx));
        show_cli_agent_footer(
            &mut app,
            &current_prompt,
            [ContextChipKind::WorkingDirectory],
        );

        current_prompt.update(&mut app, |current_prompt, ctx| {
            current_prompt.latest_context = Some(PromptContext {
                active_block_metadata: BlockMetadata::new(
                    Some(session_id),
                    Some("/tmp/project".to_string()),
                ),
                environment: Environment::default(),
            });
            current_prompt.update_states_with_new_context(ctx);

            let state = current_prompt
                .states
                .get(&ContextChipKind::WorkingDirectory)
                .expect("expected working directory state");
            assert_eq!(state.update_status, ChipUpdateStatus::Ready);
            assert!(state.last_fingerprint.is_some());
        });

        current_prompt.update(&mut app, |current_prompt, ctx| {
            current_prompt.update_states_with_new_context(ctx);

            let state = current_prompt
                .states
                .get(&ContextChipKind::WorkingDirectory)
                .expect("expected working directory state");
            assert_eq!(state.update_status, ChipUpdateStatus::Cached);
            assert!(matches!(
                state.last_computed_value.as_ref().and_then(|v| v.as_text()),
                Some("/tmp/project")
            ));
        });
    });
}

#[test]
fn test_shell_chip_is_disabled_when_required_executable_is_missing() {
    App::test((), |mut app| async move {
        let session_id = SessionId::from(456);
        add_cli_agent_footer_singletons(&mut app);

        let executor = Arc::new(RecordingCommandExecutor::default());
        let sessions = app.add_model(|ctx| {
            let mut sessions = Sessions::new_for_test().with_command_executor(executor.clone());
            sessions.initialize_bootstrapped_session(
                SessionInfo::new_for_test().with_id(session_id),
                "test command".to_string(),
                vec![],
                None,
                ctx,
            );
            sessions
        });
        let sessions_for_prompt = sessions.clone();
        let current_prompt =
            app.add_model(move |ctx| CurrentPrompt::new(sessions_for_prompt.clone(), ctx));
        show_cli_agent_footer(&mut app, &current_prompt, [ContextChipKind::ShellGitBranch]);

        let session = app
            .read(|ctx| sessions.as_ref(ctx).get(session_id))
            .expect("session should exist");
        session.load_external_commands().await;
        executor.clear();

        current_prompt.update(&mut app, |current_prompt, ctx| {
            current_prompt.latest_context = Some(PromptContext {
                active_block_metadata: BlockMetadata::new(
                    Some(session_id),
                    Some("/tmp/project".to_string()),
                ),
                environment: Environment::default(),
            });
            current_prompt.update_states_with_new_context(ctx);

            let state = current_prompt
                .states
                .get(&ContextChipKind::ShellGitBranch)
                .expect("expected git branch state");
            assert_eq!(
                state.availability,
                ChipAvailability::Disabled(ChipDisabledReason::RequiresExecutable {
                    command: "git".to_string(),
                })
            );
            assert_eq!(state.update_status, ChipUpdateStatus::Disabled);
            assert!(state.generator_handle.is_none());
            assert!(state.on_click_generator_handle.is_none());
        });

        assert!(executor.commands.lock().is_empty());
    });
}

#[test]
fn test_github_pr_chip_runtime_policy_configuration() {
    let _flag_guard = FeatureFlag::GithubPrPromptChip.override_enabled(true);
    let chip = ContextChipKind::GithubPullRequest
        .to_chip()
        .expect("github pr chip should exist");
    let policy = chip.runtime_policy();

    assert!(matches!(
        chip.generator(),
        PromptGenerator::Contextual { .. }
    ));
    assert!(policy.required_executables().is_empty());
    assert_eq!(policy.shell_command_timeout(), None);
    assert!(!policy.suppress_on_failure());
    assert!(policy.fingerprint_inputs().is_empty());
    assert!(policy.invalidate_on_commands().is_empty());
}

#[test]
fn test_invalidating_command_count_unaffected_for_chips_without_invalidate_on_commands() {
    App::test((), |mut app| async move {
        let session_id = SessionId::from(888);
        add_cli_agent_footer_singletons(&mut app);

        let sessions = app.add_model(|_| Sessions::new_for_test());
        let current_prompt = app.add_model(move |ctx| CurrentPrompt::new(sessions, ctx));
        show_cli_agent_footer(
            &mut app,
            &current_prompt,
            [ContextChipKind::WorkingDirectory],
        );

        current_prompt.update(&mut app, |current_prompt, ctx| {
            current_prompt.latest_context = Some(PromptContext {
                active_block_metadata: BlockMetadata::new(
                    Some(session_id),
                    Some("/tmp/project".to_string()),
                ),
                environment: Environment::default(),
            });
            current_prompt.update_states_with_new_context(ctx);

            // WorkingDirectory has no invalidate_on_commands, so the counter should be 0.
            let state = current_prompt
                .states
                .get(&ContextChipKind::WorkingDirectory)
                .expect("expected working directory state");
            assert_eq!(state.invalidating_command_count, 0);
        });
    });
}

#[test]
fn test_chips_to_run_only_includes_active_surface_configurations() {
    App::test((), |mut app| async move {
        app.add_singleton_model(|_| {
            Prompt::mock_with(
                [ContextChipKind::Username],
                false,
                WarpPromptSeparator::None,
            )
        });
        app.add_singleton_model(SessionSettings::new_with_defaults);
        app.add_singleton_model(|_ctx| {
            settings::PublicPreferences::new(
                Box::<user_preferences::in_memory::InMemoryPreferences>::default(),
            )
        });
        app.add_singleton_model(|_| {
            settings::PrivatePreferences::new(
                Box::<user_preferences::in_memory::InMemoryPreferences>::default(),
            )
        });
        app.update(|ctx| {
            SessionSettings::handle(ctx).update(ctx, |settings, ctx| {
                settings
                    .cli_agent_footer_chip_selection
                    .set_value(
                        CLIAgentToolbarChipSelection::Custom {
                            left: vec![AgentToolbarItemKind::ContextChip(
                                ContextChipKind::ShellGitBranch,
                            )],
                            right: vec![],
                        },
                        ctx,
                    )
                    .unwrap();
            });
        });

        let sessions = app.add_model(|_| Sessions::new_for_test());
        let current_prompt = app.add_model(move |ctx| CurrentPrompt::new(sessions, ctx));

        app.read(|ctx| {
            let current_prompt = current_prompt.as_ref(ctx);
            let chips_for = |surfaces| current_prompt.chips_to_run_for_surfaces(surfaces, ctx);

            assert!(chips_for(ActiveChipSurfaces::default()).is_empty());
            assert_eq!(
                chips_for(ActiveChipSurfaces {
                    cli_agent_footer: true,
                }),
                vec![ContextChipKind::ShellGitBranch]
            );
        });
    });
}

#[test]
fn test_cli_agent_footer_chips_require_a_visible_supported_footer() {
    App::test((), |mut app| async move {
        app.add_singleton_model(|_| Prompt::mock());
        app.add_singleton_model(SessionSettings::new_with_defaults);
        app.add_singleton_model(|_| History::new(vec![]));
        app.add_singleton_model(|_ctx| {
            settings::PublicPreferences::new(
                Box::<user_preferences::in_memory::InMemoryPreferences>::default(),
            )
        });
        app.add_singleton_model(|_| {
            settings::PrivatePreferences::new(
                Box::<user_preferences::in_memory::InMemoryPreferences>::default(),
            )
        });
        app.add_singleton_model(|_| ServerApiProvider::new_for_test());
        app.add_singleton_model(|_| AuthStateProvider::new_for_test());
        app.add_singleton_model(AppTelemetryContextProvider::new_context_provider);
        app.add_singleton_model(AuthManager::new_for_test);
        app.add_singleton_model(|_| crate::settings::manager::SettingsManager::default());
        crate::settings::InputSettings::register(&mut app);
        app.update(crate::settings::CLIAgentSettings::register_and_subscribe_to_events);
        app.add_singleton_model(crate::workspaces::user_workspaces::UserWorkspaces::default_mock);
        app.add_singleton_model(|_| CLIAgentSessionsModel::new());
        #[cfg(windows)]
        app.add_singleton_model(SystemInfo::new);

        let sessions = app.add_model(|_| Sessions::new_for_test());
        let current_prompt = app.add_model(move |ctx| CurrentPrompt::new(sessions, ctx));
        let terminal_view_id = current_prompt.id();
        current_prompt.update(&mut app, |current_prompt, _| {
            current_prompt.terminal_view_id = Some(terminal_view_id);
        });

        let cli_footer_chips =
            |app: &App| app.read(|ctx| current_prompt.as_ref(ctx).chips_to_run(ctx));
        assert!(cli_footer_chips(&app).is_empty());

        let session_for = |agent| CLIAgentSession {
            agent,
            status: CLIAgentSessionStatus::InProgress,
            session_context: CLIAgentSessionContext::default(),
            input_state: CLIAgentInputState::Closed,
            should_auto_toggle_input: false,
            listener: None,
            plugin_version: None,
            remote_host: None,
            draft_text: None,
            custom_command_prefix: None,
            received_rich_notification: false,
        };

        CLIAgentSessionsModel::handle(&app).update(&mut app, |sessions, ctx| {
            sessions.set_session(terminal_view_id, session_for(CLIAgent::Claude), ctx);
        });
        assert_eq!(
            cli_footer_chips(&app),
            app.read(|ctx| {
                SessionSettings::as_ref(ctx)
                    .cli_agent_footer_chip_selection
                    .all_chips()
            })
        );

        CLIAgentSessionsModel::handle(&app).update(&mut app, |sessions, ctx| {
            sessions.set_session(terminal_view_id, session_for(CLIAgent::Claude), ctx);
        });
        crate::settings::CLIAgentSettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .should_render_cli_agent_footer
                .set_value(false, ctx)
                .unwrap();
        });
        assert!(cli_footer_chips(&app).is_empty());
    });
}

#[test]
fn test_ps1_without_active_agent_surface_runs_no_footer_generators() {
    let _flag_guard = FeatureFlag::AgentView.override_enabled(true);
    App::test((), |mut app| async move {
        let session_id = SessionId::from(321);
        app.add_singleton_model(|_| Prompt::mock());
        app.add_singleton_model(SessionSettings::new_with_defaults);
        app.add_singleton_model(|_| History::new(vec![]));
        app.add_singleton_model(|_ctx| {
            settings::PublicPreferences::new(
                Box::<user_preferences::in_memory::InMemoryPreferences>::default(),
            )
        });
        app.add_singleton_model(|_| {
            settings::PrivatePreferences::new(
                Box::<user_preferences::in_memory::InMemoryPreferences>::default(),
            )
        });
        app.update(|ctx| {
            SessionSettings::handle(ctx).update(ctx, |settings, ctx| {
                settings.honor_ps1.set_value(true, ctx).unwrap();
            });
        });
        app.add_singleton_model(|_| ServerApiProvider::new_for_test());
        app.add_singleton_model(|_| AuthStateProvider::new_for_test());
        app.add_singleton_model(AppTelemetryContextProvider::new_context_provider);
        app.add_singleton_model(AuthManager::new_for_test);
        app.add_singleton_model(|_| crate::settings::manager::SettingsManager::default());
        crate::settings::InputSettings::register(&mut app);
        app.update(crate::settings::CLIAgentSettings::register_and_subscribe_to_events);
        app.add_singleton_model(crate::workspaces::user_workspaces::UserWorkspaces::default_mock);
        #[cfg(windows)]
        app.add_singleton_model(SystemInfo::new);

        let executor = Arc::new(RecordingCommandExecutor::default());
        let sessions = app.add_model(|ctx| {
            let mut sessions = Sessions::new_for_test().with_command_executor(executor.clone());
            sessions.initialize_bootstrapped_session(
                SessionInfo::new_for_test().with_id(session_id),
                "test command".to_string(),
                vec![],
                None,
                ctx,
            );
            sessions
        });
        let current_prompt = app.add_model(move |ctx| CurrentPrompt::new(sessions, ctx));

        current_prompt
            .update(&mut app, |current_prompt, ctx| {
                current_prompt.latest_context = Some(PromptContext {
                    active_block_metadata: BlockMetadata::new(Some(session_id), None),
                    environment: Environment::default(),
                });
                current_prompt.update_states_with_new_context(ctx);
                current_prompt.await_generators(ctx)
            })
            .await;

        assert!(executor.commands.lock().is_empty());
        app.read(|ctx| {
            assert!(current_prompt.as_ref(ctx).states.is_empty());
        });
    });
}
#[cfg(feature = "local_fs")]
#[test]
fn test_externally_driven_chip_skips_periodic_timer() {
    App::test((), |mut app| async move {
        add_cli_agent_footer_singletons(&mut app);

        let temp_dir = tempfile::TempDir::new().unwrap();
        let watcher_handle = app.add_singleton_model(DirectoryWatcher::new_for_testing);
        let repo_handle = watcher_handle.update(&mut app, |watcher, ctx| {
            watcher
                .add_directory(
                    torch_util::standardized_path::StandardizedPath::from_local_canonicalized(
                        temp_dir.path(),
                    )
                    .unwrap(),
                    ctx,
                )
                .unwrap()
        });
        let git_status = app
            .add_model(move |ctx| GitRepoStatusModel::new_local_for_test(repo_handle, None, ctx));

        let sessions = app.add_model(|_| Sessions::new_for_test());
        let current_prompt = app.add_model(move |ctx| CurrentPrompt::new(sessions, ctx));
        show_cli_agent_footer(&mut app, &current_prompt, [ContextChipKind::ShellGitBranch]);

        current_prompt.update(&mut app, |cp, ctx| {
            cp.set_git_repo_status(Some(git_status.downgrade()), ctx);
            cp.update_states_with_new_context(ctx);
        });

        app.read(|ctx| {
            let cp = current_prompt.as_ref(ctx);
            let state = cp
                .states
                .get(&ContextChipKind::ShellGitBranch)
                .expect("ShellGitBranch state should exist after set_git_repo_status");
            assert!(
                state.refresh_handle.is_none(),
                "Externally-driven chip should not have a periodic refresh handle"
            );
        });
    });
}

#[cfg(feature = "local_fs")]
#[test]
fn test_git_status_change_updates_chip_value() {
    App::test((), |mut app| async move {
        add_cli_agent_footer_singletons(&mut app);

        let temp_dir = tempfile::TempDir::new().unwrap();
        let watcher_handle = app.add_singleton_model(DirectoryWatcher::new_for_testing);
        let repo_handle = watcher_handle.update(&mut app, |watcher, ctx| {
            watcher
                .add_directory(
                    torch_util::standardized_path::StandardizedPath::from_local_canonicalized(
                        temp_dir.path(),
                    )
                    .unwrap(),
                    ctx,
                )
                .unwrap()
        });

        let initial_metadata = git_status_metadata("main");
        let git_status = app.add_model(move |ctx| {
            GitRepoStatusModel::new_local_for_test(repo_handle, Some(initial_metadata), ctx)
        });

        let sessions = app.add_model(|_| Sessions::new_for_test());
        let current_prompt = app.add_model(move |ctx| CurrentPrompt::new(sessions, ctx));
        show_cli_agent_footer(&mut app, &current_prompt, [ContextChipKind::ShellGitBranch]);

        // Subscribe to the git status model and run chips.
        current_prompt.update(&mut app, |cp, ctx| {
            cp.set_git_repo_status(Some(git_status.downgrade()), ctx);
            cp.update_states_with_new_context(ctx);
        });

        // Simulate a branch change by updating the model's metadata.
        git_status.update(&mut app, |model, ctx| {
            model.set_metadata_for_test(Some(git_status_metadata("feature-branch")), ctx);
        });

        app.read(|ctx| {
            let value = current_prompt
                .as_ref(ctx)
                .latest_chip_value(&ContextChipKind::ShellGitBranch);
            assert_eq!(
                value,
                Some(&crate::context_chips::ChipValue::Text(
                    "feature-branch".to_string(),
                )),
                "Chip value should reflect the new branch name after metadata change"
            );
        });
    });
}

#[cfg(feature = "local_fs")]
#[test]
fn test_git_status_change_updates_branch_status_chip_value() {
    App::test((), |mut app| async move {
        add_cli_agent_footer_singletons(&mut app);

        let temp_dir = tempfile::TempDir::new().unwrap();
        let watcher_handle = app.add_singleton_model(DirectoryWatcher::new_for_testing);
        let repo_handle = watcher_handle.update(&mut app, |watcher, ctx| {
            watcher
                .add_directory(
                    torch_util::standardized_path::StandardizedPath::from_local_canonicalized(
                        temp_dir.path(),
                    )
                    .unwrap(),
                    ctx,
                )
                .unwrap()
        });

        let git_status = app
            .add_model(move |ctx| GitRepoStatusModel::new_local_for_test(repo_handle, None, ctx));
        let sessions = app.add_model(|_| Sessions::new_for_test());
        let current_prompt = app.add_model(move |ctx| CurrentPrompt::new(sessions, ctx));
        show_cli_agent_footer(
            &mut app,
            &current_prompt,
            [ContextChipKind::GitBranchStatus],
        );

        current_prompt.update(&mut app, |cp, ctx| {
            cp.set_git_repo_status(Some(git_status.downgrade()), ctx);
            cp.update_states_with_new_context(ctx);
        });

        let branch_tracking_status = GitBranchTrackingStatus::new(
            "feature-branch".to_string(),
            Some("origin/feature-branch".to_string()),
            3,
            1,
        );
        git_status.update(&mut app, |model, ctx| {
            model.set_metadata_for_test(
                Some(GitStatusMetadata {
                    current_branch_name: "feature-branch".to_string(),
                    main_branch_name: "main".to_string(),
                    stats_against_head: DiffStats::default(),
                    branch_tracking_status: branch_tracking_status.clone(),
                }),
                ctx,
            );
        });

        app.read(|ctx| {
            let value = current_prompt
                .as_ref(ctx)
                .latest_chip_value(&ContextChipKind::GitBranchStatus);
            assert_eq!(
                value,
                Some(&crate::context_chips::ChipValue::GitBranchStatus(
                    branch_tracking_status,
                )),
                "Branch status chip should reflect ahead and behind metadata"
            );
        });
    });
}

#[cfg(feature = "local_fs")]
#[test]
fn test_git_status_pr_info_updates_github_pr_chip_value() {
    let _flag_guard = FeatureFlag::GithubPrPromptChip.override_enabled(true);
    App::test((), |mut app| async move {
        add_cli_agent_footer_singletons(&mut app);

        let temp_dir = tempfile::TempDir::new().unwrap();
        let watcher_handle = app.add_singleton_model(DirectoryWatcher::new_for_testing);
        let repo_handle = watcher_handle.update(&mut app, |watcher, ctx| {
            watcher
                .add_directory(
                    torch_util::standardized_path::StandardizedPath::from_local_canonicalized(
                        temp_dir.path(),
                    )
                    .unwrap(),
                    ctx,
                )
                .unwrap()
        });

        let git_status = app.add_model(move |ctx| {
            GitRepoStatusModel::new_local_for_test(
                repo_handle,
                Some(git_status_metadata("feature-a")),
                ctx,
            )
        });
        let github_repo_model = {
            let git_status = git_status.clone();
            app.add_model(move |ctx| GitHubRepoModel::new_local_for_test(git_status, ctx))
        };

        let sessions = app.add_model(|_| Sessions::new_for_test());
        let current_prompt = app.add_model(move |ctx| CurrentPrompt::new(sessions, ctx));
        show_cli_agent_footer(
            &mut app,
            &current_prompt,
            [ContextChipKind::GithubPullRequest],
        );

        current_prompt.update(&mut app, |cp, ctx| {
            cp.set_git_repo_status(Some(git_status.downgrade()), ctx);
            cp.set_github_repo_model(Some(github_repo_model.downgrade()), ctx);
            cp.update_states_with_new_context(ctx);
        });

        github_repo_model.update(&mut app, |model, ctx| {
            model.set_pr_info_for_test(
                Some(PrInfo {
                    number: 123,
                    url: "https://github.com/warp/warp/pull/123".to_string(),
                    state: "OPEN".to_string(),
                    draft: true,
                    base_branch: "main".to_string(),
                }),
                ctx,
            );
        });

        app.read(|ctx| {
            let value = current_prompt
                .as_ref(ctx)
                .latest_chip_value(&ContextChipKind::GithubPullRequest);
            assert_eq!(
                value,
                Some(&crate::context_chips::ChipValue::Text(
                    "https://github.com/warp/warp/pull/123".to_string(),
                )),
            );
        });

        // Clearing the model's PR info propagates to the chip.
        github_repo_model.update(&mut app, |model, ctx| {
            model.set_pr_info_for_test(None, ctx);
        });

        app.read(|ctx| {
            let value = current_prompt
                .as_ref(ctx)
                .latest_chip_value(&ContextChipKind::GithubPullRequest);
            assert_eq!(
                value, None,
                "PR chip should clear when the model's cached PR info is cleared"
            );
        });

        github_repo_model.update(&mut app, |model, ctx| {
            model.set_pr_info_for_test(
                Some(PrInfo {
                    number: 456,
                    url: "https://github.com/warp/warp/pull/456".to_string(),
                    state: "OPEN".to_string(),
                    draft: false,
                    base_branch: "main".to_string(),
                }),
                ctx,
            );
        });

        current_prompt.update(&mut app, |cp, ctx| {
            {
                let state = cp
                    .states
                    .get_mut(&ContextChipKind::GithubPullRequest)
                    .expect("expected github PR chip state");
                state.last_on_click_values = Some(vec!["stale".to_string()]);
                state.last_fingerprint = Some(123);
                state.last_failure_fingerprint = Some(456);
                state.update_status = ChipUpdateStatus::Cached;
                state.generator_handle = Some(ctx.spawn(
                    async {
                        async_io::Timer::after(std::time::Duration::from_secs(60)).await;
                    },
                    |_, _, _| {},
                ));
            }

            cp.set_github_repo_model(None, ctx);

            let state = cp
                .states
                .get(&ContextChipKind::GithubPullRequest)
                .expect("expected github PR chip state");
            assert!(state.last_computed_value.is_none());
            assert!(state.last_on_click_values.is_none());
            assert!(state.last_fingerprint.is_none());
            assert!(state.last_failure_fingerprint.is_none());
            assert_eq!(state.update_status, ChipUpdateStatus::Idle);
            assert!(state.generator_handle.is_none());
        });
    });
}

/// A [`CommandExecutor`] implementation that records which commands were run, but does not
/// execute them.
#[derive(Debug, Default)]
struct RecordingCommandExecutor {
    commands: Mutex<Vec<String>>,
    response_queue: Mutex<VecDeque<CommandOutput>>,
}

impl RecordingCommandExecutor {
    pub fn with_success_responses(responses: impl IntoIterator<Item = &'static str>) -> Self {
        Self::with_outputs(
            responses
                .into_iter()
                .map(Self::success_output)
                .collect::<Vec<_>>(),
        )
    }

    pub fn with_outputs(outputs: impl IntoIterator<Item = CommandOutput>) -> Self {
        Self {
            commands: Mutex::default(),
            response_queue: Mutex::new(outputs.into_iter().collect()),
        }
    }

    pub fn success_output(stdout: impl AsRef<[u8]>) -> CommandOutput {
        CommandOutput {
            stdout: stdout.as_ref().to_vec(),
            stderr: vec![],
            status: CommandExitStatus::Success,
            exit_code: Some(ExitCode::from(0)),
        }
    }

    pub fn failure_output(stderr: impl AsRef<[u8]>, exit_code: ExitCode) -> CommandOutput {
        CommandOutput {
            stdout: vec![],
            stderr: stderr.as_ref().to_vec(),
            status: CommandExitStatus::Failure,
            exit_code: Some(exit_code),
        }
    }

    pub fn clear(&self) {
        self.commands.lock().clear();
    }
}

#[async_trait]
impl CommandExecutor for RecordingCommandExecutor {
    async fn execute_command(
        &self,
        command: &str,
        _shell: &Shell,
        _current_directory_path: Option<&str>,
        _environment_variables: Option<HashMap<String, String>>,
        _execute_command_options: ExecuteCommandOptions,
    ) -> anyhow::Result<CommandOutput> {
        self.commands.lock().push(command.to_string());
        let output = self
            .response_queue
            .lock()
            .pop_front()
            .unwrap_or_else(|| Self::success_output("test"));
        Ok(output)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn supports_parallel_command_execution(&self) -> bool {
        false
    }
}
