use std::collections::HashMap;
#[cfg(feature = "local_fs")]
use std::path::PathBuf;
#[cfg(feature = "local_fs")]
use std::time::Duration;

#[cfg(feature = "local_fs")]
use diesel::SqliteConnection;
use torch_completer::completer::{
    self, AliasExpansionResult, CompleterOptions, CompletionsFallbackStrategy, MatchStrategy,
    expand_command_aliases,
};
use torch_completer::meta::Spanned;
use torch_completer::parsers::ParsedExpression;
#[cfg(feature = "local_fs")]
use torch_completer::parsers::hir::ArgType;
use torch_completer::parsers::hir::{Command, Expression, FlagType};
#[cfg(feature = "local_fs")]
use torch_core::command::ExitCode;
use torch_core::features::FeatureFlag;
#[cfg(feature = "local_fs")]
use torchui::r#async::FutureExt;
use torchui::{AppContext, SingletonEntity};

use super::CompleterData;
use crate::completer::SessionContext;
#[cfg(feature = "local_fs")]
use crate::terminal::ShellHost;
use crate::terminal::{History, HistoryEntry};

#[cfg(feature = "local_fs")]
const ARG_GENERATOR_VALIDATION_TIMEOUT: Duration = Duration::from_millis(150);

#[cfg(feature = "local_fs")]
const MAX_NUM_SIMILAR_HISTORY_CONTEXT: usize = 25;

/// Returns the commands that historically followed `command`, oldest first, restricted to runs
/// with the same working directory, exit code and shell host.
#[cfg(feature = "local_fs")]
pub(super) fn next_commands_after_similar_history(
    conn: &mut SqliteConnection,
    command: &str,
    pwd: &Option<String>,
    exit_code: ExitCode,
    shell_host: Option<&ShellHost>,
) -> Vec<crate::persistence::model::Command> {
    let Ok(same_commands_from_history) =
        crate::persistence::commands::get_same_commands_from_history(
            conn,
            command,
            pwd,
            exit_code,
            shell_host,
            MAX_NUM_SIMILAR_HISTORY_CONTEXT,
        )
    else {
        return vec![];
    };
    same_commands_from_history
        .into_iter()
        .rev()
        .filter_map(|command| crate::persistence::commands::get_next_command(conn, &command).ok())
        .collect()
}

/// Returns the most recent command with a matching prefix run in the user's current working directory.
/// If no such command exists, returns the most recent command anywhere with a matching prefix.
pub(super) fn reverse_chronological_potential_autosuggestions(
    prefix: &str,
    completer_data: &CompleterData,
    app: &AppContext,
) -> Option<Vec<HistoryEntry>> {
    let session_id = completer_data.active_block_session_id()?;
    let history_entries = History::as_ref(app).commands(session_id)?;
    let working_dir = completer_data
        .active_block_metadata
        .as_ref()
        .and_then(|block_metadata| block_metadata.current_working_directory());
    Some(find_potential_autosuggestions_from_history(
        history_entries.into_iter(),
        prefix,
        working_dir,
    ))
}

/// Validates that the arg is valid given its type (e.g. filepath exists if it's a filepath arg).
/// This uses a file system call, so this function should be called only in background threads.
#[cfg_attr(not(feature = "local_fs"), allow(unused_variables))]
async fn is_arg_valid(
    full_command: &str,
    arg: &Spanned<ParsedExpression>,
    ctx: &SessionContext,
    session_env_vars: Option<&HashMap<String, String>>,
) -> bool {
    let Expression::ValidatableArgument(arg_types_to_validate) = arg.expression() else {
        return true;
    };
    // The expression shouldn't be parsed as a `ValidatableArgument` if the arg types are empty,
    // but we check this just in case.
    if arg_types_to_validate.is_empty() {
        return true;
    }
    cfg_if::cfg_if! {
        if #[cfg(feature = "local_fs")] {
            // If we have arg types to validate, the arg must pass validation for at least one of them.
            // If the argument has one or more generators, validate these last because they're more expensive
            // and we can check all generators together using completions suggestions.
            let mut has_generator_arg_type = false;
            for arg_type in arg_types_to_validate {
                match arg_type {
                    ArgType::File => {
                        let mut path_arg = PathBuf::from(arg.value().as_str());
                        if path_arg.is_relative()
                            && let Ok(working_dir) = PathBuf::try_from(ctx.current_working_directory.clone()) {
                                path_arg = working_dir.join(path_arg);
                            }
                        if path_arg.is_file() {
                            return true;
                        }
                    }
                    ArgType::Folder => {
                        let mut path_arg = PathBuf::from(arg.value().as_str());
                        if path_arg.is_relative()
                            && let Ok(working_dir) = PathBuf::try_from(ctx.current_working_directory.clone()) {
                                path_arg = working_dir.join(path_arg);
                            }
                        if path_arg.is_dir() {
                            return true;
                        }
                    }
                    ArgType::Generator(_) => {
                        has_generator_arg_type = true;
                    }
                };
            }
            if has_generator_arg_type {
                // We don't have completions implemented for feature flags like --features=with_local_server.
                // If arg is the span of `with_local_server`, attempting to complete on --features= to validate it will return no results.
                // We should only use completions to validate the arg if the previous character is whitespace, until completions handles this case.
                let prev_char = full_command.get(..arg.span.start()).and_then(|s| s.chars().next_back());
                if prev_char.is_some_and(|c| !c.is_whitespace()) {
                    return true;
                }
                // Running completions runs all generators, so we only need to do this once.
                // TODO(roland): this also generates completions from sources other than generators, which are unnecessary.
                // If performance becomes a concern, consider validating against generators sequentially and returning early if valid.
                // We use completions suggestions because it's simpler to implement and read.
                let completions_future = completer::suggestions(
                    full_command,
                    arg.span.start(),
                    session_env_vars,
                    CompleterOptions {
                        match_strategy: MatchStrategy::CaseSensitive,
                        fallback_strategy: CompletionsFallbackStrategy::None,
                        suggest_file_path_completions_only: false,
                        parse_quotes_as_literals: false,
                    },
                    ctx,
                );

                // If the completions call times out, assume the arg is valid.
                // This is necessary because some generators can hang (e.g. kubectl commands if the cluster isn't running).
                let Ok(completion_result) = completions_future.with_timeout(ARG_GENERATOR_VALIDATION_TIMEOUT).await else {
                    log::debug!("Generator validation for arg `{}` in command `{}` timed out - assuming it's valid", arg.value().as_str(), full_command);
                    return true;
                };

                let Some(completion_result) = completion_result else {
                    return true;
                };
                for suggestion in completion_result.suggestions {
                    if suggestion.display() == arg.value().as_str() {
                        return true;
                    }
                }
            }
            // If we didn't pass validation for any of the possible arg types, this arg is invalid.
            log::debug!("arg `{}` in command `{}` failed validation", arg.value().as_str(), full_command);
            false
        } else {
            true
        }
    }
}

/// Validates the command is valid.
/// Currently uses completions specs to check if parsing is successful, and validates
/// that any filepaths args actually exist on disk.
/// This uses a file system call, so this function should be called only in background threads.
pub(super) async fn is_command_valid(
    command: &str,
    ctx: Option<&SessionContext>,
    session_env_vars: Option<&HashMap<String, String>>,
) -> bool {
    if !FeatureFlag::ValidateAutosuggestions.is_enabled() {
        return true;
    }
    let Some(ctx) = ctx else {
        return true;
    };
    let AliasExpansionResult {
        expanded_command_line,
        classified_command,
        ..
    } = expand_command_aliases(command, false, ctx).await;

    let Some(classified_command) = classified_command else {
        return true;
    };

    // We assume the command is valid on parse error because
    // 1. Our completion specs are not always comprehensive (unknown args/options cause parse error)
    // 2. Our parsing logic has some bugs that need to be investigated (INT-816)
    if classified_command.error.is_some() {
        log::debug!(
            "Assuming command `{}` is valid because it failed to parse: {:?}",
            expanded_command_line,
            classified_command.error.unwrap()
        );
        return true;
    }
    // If we can't classify the command, it means we don't have completion specs for it.
    // Assume it's valid.
    let Command::Classified(shell_command) = classified_command.command else {
        return true;
    };
    if let Some(positionals) = &shell_command.args.positionals {
        for positional in positionals {
            if !is_arg_valid(&expanded_command_line, positional, ctx, session_env_vars).await {
                return false;
            }
        }
    }
    if let Some(flags) = shell_command.args.flags {
        for flag in flags.iter() {
            if let FlagType::Argument { value } = &flag.flag_type
                && !is_arg_valid(&expanded_command_line, value, ctx, session_env_vars).await
            {
                return false;
            }
        }
    }
    true
}

/// Scans the given history entries in reverse order for commands that start
/// with the buffer text to return as a potential autosuggestion. Prioritizes commands
/// in history that were executed in the user's current working directory,
/// with any command executed in other directories at the end.
fn find_potential_autosuggestions_from_history<'a>(
    history_entries: impl DoubleEndedIterator<Item = &'a HistoryEntry>,
    buffer_text: &str,
    working_dir: Option<&str>,
) -> Vec<HistoryEntry> {
    let mut commands_in_same_dir = vec![];
    let mut commands_in_other_dirs = vec![];
    for entry in history_entries.rev() {
        if !entry.command.starts_with(buffer_text) {
            continue;
        }
        let same_dir = entry
            .pwd
            .as_ref()
            .zip(working_dir)
            .is_some_and(|(pwd, working_dir)| pwd == working_dir);

        if same_dir {
            commands_in_same_dir.push(entry.clone());
        } else {
            commands_in_other_dirs.push(entry.clone());
        }
    }
    commands_in_same_dir.extend(commands_in_other_dirs);
    commands_in_same_dir
}
