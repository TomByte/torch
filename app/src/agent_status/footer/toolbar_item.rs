use serde::{Deserialize, Serialize};
use torchui::SingletonEntity;

use crate::context_chips::{ContextChipKind, available_chips};
use crate::features::FeatureFlag;
use crate::settings::CodeSettings;
use crate::ui_components::icons::Icon;

/// A configurable item
///
/// This unifies context-chip data displays with interactive control buttons so
/// they can all be arranged through the same drag-and-drop editor.
#[derive(
    Clone,
    Debug,
    Eq,
    PartialEq,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(
    description = "An item that can appear in the agent toolbar.",
    rename_all = "snake_case"
)]
pub enum AgentToolbarItemKind {
    #[schemars(description = "A prompt context chip.")]
    ContextChip(ContextChipKind),
    RichInput,
    FileExplorer,
    // Renamed from ImageAttach; alias preserves existing user toolbar configs.
    #[serde(alias = "ImageAttach")]
    FileAttach,
    /// Opens settings to the Coding Agents section.
    Settings,
}

impl AgentToolbarItemKind {
    pub fn display_label(&self) -> &'static str {
        match self {
            Self::ContextChip(_) => "Context Chip",
            Self::FileAttach => "Attach File",
            Self::FileExplorer => "File Explorer",
            Self::RichInput => "Rich Input",
            Self::Settings => "Settings",
        }
    }

    pub fn icon(&self) -> Option<Icon> {
        match self {
            Self::ContextChip(kind) => kind.udi_icon(),
            Self::FileAttach => Some(Icon::Plus),
            Self::FileExplorer => Some(Icon::FileCopy),
            Self::RichInput => Some(Icon::TextInput),
            Self::Settings => Some(Icon::Settings),
        }
    }

    /// Whether this item should be included in the toolbar given the current app state.
    /// Feature-flag checks live in `all_available()` / `default_*()`. This method
    /// handles runtime conditions that depend on user settings or workspace state.
    pub fn is_available(&self, app: &torchui::AppContext) -> bool {
        match self {
            // Matches the gating on every other project explorer entry point, so the chip
            // cannot open a tool view the rest of the app hides. See
            // `Workspace::compute_left_panel_views` and the `SHOW_PROJECT_EXPLORER`
            // keybinding predicate.
            Self::FileExplorer => {
                cfg!(feature = "local_fs")
                    && crate::code::editor_available()
                    && *CodeSettings::as_ref(app).show_project_explorer
            }
            Self::ContextChip(_) | Self::RichInput | Self::FileAttach | Self::Settings => true,
        }
    }

    pub fn is_context_chip(&self) -> bool {
        matches!(self, Self::ContextChip(_))
    }

    pub fn context_chip_kind(&self) -> Option<&ContextChipKind> {
        match self {
            Self::ContextChip(kind) => Some(kind),
            _ => None,
        }
    }

    /// Default left-side items for the footer.
    pub fn default_left() -> Vec<Self> {
        let mut items = vec![
            Self::FileAttach,
            Self::ContextChip(ContextChipKind::GitDiffStats),
        ];
        if crate::code::editor_available() {
            items.push(Self::FileExplorer);
        }
        if FeatureFlag::CLIAgentRichInput.is_enabled() {
            items.push(Self::RichInput);
        }
        items
    }

    /// Default right-side items for the footer.
    pub fn default_right() -> Vec<Self> {
        vec![
            Self::ContextChip(ContextChipKind::WorkingDirectory),
            Self::ContextChip(ContextChipKind::ShellGitBranch),
            Self::Settings,
        ]
    }

    /// All items available for the footer configurator.
    pub fn all_available() -> Vec<Self> {
        let mut items: Vec<Self> = available_chips()
            .into_iter()
            .map(Self::ContextChip)
            .collect();
        if crate::code::editor_available() {
            items.push(Self::FileExplorer);
        }
        items.extend([Self::RichInput, Self::FileAttach, Self::Settings]);
        items
    }
}

impl From<ContextChipKind> for AgentToolbarItemKind {
    fn from(kind: ContextChipKind) -> Self {
        Self::ContextChip(kind)
    }
}
