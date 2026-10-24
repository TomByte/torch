use std::cmp::Ordering;
use std::{fmt, io};

use async_trait::async_trait;

/// Distinguishes whether the plugin instructions modal should show install or update steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginModalKind {
    Install,
    Update,
}

/// A single step in the plugin install/update instructions pane.
pub(crate) struct PluginInstructionStep {
    pub description: &'static str,
    pub command: &'static str,
    /// When true, the code block shows a "Run" button that inserts the command into the terminal.
    /// Defaults-by-convention to `true`; set to `false` for steps that are not runnable
    /// (e.g. config file snippets).
    pub executable: bool,
    /// Optional URL rendered as a clickable "Learn more" link after the description.
    /// When set with an empty `command`, the code block is omitted entirely.
    pub link: Option<&'static str>,
}

/// All content needed to render the plugin instructions pane for a given agent.
pub(crate) struct PluginInstructions {
    pub title: &'static str,
    pub subtitle: &'static str,
    pub steps: &'static [PluginInstructionStep],
    /// Displayed after the steps in the same style as the subtitle, one per paragraph.
    pub post_install_notes: &'static [&'static str],
}

/// Error returned when plugin installation fails.
/// Carries both a short user-facing message (for the toast) and a detailed
/// command log (for the log file the user can inspect).
#[derive(Debug)]
pub(crate) struct PluginInstallError {
    /// Short description shown in the toast notification.
    pub message: String,
    /// Detailed log of every command/step that was attempted.
    pub log: String,
}

impl fmt::Display for PluginInstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PluginInstallError {}

impl From<io::Error> for PluginInstallError {
    fn from(err: io::Error) -> Self {
        let msg = err.to_string();
        Self {
            message: msg.clone(),
            log: msg,
        }
    }
}

/// Compares two `X.Y.Z` version strings.
/// Returns `Ordering::Less` if `a < b`, etc.
/// Unparseable components are treated as 0.
pub(crate) fn compare_versions(a: &str, b: &str) -> Ordering {
    let parse = |s: &str| -> [u64; 3] {
        let mut parts = s.splitn(3, '.');
        let major = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        [major, minor, patch]
    };
    parse(a).cmp(&parse(b))
}

/// Manages the Torch notification plugin for a specific CLI agent.
///
/// Each supported CLI agent has its own implementation that knows how to
/// check installation state and perform install/update operations.
#[async_trait]
pub(crate) trait CliAgentPluginManager: Send + Sync {
    /// The minimum plugin version required by this Torch build.
    fn minimum_plugin_version(&self) -> &'static str;

    /// Whether this agent supports one-click auto-install/update.
    /// When `false`, the footer always opens the manual instructions modal.
    fn can_auto_install(&self) -> bool;

    /// Whether the Torch notification plugin is installed.
    /// Default returns `false` (no filesystem check).
    fn is_installed(&self) -> bool {
        false
    }

    /// Whether the on-disk plugin version is below the minimum required.
    /// Default returns `false` (no filesystem check).
    fn needs_update(&self) -> bool {
        false
    }

    /// Install the Torch notification plugin.
    /// Default returns an error — only agents with `can_auto_install() == true` should override.
    async fn install(&self) -> Result<(), PluginInstallError> {
        Err(PluginInstallError {
            message: "Auto-install not supported for this agent".to_owned(),
            log: String::new(),
        })
    }

    /// Update the Torch notification plugin to the latest version.
    /// Default returns an error — only agents with `can_auto_install() == true` should override.
    async fn update(&self) -> Result<(), PluginInstallError> {
        Err(PluginInstallError {
            message: "Auto-update not supported for this agent".to_owned(),
            log: String::new(),
        })
    }

    /// Toast message shown after a successful auto-install.
    fn install_success_message(&self) -> &'static str {
        "Torch plugin installed. Please restart the session to activate."
    }

    /// Toast message shown after a successful auto-update.
    fn update_success_message(&self) -> &'static str {
        "Torch plugin updated. Please restart the session to activate."
    }

    /// Manual installation instructions for the modal UI.
    fn install_instructions(&self) -> &'static PluginInstructions;

    /// Whether this agent supports version-based update checking.
    /// When `false`, the update chip is never shown; only the install chip appears.
    fn supports_update(&self) -> bool {
        true
    }

    /// Manual update instructions for the modal UI.
    fn update_instructions(&self) -> &'static PluginInstructions;
}

/// Returns the notification plugin manager for a CLI agent. Torch installs no agent plugins: it
/// reports agent status through its own hooks in `crate::local_hooks`, so there is never one.
pub(crate) fn plugin_manager() -> Option<Box<dyn CliAgentPluginManager>> {
    None
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
