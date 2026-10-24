mod entry;
mod query;

use std::collections::{HashMap, HashSet};

pub use entry::AgentConversationEntry;
use fuzzy_match::FuzzyMatchResult;
use itertools::Itertools;
pub use query::query_conversation_entries;
use torch_core::features::FeatureFlag;
use torch_core::ui::theme::WarpTheme;
use torch_core::ui::theme::color::internal_colors;
use torchui::color::ColorU;
use torchui::{AppContext, Entity, ModelContext, SingletonEntity};

use crate::agent_status::ConversationStatus;
use crate::agent_status::conversation_status::StatusElementStyle;
use crate::ai::active_agent_views_model::ActiveAgentViewsModel;
use crate::ai::agent::conversation::AIConversationId;
use crate::ai::blocklist::{
    BlocklistAIHistoryEvent, BlocklistAIHistoryModel, ConversationStatusUpdate,
};
use crate::ai::conversation_navigation::ConversationNavigationData;
use crate::ui_components::icons::Icon;
use crate::workspace::{RestoreConversationLayout, WorkspaceAction};

/// Frontend-specific classification of a normalized conversation-list entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentConversationListEntryState {
    Selected,
    OpenElsewhere,
    Available,
    Unavailable,
}

/// Per-frontend policy for classifying normalized conversation-list entries.
pub trait AgentConversationListPolicy: 'static {
    /// Classifies `entry` as selected, open elsewhere, available, or unavailable.
    fn classify_entry(
        &self,
        entry: &AgentConversationEntry,
        app: &AppContext,
    ) -> AgentConversationListEntryState;
}

/// A normalized conversation entry paired with optional title-match metadata.
pub struct AgentConversationQueryResult {
    pub entry: AgentConversationEntry,
    pub title_match: Option<FuzzyMatchResult>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentRunDisplayStatus {
    InProgress,
    Succeeded,
    Error,
    Blocked { blocked_action: String },
    Cancelled,
}

impl AgentRunDisplayStatus {
    pub fn from_conversation_status(status: &ConversationStatus) -> Self {
        match status {
            ConversationStatus::InProgress => Self::InProgress,
            // A recovery is in flight; the run is still working.
            ConversationStatus::TransientError => Self::InProgress,
            ConversationStatus::Success => Self::Succeeded,
            ConversationStatus::Error => Self::Error,
            ConversationStatus::Cancelled => Self::Cancelled,
            ConversationStatus::Blocked { blocked_action } => Self::Blocked {
                blocked_action: blocked_action.clone(),
            },
            // Treat a yielded conversation as still in progress so it stays in the working
            // bucket.
            ConversationStatus::WaitingForEvents => Self::InProgress,
        }
    }

    pub fn to_conversation_status(&self) -> ConversationStatus {
        match self {
            AgentRunDisplayStatus::InProgress => ConversationStatus::InProgress,
            AgentRunDisplayStatus::Succeeded => ConversationStatus::Success,
            AgentRunDisplayStatus::Error => ConversationStatus::Error,
            AgentRunDisplayStatus::Blocked { blocked_action } => ConversationStatus::Blocked {
                blocked_action: blocked_action.clone(),
            },
            AgentRunDisplayStatus::Cancelled => ConversationStatus::Cancelled,
        }
    }

    pub fn status_icon_and_color(&self, theme: &WarpTheme) -> (Icon, ColorU) {
        match self {
            AgentRunDisplayStatus::InProgress => (Icon::ClockLoader, theme.ansi_fg_magenta()),
            AgentRunDisplayStatus::Succeeded => (Icon::Check, theme.ansi_fg_green()),
            AgentRunDisplayStatus::Error => (Icon::Triangle, theme.ansi_fg_red()),
            AgentRunDisplayStatus::Blocked { .. } => (Icon::StopFilled, theme.ansi_fg_yellow()),
            AgentRunDisplayStatus::Cancelled => {
                (Icon::StopFilled, internal_colors::neutral_5(theme))
            }
        }
    }
}

impl StatusElementStyle for AgentRunDisplayStatus {
    fn status_icon_and_color(&self, theme: &WarpTheme) -> (Icon, ColorU) {
        AgentRunDisplayStatus::status_icon_and_color(self, theme)
    }
}

/// Stores conversation metadata needed for display in conversation views.
pub struct ConversationMetadata {
    pub nav_data: ConversationNavigationData,
}

/// This model serves as a unified interface for reading local agent conversations, both loaded
/// and historical.
///
/// This model backs the conversation list view.
pub struct AgentConversationsModel {
    /// A map of conversation IDs to local conversations.
    conversations: HashMap<AIConversationId, ConversationMetadata>,
    is_loading: bool,
}

pub enum AgentConversationsModelEvent {
    /// Conversation data was loaded or refreshed.
    ConversationsLoaded,
    /// Conversation status data was updated
    ConversationUpdated { kind: ConversationUpdateKind },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationUpdateKind {
    /// The conversation was re-loaded into a terminal view.
    Restored,
    /// The conversation's status was set.
    StatusSet,
    /// Conversation title changed.
    TitleChanged,
}

impl Entity for AgentConversationsModel {
    type Event = AgentConversationsModelEvent;
}

impl SingletonEntity for AgentConversationsModel {}

impl AgentConversationsModel {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let mut model = Self {
            conversations: HashMap::new(),
            is_loading: true,
        };

        let history_model = BlocklistAIHistoryModel::handle(ctx);
        ctx.subscribe_to_model(&history_model, move |me, _, event, ctx| {
            me.handle_history_event(event, ctx);
        });

        let active_views_model = ActiveAgentViewsModel::handle(ctx);
        ctx.subscribe_to_model(&active_views_model, |me, _, _event, ctx| {
            me.sync_conversations(ctx);
        });

        model.sync_conversations(ctx);
        model
    }

    /// Sync all conversations to the AgentConversationsModel.
    ///
    /// This function will loop through all active panes, recently closed panes, and historical
    /// conversations to construct a complete snapshot of conversations.
    pub fn sync_conversations(&mut self, ctx: &mut ModelContext<Self>) {
        if !FeatureFlag::InteractiveConversationManagementView.is_enabled() {
            return;
        }

        let nav_data_list = ConversationNavigationData::all_conversations(ctx);

        self.conversations.clear();
        for nav_data in nav_data_list {
            let conversation_id = nav_data.id;
            let metadata = ConversationMetadata { nav_data };
            self.conversations.insert(conversation_id, metadata);
        }
        self.is_loading = false;

        ctx.emit(AgentConversationsModelEvent::ConversationsLoaded);
    }

    /// Returns normalized, owned entries for conversation list and navigation surfaces, most
    /// recently updated first.
    pub fn get_entries(&self, app: &AppContext) -> Vec<AgentConversationEntry> {
        let history_model = BlocklistAIHistoryModel::as_ref(app);
        let mut entries = Vec::new();
        let mut emitted_conversation_ids = HashSet::new();

        for metadata in self.conversations.values() {
            emitted_conversation_ids.insert(metadata.nav_data.id);
            entries.push(entry::entry_for_conversation(metadata, history_model));
        }

        for metadata in history_model.get_local_conversations_metadata() {
            if emitted_conversation_ids.contains(&metadata.id) {
                continue;
            }
            let nav_data =
                ConversationNavigationData::from_historical_conversation_metadata(metadata);
            entries.push(entry::entry_for_historical_metadata(
                metadata,
                nav_data,
                history_model,
            ));
        }

        entries
            .into_iter()
            .sorted_by(|a, b| b.display.last_updated.cmp(&a.display.last_updated))
            .collect()
    }

    pub fn get_entry_by_id(
        &self,
        conversation_id: &AIConversationId,
        app: &AppContext,
    ) -> Option<AgentConversationEntry> {
        let history_model = BlocklistAIHistoryModel::as_ref(app);
        self.conversations
            .get(conversation_id)
            .map(|metadata| entry::entry_for_conversation(metadata, history_model))
            .or_else(|| {
                history_model
                    .get_conversation_metadata(conversation_id)
                    .map(|metadata| {
                        let nav_data =
                            ConversationNavigationData::from_historical_conversation_metadata(
                                metadata,
                            );
                        entry::entry_for_historical_metadata(metadata, nav_data, history_model)
                    })
            })
    }

    pub fn resolve_open_action(
        id: AIConversationId,
        restore_layout: Option<RestoreConversationLayout>,
        app: &AppContext,
    ) -> Option<WorkspaceAction> {
        let model = Self::as_ref(app);
        model
            .get_entry_by_id(&id, app)
            .and_then(|entry| model.resolve_entry_open_action(&entry, restore_layout, app))
    }

    fn resolve_entry_open_action(
        &self,
        entry: &AgentConversationEntry,
        restore_layout: Option<RestoreConversationLayout>,
        app: &AppContext,
    ) -> Option<WorkspaceAction> {
        let active_views_model = ActiveAgentViewsModel::as_ref(app);
        let conversation_id = entry.id;

        if active_views_model.is_conversation_open(conversation_id, app) {
            if let Some(nav_data) = self
                .conversations
                .get(&conversation_id)
                .map(|metadata| &metadata.nav_data)
            {
                return Some(WorkspaceAction::RestoreOrNavigateToConversation {
                    conversation_id,
                    window_id: nav_data.window_id,
                    pane_view_locator: nav_data.pane_view_locator,
                    terminal_view_id: nav_data.terminal_view_id,
                    restore_layout,
                });
            }

            if let Some(terminal_view_id) =
                active_views_model.get_terminal_view_id_for_conversation(conversation_id, app)
            {
                return Some(WorkspaceAction::FocusTerminalViewInWorkspace { terminal_view_id });
            }
        }

        let nav_data = self
            .conversations
            .get(&conversation_id)
            .map(|metadata| &metadata.nav_data);
        if !entry.backing.has_cloud_data
            || entry.backing.has_local_persisted_data
            || entry.backing.has_loaded_conversation
            || nav_data.is_some()
        {
            return Some(WorkspaceAction::RestoreOrNavigateToConversation {
                conversation_id,
                window_id: nav_data.and_then(|nav_data| nav_data.window_id),
                pane_view_locator: None,
                terminal_view_id: nav_data.and_then(|nav_data| nav_data.terminal_view_id),
                restore_layout,
            });
        }

        None
    }

    fn handle_history_event(
        &mut self,
        event: &BlocklistAIHistoryEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        if !FeatureFlag::InteractiveConversationManagementView.is_enabled() {
            return;
        }
        match event {
            // Events that affect conversation navigation data - need full sync
            BlocklistAIHistoryEvent::StartedNewConversation { .. }
            | BlocklistAIHistoryEvent::SetActiveConversation { .. }
            | BlocklistAIHistoryEvent::AppendedExchange { .. }
            | BlocklistAIHistoryEvent::SplitConversation { .. }
            | BlocklistAIHistoryEvent::RestoredConversations { .. }
            | BlocklistAIHistoryEvent::RemoveConversation { .. }
            | BlocklistAIHistoryEvent::DeletedConversation { .. }
            | BlocklistAIHistoryEvent::ClearedConversationsForTerminalSurface { .. }
            | BlocklistAIHistoryEvent::ClearedActiveConversation { .. } => {
                self.sync_conversations(ctx);
            }

            // Status changes - just trigger re-render since status is looked up at render time
            BlocklistAIHistoryEvent::UpdatedConversationStatus { update, .. } => {
                let kind = match update {
                    ConversationStatusUpdate::Restored => ConversationUpdateKind::Restored,
                    ConversationStatusUpdate::Changed { .. } => ConversationUpdateKind::StatusSet,
                };
                ctx.emit(AgentConversationsModelEvent::ConversationUpdated { kind });
            }

            BlocklistAIHistoryEvent::UpdatedConversationTitle { .. } => {
                ctx.emit(AgentConversationsModelEvent::ConversationUpdated {
                    kind: ConversationUpdateKind::TitleChanged,
                });
            }

            // Task/exchange-level changes that don't affect conversation navigation.
            BlocklistAIHistoryEvent::CreatedSubtask { .. }
            | BlocklistAIHistoryEvent::UpgradedTask { .. }
            | BlocklistAIHistoryEvent::ReassignedExchange { .. }
            | BlocklistAIHistoryEvent::UpdatedTodoList { .. }
            | BlocklistAIHistoryEvent::UpdatedAutoexecuteOverride { .. }
            // UpdatedStreamingExchange covers streaming and other exchange-level updates but
            // doesn't change any ConversationNavigationData fields (title comes from
            // UpdateTaskDescription, last_updated uses exchange.start_time which is set at append time).
            | BlocklistAIHistoryEvent::UpdatedStreamingExchange { .. }
            | BlocklistAIHistoryEvent::ConversationTransferredBetweenTerminalSurfaces { .. }
            | BlocklistAIHistoryEvent::ConversationUsageMetadataUpdated { .. }
            | BlocklistAIHistoryEvent::UpdatedConversationMetadata { .. } => {}
        }
    }

    /// Clears all stored conversation data in memory.
    /// This is used when logging out to ensure no conversation history persists across users.
    pub(crate) fn reset(&mut self) {
        self.conversations.clear();
        self.is_loading = true;
    }
}
