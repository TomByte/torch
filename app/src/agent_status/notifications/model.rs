use torchui::{AppContext, Entity, EntityId, ModelContext, SingletonEntity, ViewHandle};

use super::{NotificationCategory, NotificationId, NotificationItem, NotificationItems};
use crate::terminal::cli_agent_sessions::notification::{
    CLIAgentNotificationSubject, notification_text,
};
use crate::terminal::cli_agent_sessions::{
    CLIAgentSessionContext, CLIAgentSessionStatus, CLIAgentSessionsModel,
    CLIAgentSessionsModelEvent,
};
use crate::terminal::{CLIAgent, TerminalView};
use crate::workspace::util::is_terminal_view_in_same_tab;
use crate::workspace::{Workspace, WorkspaceRegistry};

/// Singleton model that tracks and stores in-app notifications for the notifications mailbox,
/// raised by status changes of CLI agent sessions.
pub struct AgentNotificationsModel {
    notifications: NotificationItems,
}

impl Entity for AgentNotificationsModel {
    type Event = AgentNotificationsEvent;
}

impl SingletonEntity for AgentNotificationsModel {}

impl AgentNotificationsModel {
    pub(crate) fn new(ctx: &mut ModelContext<Self>) -> Self {
        let cli_sessions_model = CLIAgentSessionsModel::handle(ctx);
        ctx.subscribe_to_model(&cli_sessions_model, |me, _, event, ctx| {
            me.handle_cli_agent_session_event(event, ctx);
        });

        Self {
            notifications: NotificationItems::default(),
        }
    }

    pub(crate) fn notifications(&self) -> &NotificationItems {
        &self.notifications
    }

    pub(crate) fn mark_item_read(&mut self, id: NotificationId, ctx: &mut ModelContext<Self>) {
        if self.notifications.mark_item_read(id) {
            ctx.emit(AgentNotificationsEvent::NotificationUpdated);
        }
    }

    pub(crate) fn mark_all_items_read(&mut self, ctx: &mut ModelContext<Self>) {
        if self.notifications.mark_all_items_read() {
            ctx.emit(AgentNotificationsEvent::AllNotificationsMarkedRead);
        }
    }

    /// Marks all notifications from the given terminal view as read.
    pub(crate) fn mark_items_from_terminal_view_read(
        &mut self,
        terminal_view_id: EntityId,
        ctx: &mut ModelContext<Self>,
    ) {
        if self
            .notifications
            .mark_all_terminal_view_items_as_read(terminal_view_id)
        {
            ctx.emit(AgentNotificationsEvent::NotificationUpdated);
        }
    }

    fn handle_cli_agent_session_event(
        &mut self,
        event: &CLIAgentSessionsModelEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        match event {
            CLIAgentSessionsModelEvent::Ended {
                terminal_view_id, ..
            } => {
                self.remove_notification_for_terminal_view(*terminal_view_id, ctx);
            }
            CLIAgentSessionsModelEvent::Started { .. }
            | CLIAgentSessionsModelEvent::InputSessionChanged { .. }
            | CLIAgentSessionsModelEvent::SessionUpdated { .. } => {}
            CLIAgentSessionsModelEvent::StatusChanged {
                terminal_view_id,
                agent,
                status,
                session_context,
                ..
            } => {
                self.add_cli_notification(*terminal_view_id, *agent, status, session_context, ctx);
            }
        }
    }

    /// Words a notification the way Torch's desktop notifications do, and skips the statuses
    /// they skip.
    fn add_cli_notification(
        &mut self,
        terminal_view_id: EntityId,
        agent: CLIAgent,
        status: &CLIAgentSessionStatus,
        session_context: &CLIAgentSessionContext,
        ctx: &mut ModelContext<Self>,
    ) {
        let category = match status {
            CLIAgentSessionStatus::Blocked { .. } => NotificationCategory::Request,
            CLIAgentSessionStatus::Success => NotificationCategory::Complete,
            CLIAgentSessionStatus::Failed { .. } => NotificationCategory::Error,
            CLIAgentSessionStatus::InProgress => {
                self.remove_notification_for_terminal_view(terminal_view_id, ctx);
                return;
            }
        };
        let terminal_view = find_terminal_view_by_id(terminal_view_id, ctx);
        let view = terminal_view.as_ref().map(|view| view.as_ref(ctx));
        let text = match view {
            Some(view) => view.agent_notification_text(agent, status, session_context, ctx),
            None => notification_text(
                &CLIAgentNotificationSubject {
                    agent_name: agent.display_name(),
                    session_title: None,
                    group_label: None,
                    tool_name: session_context.tool_name.as_deref(),
                },
                status,
            ),
        };
        let Some(text) = text else {
            return;
        };
        let branch = view.and_then(|view| view.current_git_branch(ctx));
        self.add_notification(
            text.title,
            text.body,
            category,
            agent,
            terminal_view_id,
            branch,
            ctx,
        );
    }

    /// Removes the existing notification for the given terminal view (if any) and emits an
    /// update event.
    fn remove_notification_for_terminal_view(
        &mut self,
        terminal_view_id: EntityId,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.notifications.remove_by_terminal_view(terminal_view_id) {
            ctx.emit(AgentNotificationsEvent::NotificationUpdated);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add_notification(
        &mut self,
        title: String,
        message: String,
        category: NotificationCategory,
        agent: CLIAgent,
        terminal_view_id: EntityId,
        branch: Option<String>,
        ctx: &mut ModelContext<Self>,
    ) {
        let is_visible = is_terminal_view_visible(terminal_view_id, ctx);
        let item = NotificationItem::new(
            title,
            message,
            category,
            agent,
            is_visible,
            terminal_view_id,
            branch,
        );

        let id = item.id;
        self.notifications.push(item);
        ctx.emit(AgentNotificationsEvent::NotificationAdded { id });
    }
}

#[derive(Clone, Debug)]
pub enum AgentNotificationsEvent {
    /// A new notification was added to the persistent notification center.
    NotificationAdded { id: NotificationId },
    /// A notification's read state changed.
    NotificationUpdated,
    /// All notifications were marked as read.
    AllNotificationsMarkedRead,
}

fn is_terminal_view_visible(terminal_view_id: EntityId, app: &AppContext) -> bool {
    let Some(active_id) = active_focused_terminal_id(app) else {
        return false;
    };
    active_id == terminal_view_id
        || is_terminal_view_in_same_tab(&active_id, &terminal_view_id, app)
}

fn find_terminal_view_by_id(
    terminal_view_id: EntityId,
    app: &AppContext,
) -> Option<ViewHandle<TerminalView>> {
    for (_, workspace_handle) in WorkspaceRegistry::as_ref(app).all_workspaces(app) {
        for pane_group in workspace_handle.as_ref(app).tab_views() {
            let pane_group = pane_group.as_ref(app);
            for pane_id in pane_group.terminal_pane_ids() {
                if let Some(terminal_view) = pane_group.terminal_view_from_pane_id(pane_id, app)
                    && terminal_view.id() == terminal_view_id
                {
                    return Some(terminal_view);
                }
            }
        }
    }
    None
}

fn active_focused_terminal_id(app: &AppContext) -> Option<EntityId> {
    let active_window = app.windows().active_window()?;
    let workspace = app
        .views_of_type::<Workspace>(active_window)
        .and_then(|views| views.first().cloned())?;

    let workspace = workspace.as_ref(app);
    workspace.active_terminal_id(app)
}
