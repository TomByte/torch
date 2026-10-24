use async_channel::Sender;
use itertools::Itertools as _;
pub use torch_terminal::shell::{shell_escape_single_quotes, shell_quote_arg};
use torch_util::path::ShellFamily;

use crate::terminal::model::session::command_executor::{
    InBandCommand, InBandCommandCancelledEvent,
};
use crate::terminal::shell::ShellType;

/// Set of events sent by command executors.
pub enum ExecutorCommandEvent {
    /// The command should be executed.
    ExecuteCommand {
        command: InBandCommand,
        /// A Sender that can be used to signal that the command has been cancelled.
        /// Lets us unblock the command in the executor.
        cancel_tx: Sender<InBandCommandCancelledEvent>,
    },
    /// The command identified by `id` should be cancelled.
    CancelCommand { id: String },
}

/// Serializes variables as a single shell-specific line of assignments to prefix a command with.
pub(super) fn serialize_variables_for_shell<'s>(
    pairs: impl IntoIterator<Item = (&'s str, &'s str)>,
    shell_type: ShellType,
) -> String {
    let shell_family = ShellFamily::from(shell_type);
    let (prefix, separator, postfix) = match shell_type {
        // Torch doesn't support newlines in fish so we can't use env syntax
        ShellType::Fish => ("set -x ", " ", ";"),
        ShellType::Bash | ShellType::Zsh => ("", "=", ""),
        ShellType::PowerShell => ("$env:", " = ", ";"),
    };
    pairs
        .into_iter()
        .map(|(name, value)| {
            let value = match shell_family {
                ShellFamily::Posix => shell_family.escape(value).into_owned(),
                ShellFamily::PowerShell => format!("'{}'", value.replace('\'', "''")),
            };
            format!(
                "{prefix}{}{separator}{value}{postfix}",
                shell_family.escape(name)
            )
        })
        .join(" ")
}
