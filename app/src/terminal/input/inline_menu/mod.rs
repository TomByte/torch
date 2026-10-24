//! Generic inline menu view for rendering search results with selection and navigation.
mod message_bar;
mod message_provider;
mod model;
pub(crate) mod positioning;
pub mod styles;
mod view;

pub use message_bar::{InlineMenuMessageArgs, InlineMenuMessageBarArgs};
pub use message_provider::{InlineMenuMessageProvider, default_navigation_message_items};
pub use model::{InlineMenuModel, InlineMenuModelEvent, InlineMenuTabConfig};
pub use positioning::InlineMenuPositioner;
use serde::{Deserialize, Serialize};
pub use view::{
    DetailsRenderConfig, InlineMenuAction, InlineMenuClickBehavior, InlineMenuEvent,
    InlineMenuHeaderConfig, InlineMenuRowAction, InlineMenuView, QueryResultRendererExt,
};

use super::InputSuggestionsMode;

/// Identifies a specific inline menu type.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(
    description = "Identifies a specific inline menu.",
    rename_all = "snake_case"
)]
pub enum InlineMenuType {
    InlineHistoryMenu,
}

impl InlineMenuType {
    fn display_label(&self) -> &'static str {
        match self {
            InlineMenuType::InlineHistoryMenu => "History",
        }
    }

    pub(crate) fn from_suggestions_mode(mode: &InputSuggestionsMode) -> Option<Self> {
        match mode {
            InputSuggestionsMode::InlineHistoryMenu => Some(InlineMenuType::InlineHistoryMenu),
            InputSuggestionsMode::Closed
            | InputSuggestionsMode::HistoryUp { .. }
            | InputSuggestionsMode::CompletionSuggestions { .. }
            | InputSuggestionsMode::StaticWorkflowEnumSuggestions { .. }
            | InputSuggestionsMode::DynamicWorkflowEnumSuggestions { .. } => None,
        }
    }
}
