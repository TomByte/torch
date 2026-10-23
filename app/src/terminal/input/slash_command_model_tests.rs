use torchui::{App, SingletonEntity as _};

use super::{ParsedSlashCommandInput, SlashCommandEntryState};
use crate::search::slash_command_menu::static_commands::commands;
use crate::terminal::input::slash_commands::SlashCommandDataSource as _;
use crate::terminal::input::tests::{add_window_with_bootstrapped_terminal, initialize_app};

#[test]
fn test_parse_input_requires_slash_at_start() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        let slash_command_data_source =
            input.read(&app, |input, _| input.slash_command_data_source.clone());

        slash_command_data_source.read(&app, |data_source, ctx| {
            let command_name = data_source
                .active_commands()
                .next()
                .map(|(_, command)| command.name)
                .expect("expected at least one active slash command");
            let input = format!("  {command_name}");

            assert!(matches!(
                data_source.parse_input(&input, ctx),
                ParsedSlashCommandInput::None
            ));
        });
    });
}

#[test]
fn test_parse_rename_tab_slash_command_arguments() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        let slash_command_data_source =
            input.read(&app, |input, _| input.slash_command_data_source.clone());

        slash_command_data_source.read(&app, |data_source, _| {
            assert!(
                data_source
                    .parse_slash_command(commands::RENAME_TAB.name)
                    .is_none(),
                "expected /rename-tab without an argument not to parse"
            );

            let detected_with_argument = data_source
                .parse_slash_command("/rename-tab Backend")
                .expect("expected /rename-tab to parse with an argument");
            assert_eq!(detected_with_argument.argument.as_deref(), Some("Backend"));

            let detected_with_multi_word_argument = data_source
                .parse_slash_command("/rename-tab Backend API")
                .expect("expected /rename-tab to preserve the rest of the line as one argument");
            assert_eq!(
                detected_with_multi_word_argument.argument.as_deref(),
                Some("Backend API")
            );

            let detected_with_empty_argument = data_source
                .parse_slash_command("/rename-tab ")
                .expect("expected /rename-tab to parse once the required argument is started");
            assert_eq!(detected_with_empty_argument.argument.as_deref(), Some(""));
        });
    });
}

#[test]
fn test_disabled_until_empty_buffer_ignores_non_slash_edits() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.update(&mut app, |input, ctx| {
            input.set_input_mode_natural_language_detection(ctx);
            input.user_insert("echo hello", ctx);
        });
        input.update(&mut app, |input, ctx| {
            input.slash_command_model.update(ctx, |model, ctx| {
                model.disable(ctx);
            });
        });

        input.read(&app, |input, ctx| {
            assert!(input.slash_command_model.as_ref(ctx).is_disabled());
        });

        input.update(&mut app, |input, ctx| {
            input.user_insert(" world", ctx);
        });

        input.read(&app, |input, ctx| {
            assert!(input.slash_command_model.as_ref(ctx).is_disabled());
        });
    });
}

#[test]
fn test_second_slash_in_command_token_sets_state_to_none() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(
            &mut app, None, /* history_file_commands */
            None,
        )
        .await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.update(&mut app, |input, ctx| {
            input.set_input_mode_natural_language_detection(ctx);
            input.user_insert("/foo/bar", ctx);
        });

        input.read(&app, |input, ctx| {
            assert!(matches!(
                input.slash_command_model.as_ref(ctx).state(),
                SlashCommandEntryState::None
            ));
        });
    });
}

#[test]
fn test_detect_command_returns_none_for_plain_text() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(&mut app, None, None).await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.read(&app, |input, ctx| {
            let result = input
                .slash_command_model
                .as_ref(ctx)
                .detect_command("fix the tests", ctx);
            assert!(
                matches!(result, SlashCommandEntryState::None),
                "plain text should not be detected as a command"
            );
        });
    });
}

#[test]
fn test_detect_command_finds_known_slash_command() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(&mut app, None, None).await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.read(&app, |input, ctx| {
            let slash_command_data_source = &input.slash_command_data_source;
            let command_name = slash_command_data_source
                .as_ref(ctx)
                .active_commands()
                .find_map(|(_, command)| {
                    slash_command_data_source
                        .as_ref(ctx)
                        .parse_slash_command(command.name)
                        .is_some()
                        .then(|| command.name.to_owned())
                })
                .expect("expected at least one active slash command");

            let result = input
                .slash_command_model
                .as_ref(ctx)
                .detect_command(&command_name, ctx);
            assert!(
                matches!(result, SlashCommandEntryState::SlashCommand(_)),
                "known command should be detected: {command_name}"
            );
        });
    });
}

#[test]
fn test_detect_command_returns_none_for_unknown_slash() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let terminal = add_window_with_bootstrapped_terminal(&mut app, None, None).await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());

        input.read(&app, |input, ctx| {
            let result = input
                .slash_command_model
                .as_ref(ctx)
                .detect_command("/nonexistent-command-xyz", ctx);
            assert!(
                matches!(result, SlashCommandEntryState::None),
                "unknown /command should return None"
            );
        });
    });
}

/// Regression test: a REPOSITORY-gated command must stop being available the
/// moment the working directory leaves the repository, even before async git
/// detection has refreshed the cached repo root. Before the fix, availability
/// keyed off the stale `active_repo_root` cache, so the command lingered in the
/// window after `cd`-ing out of the repo until detection resolved.
#[test]
fn repository_gated_command_drops_when_leaving_repository() {
    use repo_metadata::repositories::DetectedRepositories;
    use torch_util::standardized_path::StandardizedPath;

    use crate::terminal::input::tests::simulate_directory_for_completion;
    use crate::terminal::model::session::SessionInfo;

    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let session_info = SessionInfo::new_for_test();
        let session_id = session_info.session_id;
        let terminal =
            add_window_with_bootstrapped_terminal(&mut app, None, Some(session_info)).await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        let data_source = input.read(&app, |input, _| input.slash_command_data_source.clone());

        // Real on-disk directories so canonicalization (used by repo detection
        // and by the availability check) resolves consistently.
        let repo_dir = tempfile::TempDir::new().expect("repo temp dir");
        let outside_dir = tempfile::TempDir::new().expect("non-repo temp dir");
        let repo_path = repo_dir.path().to_path_buf();
        let outside_path = outside_dir.path().to_path_buf();

        // Register the repo root, mimicking async detection resolving.
        DetectedRepositories::handle(&app).update(&mut app, |repos, _| {
            let root = StandardizedPath::from_local_canonicalized(&repo_path)
                .expect("canonicalized repo root");
            repos.insert_test_repo_root(root);
        });

        // Enter the repo: both the live pwd and the (stale) cache point at it.
        simulate_directory_for_completion(
            session_id,
            &terminal,
            &mut app,
            repo_path.to_string_lossy().into_owned(),
        );
        data_source.update(&mut app, |data_source, ctx| {
            data_source.set_active_repo_root(Some(repo_path.clone()), ctx);
        });
        data_source.read(&app, |data_source, ctx| {
            assert!(
                data_source.command_is_active(&commands::OPEN_CODE_REVIEW, ctx),
                "a REPOSITORY-gated command should be available inside a repo"
            );
        });

        // Leave the repo. Async detection has NOT run yet, so the cached repo
        // root is still the old repo — this is the stale window the bug lived in.
        simulate_directory_for_completion(
            session_id,
            &terminal,
            &mut app,
            outside_path.to_string_lossy().into_owned(),
        );
        data_source.read(&app, |data_source, ctx| {
            assert!(
                !data_source.command_is_active(&commands::OPEN_CODE_REVIEW, ctx),
                "a REPOSITORY-gated command must drop immediately after leaving the \
                 repo, before async detection refreshes the cache"
            );
        });
    });
}

/// Control for the fix: moving *within* the same repository must not toggle a
/// REPOSITORY-gated command off (no flicker). Guards against an
/// eager-clear-on-`cd` approach that would drop the command on every directory
/// change inside a repo.
#[test]
fn repository_gated_command_stays_within_repository() {
    use std::fs;

    use repo_metadata::repositories::DetectedRepositories;
    use torch_util::standardized_path::StandardizedPath;

    use crate::terminal::input::tests::simulate_directory_for_completion;
    use crate::terminal::model::session::SessionInfo;

    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let session_info = SessionInfo::new_for_test();
        let session_id = session_info.session_id;
        let terminal =
            add_window_with_bootstrapped_terminal(&mut app, None, Some(session_info)).await;
        let input = terminal.read(&app, |terminal, _| terminal.input().clone());
        let data_source = input.read(&app, |input, _| input.slash_command_data_source.clone());

        let repo_dir = tempfile::TempDir::new().expect("repo temp dir");
        let repo_path = repo_dir.path().to_path_buf();
        let subdir_a = repo_path.join("a");
        let subdir_b = repo_path.join("b");
        fs::create_dir_all(&subdir_a).expect("create repo/a");
        fs::create_dir_all(&subdir_b).expect("create repo/b");

        DetectedRepositories::handle(&app).update(&mut app, |repos, _| {
            let root = StandardizedPath::from_local_canonicalized(&repo_path)
                .expect("canonicalized repo root");
            repos.insert_test_repo_root(root);
        });

        // In a subdirectory of the repo: command is available.
        simulate_directory_for_completion(
            session_id,
            &terminal,
            &mut app,
            subdir_a.to_string_lossy().into_owned(),
        );
        data_source.read(&app, |data_source, ctx| {
            assert!(
                data_source.command_is_active(&commands::OPEN_CODE_REVIEW, ctx),
                "a REPOSITORY-gated command should be available inside a repo subdir"
            );
        });

        // Move to a sibling subdirectory still inside the same repo: it must stay
        // available (no flicker off).
        simulate_directory_for_completion(
            session_id,
            &terminal,
            &mut app,
            subdir_b.to_string_lossy().into_owned(),
        );
        data_source.read(&app, |data_source, ctx| {
            assert!(
                data_source.command_is_active(&commands::OPEN_CODE_REVIEW, ctx),
                "a REPOSITORY-gated command must stay available when moving within the same repo"
            );
        });
    });
}
