//! Notebook-specific telemetry definitions.

use serde::{Deserialize, Serialize};

use super::editor::BlockInsertionSource;

/// A user action within a notebook. Some actions, like running a command, are not included here
/// because they're covered by existing telemetry.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum NotebookTelemetryAction {
    /// A block within the notebook was copied to the clipboard.
    /// Currently, this only applies to command-like blocks.
    CopyBlock {
        #[serde(flatten)]
        block: BlockInfo,
        entrypoint: ActionEntrypoint,
    },
    /// The user opened the block insertion menu.
    OpenBlockInsertionMenu { source: BlockInsertionSource },
    /// The user opened the find bar.
    OpenFindBar,
    /// The user opened the right-click context menu.
    OpenContextMenu,
    /// The selection mode changed.
    ChangeSelectionMode { mode: SelectionMode },
    /// The user navigated between command/code blocks with the keyboard.
    CommandKeyboardNavigation,
}

/// Generic entrypoint information for actions that might be keyboard or mouse driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionEntrypoint {
    /// A keyboard shortcut.
    Keyboard,
    /// A button in the UI.
    Button,
    /// A menu item.
    Menu,
}

/// Information about a block in the notebook.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "block_type")]
pub enum BlockInfo {
    /// A code or command block within the notebook.
    CodeBlock,
}

/// A selection/navigation mode within the notebook.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelectionMode {
    /// Navigate between command/code blocks.
    Command,
    /// Navigate with a text cursor/selection.
    Text,
}
