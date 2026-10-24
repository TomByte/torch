//! Implementation of terminal panes.
#[cfg(not(target_family = "wasm"))]
use std::collections::HashMap;
use std::sync::mpsc::SyncSender;

#[cfg(not(target_family = "wasm"))]
use torch_cli::agent::Harness;
use torch_core::execution_mode::AppExecutionMode;
use torch_errors::report_error;
use torchui::{
    AppContext, EntityId, ModelHandle, SingletonEntity, ViewContext, ViewHandle, WindowId,
};

#[cfg(not(target_family = "wasm"))]
use super::local_harness_launch::{PreparedLocalHarnessLaunch, prepare_local_harness_child_launch};
use super::{
    DetachType, PaneConfiguration, PaneContent, PaneId, PaneStackEvent, PaneView, ShareableLink,
    ShareableLinkError, TerminalPaneId,
};
// Imports below are only consumed by the non-wasm `launch_local_*_child`
// dispatch helpers; gating them keeps the wasm build warning-clean.
use crate::AIExecutionProfilesModel;
use crate::ai::active_agent_views_model::ActiveAgentViewsModel;
use crate::ai::agent::StartAgentExecutionMode;
use crate::ai::agent::conversation::{AIConversationId, ConversationStatus};
use crate::ai::ambient_agents::task::normalize_orchestrator_agent_name;
#[cfg(feature = "local_fs")]
use crate::ai::blocklist::BlocklistAIHistoryEvent;
use crate::ai::blocklist::agent_view::{AgentViewControllerEvent, AgentViewEntryOrigin};
use crate::ai::blocklist::orchestration_event_streamer::OrchestrationEventStreamer;
use crate::ai::blocklist::{
    BlocklistAIHistoryModel, StartAgentRequest, TEAM_CHANGED_DURING_CHILD_LAUNCH_ERROR,
};
#[cfg(not(target_family = "wasm"))]
use crate::ai::blocklist::{
    apply_child_agent_model_override, finish_local_oz_child_conversation,
    prepare_local_oz_child_launch,
};
use crate::ai::conversation_utils;
use crate::ai::llms::LLMPreferences;
use crate::app_state::{LeafContents, TerminalPaneSnapshot};
use crate::code::buffer_location::LocalOrRemotePath;
#[cfg(feature = "local_fs")]
use crate::pane_group::CodeSource;
use crate::pane_group::Event::OpenConversationHistory;
use crate::pane_group::child_agent::{
    ErrorChildAgentConversationRequest, create_error_child_agent_conversation,
};
#[cfg(not(target_family = "wasm"))]
use crate::pane_group::child_agent::{
    HiddenChildAgentConversation, HiddenChildAgentConversationRequest, HiddenChildAgentTaskContext,
    create_hidden_child_agent_conversation,
};
use crate::pane_group::{self, Direction, PaneGroup};
use crate::persistence::{BlockCompleted, ModelEvent};
#[cfg(not(target_family = "wasm"))]
use crate::server::server_api::ServerApiProvider;
use crate::session_management::SessionNavigationData;
use crate::terminal::cli_agent_sessions::CLIAgentSessionsModel;
use crate::terminal::cli_agent_sessions::resume::ResumableAgentSession;
use crate::terminal::general_settings::GeneralSettings;
use crate::terminal::view::Event;
use crate::terminal::{TerminalManager, TerminalView};
use crate::view_components::ToastFlavor;
use crate::workspace::sync_inputs::SyncedInputState;
use crate::workspace::{PaneViewLocator, WorkspaceRegistry};
#[cfg(not(target_family = "wasm"))]
use crate::workspaces::user_workspaces::TeamContextForOperation;
use crate::workspaces::user_workspaces::UserWorkspaces;

pub type TerminalPaneView = PaneView<TerminalView>;

/// Data kept for terminal panes.
pub struct TerminalPane {
    model_event_sender: Option<SyncSender<ModelEvent>>,

    /// Used to uniquely identify the pane, even across separate runs of the app.
    uuid: Vec<u8>,

    pane_configuration: ModelHandle<PaneConfiguration>,

    /// Defining `terminal_manager` before `view` means that `terminal_manager`
    /// gets dropped first (guaranteed by the language), which halts the event
    /// loop and avoids possible deadlocks during session cleanup. This is enforced
    /// by the `PaneStack`, since the terminal manager is the associated data for
    /// the backing pane view.
    view: ViewHandle<TerminalPaneView>,
}

impl TerminalPane {
    pub(in crate::pane_group) fn new(
        uuid: Vec<u8>,
        terminal_manager: ModelHandle<Box<dyn TerminalManager>>,
        terminal_view: ViewHandle<TerminalView>,
        model_event_sender: Option<SyncSender<ModelEvent>>,
        ctx: &mut ViewContext<PaneGroup>,
    ) -> Self {
        let pane_configuration = terminal_view.as_ref(ctx).pane_configuration().to_owned();
        let view = ctx.add_typed_action_view(|ctx| {
            let pane_id = PaneId::from_terminal_pane_ctx(ctx);
            PaneView::new(
                pane_id,
                terminal_view,
                terminal_manager,
                pane_configuration.clone(),
                ctx,
            )
        });

        Self {
            model_event_sender,
            uuid,
            pane_configuration,
            view,
        }
    }

    /// The [`PaneView<TerminalView>`] for this pane.
    #[cfg(any(test, feature = "integration_tests"))]
    pub(in crate::pane_group) fn pane_view(&self) -> ViewHandle<TerminalPaneView> {
        self.view.to_owned()
    }

    /// The [`TerminalView`] backing the [`PaneView`] for this terminal pane.
    pub(crate) fn terminal_view(&self, ctx: &AppContext) -> ViewHandle<TerminalView> {
        self.view.as_ref(ctx).child(ctx)
    }

    /// The UUID that identifies this terminal session across app restarts.
    pub(in crate::pane_group) fn session_uuid(&self) -> Vec<u8> {
        self.uuid.clone()
    }

    /// The terminal manager responsible for this session's event loop.
    pub(in crate::pane_group) fn terminal_manager(
        &self,
        ctx: &AppContext,
    ) -> ModelHandle<Box<dyn TerminalManager>> {
        self.view.as_ref(ctx).child_data(ctx).clone()
    }

    /// Instructs the SQLite thread to delete blocks for this session.
    pub(in crate::pane_group) fn delete_blocks(&self, ctx: &AppContext) {
        if !AppExecutionMode::as_ref(ctx).can_save_session() {
            return;
        }

        if let Some(sender) = &self.model_event_sender {
            let model_event = ModelEvent::DeleteBlocks(self.uuid.clone());
            if let Err(err) = sender.send(model_event) {
                report_error!(
                    anyhow::Error::new(err).context("Error sending blocks deleted event"),
                    extra: { "terminal_id" => ?self.terminal_view(ctx).id() }
                );
            }
        }
    }

    pub fn session_navigation_data(
        &self,
        pane_group_id: EntityId,
        window_id: WindowId,
        app: &AppContext,
    ) -> SessionNavigationData {
        let view = self.terminal_view(app).as_ref(app);
        let prompt_elements = view.prompt_elements(app);
        let prompt = match &prompt_elements.prompt_chip_snapshot {
            Some(snapshot) => snapshot.to_string(),
            None => view.full_prompt(app),
        };
        SessionNavigationData::new(
            prompt,
            prompt_elements,
            view.session_command_context(app),
            PaneViewLocator {
                pane_group_id,
                pane_id: self.id(),
            },
            view.last_focus_ts(),
            view.is_read_only(),
            window_id,
        )
    }

    pub fn terminal_pane_id(&self) -> TerminalPaneId {
        self.id()
            .as_terminal_pane_id()
            .expect("Should be able to derive a TerminalPaneId from TerminalPane")
    }
}

impl PaneContent for TerminalPane {
    fn id(&self) -> PaneId {
        PaneId::from_terminal_pane_view(&self.view)
    }

    fn attach(
        &self,
        group: &PaneGroup,
        focus_handle: crate::pane_group::focus_state::PaneFocusHandle,
        ctx: &mut ViewContext<PaneGroup>,
    ) {
        // TODO(ben): As much as possible, logic from PaneGroup::add_session should go here.
        //  This will simplify PaneGroup, especially when implementing pane management.
        let terminal_pane_id = self.terminal_pane_id();

        self.view
            .update(ctx, |view, ctx| view.set_focus_handle(focus_handle, ctx));

        // Attach the initial terminal view in the stack.
        attach_terminal_view(&self.terminal_view(ctx), terminal_pane_id, ctx);

        // Subscribe to the pane stack to handle views being pushed/popped.
        let pane_stack = self.view.as_ref(ctx).pane_stack().clone();
        ctx.subscribe_to_model(&pane_stack, move |group, _, event, ctx| {
            handle_pane_stack_event(group, event, terminal_pane_id, ctx);
        });

        ctx.subscribe_to_view(&self.view, move |group, _, event, ctx| {
            group.handle_pane_view_event(terminal_pane_id.into(), event, ctx);
        });

        if SyncedInputState::as_ref(ctx).should_sync_this_pane_group(ctx.view_id(), ctx.window_id())
            && let Some(active_pane_view) = group.active_session_view(ctx)
        {
            let event = active_pane_view
                .as_ref(ctx)
                .create_sync_event_based_on_terminal_state(ctx);

            group.send_sync_event_to_session(terminal_pane_id, &event, ctx);
        }

        let terminal_view_id = self.terminal_view(ctx).id();

        #[cfg(feature = "local_fs")]
        {
            ctx.subscribe_to_model(
                &BlocklistAIHistoryModel::handle(ctx),
                move |group, _, event, ctx| {
                    let Some(model_event_sender) = group.model_event_sender.clone() else {
                        return;
                    };

                    handle_ai_history_event(
                        event,
                        terminal_view_id,
                        terminal_pane_id,
                        model_event_sender,
                        ctx,
                    );
                },
            );
        }

        // Store the pane group entity ID on the agent view controller so the
        // message bar can perform pane-group-scoped visibility checks.
        let pane_group_id = ctx.view_id();
        let terminal_view = self.terminal_view(ctx);
        let agent_view_controller = terminal_view.as_ref(ctx).agent_view_controller().clone();
        agent_view_controller.update(ctx, |controller, _ctx| {
            controller.set_pane_group_id(pane_group_id);
        });
        ctx.subscribe_to_model(&agent_view_controller, move |group, _, event, ctx| {
            if let AgentViewControllerEvent::EnteredAgentView {
                conversation_id,
                display_mode,
                ..
            } = event
                && display_mode.is_fullscreen()
            {
                group.restore_child_agents_for_parent(
                    *conversation_id,
                    terminal_pane_id.into(),
                    ctx,
                );
            }
        });
        let active_session = terminal_view.as_ref(ctx).active_session().clone();
        ActiveAgentViewsModel::handle(ctx).update(ctx, |model, ctx| {
            model.register_agent_view_controller(
                &agent_view_controller,
                &active_session,
                terminal_view_id,
                ctx,
            );
        });
    }

    fn detach(
        &self,
        _group: &PaneGroup,
        detach_type: DetachType,
        ctx: &mut ViewContext<PaneGroup>,
    ) {
        if matches!(detach_type, DetachType::Closed) {
            // Only immediately clear conversations and delete blocks if the session is being
            // permanently closed.
            BlocklistAIHistoryModel::handle(ctx).update(ctx, |history_model, ctx| {
                history_model.clear_conversations_for_closed_terminal_surface(
                    self.terminal_view(ctx).id(),
                    ctx,
                );
            });
            self.delete_blocks(ctx);
        }

        // Unsubscribe from all views in the pane stack.
        let pane_stack = self.view.as_ref(ctx).pane_stack().clone();
        let contents = pane_stack.as_ref(ctx).entries().to_vec();
        let terminal_view_ids = contents
            .iter()
            .map(|(_, view)| view.id())
            .collect::<Vec<_>>();
        for (_, view) in contents {
            ctx.unsubscribe_to_view(&view);
        }

        // Notify the active agent views model that the terminal view has been closed
        // (and that any active views are no longer active). On a `HiddenForClose` detach,
        // `attach` will re-register via `register_agent_view_controller` when the tab is
        // restored, so this is safe to run unconditionally.
        let terminal_view_id = self.terminal_view(ctx).id();
        ActiveAgentViewsModel::handle(ctx).update(ctx, |model, ctx| {
            for terminal_view_id in terminal_view_ids {
                model.unregister_agent_view_controller(terminal_view_id, ctx);
            }
        });

        // Clean up any active CLI agent session so its notification is removed.
        // Skip this for moves — the session is still running and will re-register in the new tab.
        if !matches!(detach_type, DetachType::Moved) {
            CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
                sessions.remove_session(terminal_view_id, ctx);
            });
        }

        ctx.unsubscribe_to_model(&pane_stack);

        ctx.unsubscribe_to_view(&self.view);
        ctx.unsubscribe_to_model(
            &self
                .terminal_view(ctx)
                .as_ref(ctx)
                .agent_view_controller()
                .clone(),
        );

        #[cfg(feature = "local_fs")]
        {
            ctx.unsubscribe_to_model(&BlocklistAIHistoryModel::handle(ctx));
        }
    }

    fn snapshot(&self, app: &AppContext) -> LeafContents {
        let view = self.terminal_view(app).as_ref(app);
        let is_active = view.is_active_session(app);

        // Capture the current input_config from the AI input model
        let current_input_config = view.input_config(app.as_ref());

        let llm_model_override =
            LLMPreferences::as_ref(app).get_base_llm_override(self.terminal_view(app).id());

        let active_profile_id = AIExecutionProfilesModel::as_ref(app)
            .active_profile(Some(self.terminal_view(app).id()), app)
            .sync_id();

        // Collect all conversation IDs for this terminal view
        let conversation_ids_to_restore = BlocklistAIHistoryModel::as_ref(app)
            .all_live_conversations_for_terminal_surface(self.terminal_view(app).id())
            .map(|conversation| conversation.id())
            .collect();

        // Capture agent view state: if fullscreen, store the active conversation ID
        let active_conversation_id = view
            .agent_view_controller()
            .as_ref(app)
            .agent_view_state()
            .display_mode()
            .filter(|mode| mode.is_fullscreen())
            .and_then(|_| {
                view.agent_view_controller()
                    .as_ref(app)
                    .agent_view_state()
                    .active_conversation_id()
            });

        LeafContents::Terminal(TerminalPaneSnapshot {
            uuid: self.uuid.clone(),
            cwd: view.pwd_if_local(app),
            is_active,
            is_read_only: view.model.lock().is_read_only(),
            shell_launch_data: view.shell_launch_data_if_local(app),
            input_config: Some(current_input_config),
            llm_model_override,
            active_profile_id,
            conversation_ids_to_restore,
            active_conversation_id,
            cli_agent_session: resumable_agent_session(view.id(), app),
        })
    }

    fn has_application_focus(&self, ctx: &mut ViewContext<PaneGroup>) -> bool {
        self.view.is_self_or_child_focused(ctx)
    }

    fn focus(&self, ctx: &mut ViewContext<PaneGroup>) {
        self.terminal_view(ctx)
            .update(ctx, |view, ctx| view.redetermine_global_focus(ctx));
    }

    fn shareable_link(
        &self,
        _ctx: &mut ViewContext<PaneGroup>,
    ) -> Result<ShareableLink, ShareableLinkError> {
        Ok(ShareableLink::Base)
    }

    fn pane_configuration(&self) -> ModelHandle<PaneConfiguration> {
        self.pane_configuration.clone()
    }

    fn is_pane_being_dragged(&self, ctx: &AppContext) -> bool {
        self.view.as_ref(ctx).is_being_dragged()
    }
}

#[derive(Clone, Copy)]
struct AgentConversationActionState {
    owner_terminal_view_id: EntityId,
    is_in_progress: bool,
}

fn agent_conversation_action_state(
    conversation_id: AIConversationId,
    ctx: &AppContext,
) -> Option<AgentConversationActionState> {
    let history_model = BlocklistAIHistoryModel::as_ref(ctx);
    let conversation = history_model.conversation(&conversation_id)?;
    let owner_terminal_view_id =
        history_model.terminal_surface_id_for_conversation(&conversation_id)?;
    Some(AgentConversationActionState {
        owner_terminal_view_id,
        is_in_progress: conversation.status().is_in_progress(),
    })
}

fn terminal_view_for_owner_in_group(
    group: &PaneGroup,
    owner_terminal_view_id: EntityId,
    ctx: &AppContext,
) -> Option<ViewHandle<TerminalView>> {
    let pane_id = group.find_pane_id_for_terminal_view(owner_terminal_view_id, ctx)?;
    group.terminal_view_from_pane_id(pane_id, ctx)
}

fn pane_group_and_terminal_view_for_owner(
    owner_terminal_view_id: EntityId,
    ctx: &AppContext,
) -> Option<(ViewHandle<PaneGroup>, ViewHandle<TerminalView>)> {
    WorkspaceRegistry::as_ref(ctx)
        .all_workspaces(ctx)
        .into_iter()
        .find_map(|(_, workspace)| {
            workspace.as_ref(ctx).tab_views().find_map(|pane_group| {
                terminal_view_for_owner_in_group(
                    pane_group.as_ref(ctx),
                    owner_terminal_view_id,
                    ctx,
                )
                .map(|terminal_view| (pane_group.clone(), terminal_view))
            })
        })
}

fn stop_local_agent_conversation(
    group: &PaneGroup,
    owner_terminal_view_id: EntityId,
    conversation_id: AIConversationId,
    ctx: &mut ViewContext<PaneGroup>,
) -> bool {
    let terminal_view = terminal_view_for_owner_in_group(group, owner_terminal_view_id, ctx)
        .or_else(|| {
            pane_group_and_terminal_view_for_owner(owner_terminal_view_id, ctx)
                .map(|(_, terminal_view)| terminal_view)
        });
    let Some(terminal_view) = terminal_view else {
        log::warn!(
            "StopAgentConversation: no terminal view found for conversation {conversation_id:?}"
        );
        return false;
    };

    terminal_view.update(ctx, |terminal_view, ctx| {
        terminal_view.stop_local_agent_conversation(conversation_id, ctx);
    });
    true
}

fn stop_agent_conversation(
    group: &PaneGroup,
    conversation_id: AIConversationId,
    ctx: &mut ViewContext<PaneGroup>,
) {
    let Some(state) = agent_conversation_action_state(conversation_id, ctx) else {
        log::warn!("StopAgentConversation: conversation {conversation_id:?} not found");
        return;
    };
    if !state.is_in_progress {
        return;
    }
    if !stop_local_agent_conversation(group, state.owner_terminal_view_id, conversation_id, ctx) {
        // If the owner view is gone, still make Stop visible in history.
        BlocklistAIHistoryModel::handle(ctx).update(ctx, |history_model, ctx| {
            history_model.update_conversation_status(
                state.owner_terminal_view_id,
                conversation_id,
                ConversationStatus::Cancelled,
                ctx,
            );
        });
    }
}

fn pane_group_hosting_split_off_child(
    conversation_id: AIConversationId,
    ctx: &AppContext,
) -> Option<ViewHandle<PaneGroup>> {
    WorkspaceRegistry::as_ref(ctx)
        .all_workspaces(ctx)
        .into_iter()
        .find_map(|(_, workspace)| {
            workspace.as_ref(ctx).tab_views().find_map(|pane_group| {
                let group = pane_group.as_ref(ctx);
                group
                    .child_agent_origin()
                    .is_some_and(|origin| origin.conversation_id == conversation_id)
                    .then(|| pane_group.clone())
            })
        })
}

fn discard_child_agent_pane_for_conversation(
    group: &mut PaneGroup,
    owner_terminal_view_id: Option<EntityId>,
    conversation_id: AIConversationId,
    ctx: &mut ViewContext<PaneGroup>,
) -> bool {
    if group.discard_child_agent_pane_for_conversation(conversation_id, ctx) {
        return true;
    }
    if let Some(split_off_pane_group) = pane_group_hosting_split_off_child(conversation_id, ctx)
        && split_off_pane_group.id() != ctx.view_id()
        && split_off_pane_group.update(ctx, |pane_group, ctx| {
            pane_group.discard_child_agent_pane_for_conversation(conversation_id, ctx)
        })
    {
        return true;
    }

    let Some(owner_terminal_view_id) = owner_terminal_view_id else {
        return false;
    };
    let Some((owner_pane_group, _)) =
        pane_group_and_terminal_view_for_owner(owner_terminal_view_id, ctx)
    else {
        return false;
    };
    if owner_pane_group.id() == ctx.view_id() {
        return false;
    }

    owner_pane_group.update(ctx, |pane_group, ctx| {
        pane_group.discard_child_agent_pane_for_conversation(conversation_id, ctx)
    })
}

fn kill_agent_conversation(
    group: &mut PaneGroup,
    source_terminal_view_id: Option<EntityId>,
    conversation_id: AIConversationId,
    ctx: &mut ViewContext<PaneGroup>,
) {
    let state = agent_conversation_action_state(conversation_id, ctx);
    // Tombstone every Kill so late events cannot restore a removed child.
    OrchestrationEventStreamer::handle(ctx).update(ctx, |streamer, ctx| {
        streamer.mark_conversation_killed(conversation_id, ctx);
    });

    if let Some(state) = state
        && state.is_in_progress
    {
        stop_local_agent_conversation(group, state.owner_terminal_view_id, conversation_id, ctx);
    }

    let owner_terminal_view_id = state
        .map(|state| state.owner_terminal_view_id)
        .or(source_terminal_view_id);
    if !discard_child_agent_pane_for_conversation(
        group,
        owner_terminal_view_id,
        conversation_id,
        ctx,
    ) {
        log::warn!("KillAgentConversation: no child pane found for {conversation_id:?}");
    }

    if owner_terminal_view_id.is_none() {
        log::warn!(
            "KillAgentConversation: no terminal view found for conversation {conversation_id:?}"
        );
    }
    // Delete (not remove): drop the conversation from sqlite + cloud so a
    // killed child does not resurrect on restart.
    conversation_utils::delete_conversation(conversation_id, owner_terminal_view_id, ctx);
}

/// Attaches a terminal view to the pane group by subscribing to its events
/// and setting the file tree code model.
fn attach_terminal_view(
    terminal_view: &ViewHandle<TerminalView>,
    terminal_pane_id: TerminalPaneId,
    ctx: &mut ViewContext<PaneGroup>,
) {
    ctx.subscribe_to_view(
        terminal_view,
        move |group: &mut PaneGroup, _, event, ctx| {
            handle_terminal_view_event(group, terminal_pane_id, event, ctx);
        },
    );
}

/// Handles events from the pane stack when views are added or removed.
fn handle_pane_stack_event(
    group: &mut PaneGroup,
    event: &PaneStackEvent<TerminalView>,
    terminal_pane_id: TerminalPaneId,
    ctx: &mut ViewContext<PaneGroup>,
) {
    match event {
        PaneStackEvent::ViewAdded(terminal_view) => {
            attach_terminal_view(terminal_view, terminal_pane_id, ctx);
        }
        PaneStackEvent::ViewRemoved(terminal_view) => {
            ctx.unsubscribe_to_view(terminal_view);
        }
    }

    // Ensure we use the new top-level view's title and active session status.
    // TODO(ben): This shouldn't be necessary once titles are set declaratively.
    if let Some(active_terminal) = group.terminal_view_from_pane_id(terminal_pane_id, ctx) {
        active_terminal.update(ctx, |view, ctx| view.on_pane_state_change(ctx));
    }
}

fn handle_terminal_view_event(
    group: &mut PaneGroup,
    terminal_pane_id: TerminalPaneId,
    event: &Event,
    ctx: &mut ViewContext<PaneGroup>,
) {
    let pane_id = terminal_pane_id.into();

    if group.pane_contents.contains_key(&pane_id) {
        match event {
            Event::Escape => ctx.emit(pane_group::Event::Escape),
            Event::ExecuteCommand(event) => {
                ctx.emit(pane_group::Event::ExecuteCommand(event.clone()));
            }
            Event::Exited => {
                // If the shell process exited before it successfully bootstrapped,
                // keep the pane open.  There might be useful information visible
                // in the output, and if this was the first shell spawned when the
                // user started the app, it will prevent it from suddenly quitting.
                if group
                    .terminal_view_from_pane_id(terminal_pane_id, ctx)
                    .is_some_and(|terminal_view| {
                        !terminal_view.as_ref(ctx).is_login_shell_bootstrapped()
                    })
                {
                    return;
                }

                group.close_pane(pane_id, ctx);
            }
            Event::CloseRequested => {
                group.close_pane_with_confirmation(pane_id, ctx);
            }
            Event::Pane(pane_event) => group.handle_pane_event(pane_id, pane_event, ctx),
            Event::BlockListCleared => {
                // Capture CMD-K to clear blocks here so we could remove
                // all the associated blocks stored in the history.
                if let Some(terminal_pane) = group.terminal_session_by_id(pane_id) {
                    terminal_pane.delete_blocks(ctx);
                }
            }
            Event::SendNotification(notification) => {
                ctx.emit(pane_group::Event::SendNotification {
                    notification: notification.clone(),
                    pane_id,
                })
            }
            Event::PluggableNotification { title, body } => {
                let message = if let Some(t) = title {
                    format!("{t}: {body}")
                } else {
                    body.clone()
                };
                ctx.emit(pane_group::Event::ShowToast {
                    message,
                    flavor: ToastFlavor::Default,
                    pane_id: Some(pane_id),
                })
            }
            Event::AppStateChanged => {
                ctx.emit(pane_group::Event::AppStateChanged);
            }
            Event::BlockCompleted { block, is_local } => {
                match group.terminal_session_by_id(pane_id) {
                    Some(pane) => {
                        if *GeneralSettings::as_ref(ctx).restore_session
                            && AppExecutionMode::as_ref(ctx).can_save_session()
                            && let Some(sender) = &group.model_event_sender
                        {
                            let block_completed_event = ModelEvent::SaveBlock(BlockCompleted {
                                pane_id: pane.session_uuid(),
                                block: block.clone(),
                                is_local: *is_local,
                            });

                            let sender_clone = sender.clone();
                            let _ = ctx.spawn(
                                async move {
                                    // Sending over a sync sender can block the current thread, so we do this async.
                                    sender_clone.send(block_completed_event)
                                },
                                move |_, res, _| {
                                    if let Err(err) = res {
                                        report_error!(
                                            anyhow::Error::new(err)
                                                .context("Error sending block completed event"),
                                            extra: { "terminal_pane_id" => ?terminal_pane_id }
                                        );
                                    }
                                },
                            );
                        }
                        ctx.emit(pane_group::Event::ActiveSessionChanged);
                    }
                    None => {
                        report_error!(
                            "Could not find uuid for terminal id",
                            extra: { "terminal_pane_id" => ?terminal_pane_id }
                        );
                    }
                };
            }
            Event::SessionBootstrapped => {
                ctx.emit(pane_group::Event::ActiveSessionChanged);
            }
            Event::OpenSettings(section) => {
                ctx.emit(pane_group::Event::OpenSettings(*section));
            }
            #[cfg(not(target_family = "wasm"))]
            Event::OpenPluginInstructionsPane(agent, kind) => {
                ctx.emit(pane_group::Event::OpenPluginInstructionsPane(*agent, *kind));
            }
            Event::SyncInput(sync_event) => {
                if SyncedInputState::as_ref(ctx)
                    .should_sync_this_pane_group(ctx.view_id(), ctx.window_id())
                {
                    ctx.emit(pane_group::Event::SyncInput(sync_event.clone()));
                }
            }
            Event::ShowCommandSearch(options) => {
                ctx.emit(pane_group::Event::ShowCommandSearch(options.clone()));
            }
            Event::TerminalViewStateChanged => {
                ctx.emit(pane_group::Event::TerminalViewStateChanged);
            }
            Event::OpenPromptEditor => {
                ctx.emit(pane_group::Event::OpenPromptEditor);
            }
            Event::OpenAgentToolbarEditor => {
                ctx.emit(pane_group::Event::OpenAgentToolbarEditor);
            }
            Event::OpenCLIAgentToolbarEditor => {
                ctx.emit(pane_group::Event::OpenCLIAgentToolbarEditor);
            }
            Event::OpenFileInWarp { path, session } => {
                ctx.emit(pane_group::Event::OpenFileInWarp {
                    path: LocalOrRemotePath::Local(path.clone()),
                    session: session.clone(),
                });
            }
            #[cfg(feature = "local_fs")]
            Event::PreviewCodeInWarp { source } => {
                ctx.emit(pane_group::Event::PreviewCodeInWarp {
                    source: source.clone(),
                });
            }
            #[cfg(feature = "local_fs")]
            Event::OpenCodeInWarp { source, layout } => {
                ctx.emit(pane_group::Event::OpenCodeInWarp {
                    source: source.clone(),
                    layout: *layout,
                    line_col: if let CodeSource::Link { range_start, .. } = source {
                        *range_start
                    } else {
                        None
                    },
                });
            }
            Event::OpenCodeDiff { view } => {
                ctx.emit(pane_group::Event::OpenCodeDiff { view: view.clone() });
            }
            Event::OpenCodeReviewPane(arg) => {
                ctx.emit(pane_group::Event::OpenCodeReviewPane(arg.clone()));
            }
            Event::OpenCodeReviewPaneAndScrollToComment {
                open_code_review,
                comment,
                diff_mode,
            } => {
                ctx.emit(pane_group::Event::OpenCodeReviewPaneAndScrollToComment {
                    open_code_review: open_code_review.clone(),
                    comment: comment.clone(),
                    diff_mode: diff_mode.clone(),
                });
            }
            Event::ImportAllCodeReviewComments {
                open_code_review,
                comments,
                diff_mode,
            } => {
                ctx.emit(pane_group::Event::ImportAllCodeReviewComments {
                    open_code_review: open_code_review.clone(),
                    comments: comments.clone(),
                    diff_mode: diff_mode.clone(),
                });
            }
            Event::ToggleCodeReviewPane(arg) => {
                ctx.emit(pane_group::Event::ToggleCodeReviewPane(arg.clone()));
            }
            Event::FocusSession => {
                group.focus_pane(terminal_pane_id.into(), true, ctx);
                ctx.emit(pane_group::Event::FocusPaneGroup);
            }
            Event::OpenWarpDriveObjectInPane(uid) => {
                ctx.emit(pane_group::Event::OpenWarpDriveObjectInPane(uid.clone()));
            }
            Event::OpenSuggestedAgentModeWorkflowModal { workflow_and_id } => {
                ctx.emit(pane_group::Event::OpenSuggestedAgentModeWorkflowModal {
                    workflow_and_id: workflow_and_id.clone(),
                });
            }
            Event::OpenSuggestedRuleDialog { rule_and_id } => {
                ctx.emit(pane_group::Event::OpenSuggestedRuleModal {
                    rule_and_id: rule_and_id.clone(),
                });
            }
            Event::OpenAIFactCollection { sync_id } => {
                ctx.emit(pane_group::Event::OpenAIFactCollection { sync_id: *sync_id });
            }
            Event::SummarizationCancelDialogToggled { is_open } => {
                group.terminal_with_open_summarization_dialog = is_open.then_some(terminal_pane_id);
                ctx.notify();
            }
            Event::EnvironmentSetupModeSelectorToggled { is_open } => {
                group.pane_with_open_environment_setup_mode_selector = is_open.then_some(pane_id);
                ctx.notify();
            }
            #[cfg(feature = "local_fs")]
            Event::OpenFileWithTarget {
                path,
                target,
                line_col,
            } => {
                ctx.emit(pane_group::Event::OpenFileWithTarget {
                    path: path.clone(),
                    target: target.clone(),
                    line_col: *line_col,
                });
            }
            Event::CopyFileToRemote { command, upload_id } => {
                let new_pane_id = group.insert_terminal_pane(
                    Direction::Right,
                    pane_id,
                    None, /*chosen_shell*/
                    ctx,
                );

                group.hide_pane_for_job(new_pane_id.into(), ctx);

                let new_terminal_view = group
                    .active_session_view(ctx)
                    .expect("should have new terminal view");
                new_terminal_view.update(ctx, |terminal_view, ctx| {
                    terminal_view.set_pending_command(command, ctx);
                    terminal_view.set_is_ssh_uploader(true);
                });

                ctx.emit(pane_group::Event::FileUploadCommand {
                    upload_id: *upload_id,
                    command: command.to_owned(),
                    remote_pane_id: terminal_pane_id,
                    local_pane_id: new_pane_id,
                });

                group.focus_pane(pane_id, true, ctx);
            }
            Event::FileUploadPasswordPending => {
                ctx.emit(pane_group::Event::FileUploadPasswordPending {
                    local_pane_id: terminal_pane_id,
                });
            }
            Event::OpenConversationHistory => {
                ctx.emit(OpenConversationHistory);
            }
            Event::FileUploadFinished(exit_code) => {
                ctx.emit(pane_group::Event::FileUploadFinished {
                    local_pane_id: terminal_pane_id,
                    exit_code: *exit_code,
                });

                // Each upload spawns its own new terminal pane. Once an upload
                // has finished, we know that its terminal session will no
                // longer be responsible for any UI-based uploads.
                if let Some(uploader_terminal_view) =
                    group.terminal_view_from_pane_id(terminal_pane_id, ctx)
                {
                    uploader_terminal_view.update(ctx, |terminal_view, _ctx| {
                        terminal_view.set_is_ssh_uploader(false);
                    });
                }
            }
            Event::OpenFileUploadSession(upload_id) => {
                ctx.emit(pane_group::Event::OpenFileUploadSession {
                    remote_pane_id: terminal_pane_id,
                    upload_id: *upload_id,
                })
            }
            Event::TerminateFileUploadSession(upload_id) => {
                ctx.emit(pane_group::Event::TerminateFileUploadSession {
                    remote_pane_id: terminal_pane_id,
                    upload_id: *upload_id,
                })
            }
            Event::OpenFilesPalette { source } => {
                ctx.emit(pane_group::Event::OpenFilesPalette { source: *source })
            }
            Event::OpenAddRulePane => {
                ctx.emit(crate::pane_group::Event::OpenAddRulePane);
            }
            Event::OpenRulesPane => {
                ctx.emit(crate::pane_group::Event::OpenAIFactCollection { sync_id: None });
            }
            Event::OpenEnvironmentManagementPane => {
                ctx.emit(crate::pane_group::Event::OpenEnvironmentManagementPane);
            }
            #[cfg(feature = "local_fs")]
            Event::FileRenamed { old_path, new_path } => {
                ctx.emit(pane_group::Event::FileRenamed {
                    old_path: old_path.clone(),
                    new_path: new_path.clone(),
                });
            }
            #[cfg(feature = "local_fs")]
            Event::FileDeleted { path } => {
                ctx.emit(pane_group::Event::FileDeleted { path: path.clone() });
            }
            Event::ToggleLeftPanel {
                target_view,
                force_open,
            } => {
                ctx.emit(pane_group::Event::ToggleLeftPanel {
                    target_view: *target_view,
                    force_open: *force_open,
                });
            }
            Event::ToggleAIDocumentPane {
                document_id,
                document_version,
            } => {
                if let Some(conversation_id) =
                    crate::ai::document::ai_document_model::AIDocumentModel::as_ref(ctx)
                        .get_conversation_id_for_document_id(document_id)
                {
                    group.toggle_ai_document_pane(
                        conversation_id,
                        *document_id,
                        *document_version,
                        ctx,
                    );
                }
            }
            Event::HideAIDocumentPanes => {
                group.close_all_ai_document_panes(ctx);
            }
            Event::OpenAIDocumentPane {
                document_id,
                document_version,
                is_auto_open,
            } => {
                let should_open = if *is_auto_open {
                    // Auto-open: only open if there's already a visible plan pane
                    // (to replace it with the newest plan) or if there's enough space.
                    let has_visible_ai_doc_pane = group
                        .ai_document_panes()
                        .any(|pane_id| !group.is_pane_hidden_for_close(pane_id));

                    has_visible_ai_doc_pane
                        || group
                            .terminal_view_from_pane_id(terminal_pane_id, ctx)
                            .is_some_and(|tv| tv.as_ref(ctx).can_auto_open_panel())
                } else {
                    // User-triggered: always open.
                    true
                };

                if should_open
                    && let Some(conversation_id) =
                        crate::ai::document::ai_document_model::AIDocumentModel::as_ref(ctx)
                            .get_conversation_id_for_document_id(document_id)
                {
                    group.open_ai_document_pane(
                        conversation_id,
                        *document_id,
                        *document_version,
                        ctx,
                    );
                }
            }
            Event::OpenAgentProfileEditor { profile_id } => {
                ctx.emit(pane_group::Event::OpenAgentProfileEditor {
                    profile_id: profile_id.clone(),
                });
            }
            Event::InsertCodeReviewComments {
                repo_path,
                comments,
                diff_mode,
                open_code_review,
            } => {
                ctx.emit(pane_group::Event::InsertCodeReviewComments {
                    repo_path: repo_path.clone(),
                    comments: comments.to_owned(),
                    diff_mode: diff_mode.to_owned(),
                    open_code_review: open_code_review.clone(),
                });
            }
            Event::RevealChildAgent { conversation_id } => {
                // Routed through the swap mechanism to land all reveal cases in one path.
                if group.ensure_hidden_child_agent_pane_for_conversation(*conversation_id, ctx) {
                    group.swap_active_pane_to_conversation(pane_id, *conversation_id, ctx);
                } else {
                    log::warn!(
                        "RevealChildAgent: failed to materialize child conversation {conversation_id:?}"
                    );
                }
            }
            Event::SwapPaneToConversation { conversation_id } => {
                // Swap visibility instead of cloning so in-flight state in the
                // target pane is preserved.
                if group.ensure_hidden_child_agent_pane_for_conversation(*conversation_id, ctx) {
                    group.swap_active_pane_to_conversation(pane_id, *conversation_id, ctx);
                } else {
                    log::warn!(
                        "SwapPaneToConversation: failed to materialize conversation {conversation_id:?}"
                    );
                }
            }
            Event::OpenChildAgentInNewTab { conversation_id } => {
                // Pane group can't add tabs; forward to the workspace.
                if group.ensure_hidden_child_agent_pane_for_conversation(*conversation_id, ctx) {
                    ctx.emit(pane_group::Event::OpenChildAgentInNewTab {
                        conversation_id: *conversation_id,
                    });
                } else {
                    log::warn!(
                        "OpenChildAgentInNewTab: failed to materialize child conversation {conversation_id:?}"
                    );
                }
            }
            Event::OpenChildAgentInNewPane { conversation_id } => {
                // Reuse the existing hidden child pane to preserve in-flight
                // state and the live transcript instead of creating a new view.
                if group.ensure_hidden_child_agent_pane_for_conversation(*conversation_id, ctx) {
                    if group
                        .unhide_child_agent_pane_for_split_off(*conversation_id, ctx)
                        .is_none()
                    {
                        log::warn!(
                            "OpenChildAgentInNewPane: no hidden child pane registered for conversation {conversation_id:?}"
                        );
                    }
                } else {
                    log::warn!(
                        "OpenChildAgentInNewPane: failed to materialize child conversation {conversation_id:?}"
                    );
                }
            }
            Event::StopAgentConversation { conversation_id } => {
                stop_agent_conversation(group, *conversation_id, ctx);
            }
            Event::KillAgentConversation { conversation_id } => {
                let source_terminal_view_id = group
                    .terminal_view_from_pane_id(terminal_pane_id, ctx)
                    .map(|terminal_view| terminal_view.id());
                kill_agent_conversation(group, source_terminal_view_id, *conversation_id, ctx);
            }
            Event::StartAgentConversation(request) => {
                dispatch_start_agent_conversation(
                    group,
                    pane_id,
                    terminal_pane_id,
                    request.clone(),
                    ctx,
                );
            }
            _ => {}
        }
    } else {
        log::warn!("Session {terminal_pane_id:?} not found");
    }
}

/// Dispatches a StartAgent request to the appropriate per-mode helper.
/// Each helper echoes the child conversation id back via
/// [`BlocklistAIHistoryModel::record_new_conversation_request_complete`].
#[cfg_attr(target_family = "wasm", allow(unused_variables))]
fn dispatch_start_agent_conversation(
    group: &mut PaneGroup,
    parent_pane_id: PaneId,
    terminal_pane_id: TerminalPaneId,
    request: StartAgentRequest,
    ctx: &mut ViewContext<PaneGroup>,
) {
    let team_context = UserWorkspaces::as_ref(ctx).team_context_for_operation(ctx);
    if !request.request_team_scope.matches_scope(&team_context) {
        let _ = create_error_child_agent_conversation(
            group,
            ErrorChildAgentConversationRequest {
                parent_pane_id,
                name: request.name,
                parent_conversation_id: request.parent_conversation_id,
                request_id: Some(request.id),
                orchestration_harness: None,
                error_message: TEAM_CHANGED_DURING_CHILD_LAUNCH_ERROR.to_string(),
            },
            ctx,
        );
        return;
    }
    match request.execution_mode.clone() {
        #[cfg(not(target_family = "wasm"))]
        StartAgentExecutionMode::Local {
            harness_type: None,
            model_id,
        } => {
            launch_local_no_harness_child(parent_pane_id, request, model_id, team_context, ctx);
        }
        #[cfg(not(target_family = "wasm"))]
        StartAgentExecutionMode::Local {
            harness_type: Some(harness_type),
            model_id,
        } => {
            launch_local_harness_child(
                group,
                parent_pane_id,
                terminal_pane_id,
                request,
                harness_type,
                model_id,
                team_context,
                ctx,
            );
        }
        #[cfg(target_family = "wasm")]
        StartAgentExecutionMode::Local { .. } => {
            let _ = create_error_child_agent_conversation(
                group,
                ErrorChildAgentConversationRequest {
                    parent_pane_id,
                    name: request.name,
                    parent_conversation_id: request.parent_conversation_id,
                    request_id: Some(request.id),
                    orchestration_harness: None,
                    error_message: "Local child agents are not supported in WASM builds."
                        .to_string(),
                },
                ctx,
            );
        }
        StartAgentExecutionMode::Remote { .. } => {
            let _ = create_error_child_agent_conversation(
                group,
                ErrorChildAgentConversationRequest {
                    parent_pane_id,
                    name: request.name,
                    parent_conversation_id: request.parent_conversation_id,
                    request_id: Some(request.id),
                    orchestration_harness: None,
                    error_message: "Cloud agents are not supported in Torch.".to_string(),
                },
                ctx,
            );
        }
    }
}

/// Sets up a hidden child pane for a Local-no-harness (Oz) agent and
/// dispatches the prompt. Asynchronously creates the server-side `ai_tasks`
/// row via `AIClient::create_agent_task` at dispatch time, mirroring the
/// third-party-harness path (see [`launch_local_harness_child`]). The
/// resulting `task_id` is stamped onto the child's `AIConversation` (so the
/// per-`Network` share-reporter in `local_tty/terminal_manager.rs` can link
/// the shared session id to the child task once the shell bootstraps) and
/// onto the child's `BlocklistAIController` via the
/// `HiddenChildAgentTaskContext` (so the agent UI reflects it). On failure
/// the child surfaces as an error conversation instead.
///
/// Gated to non-wasm because `ServerApiProvider` is `cfg(not(wasm))`-only.
/// `dispatch_start_agent_conversation`'s wasm wildcard arm routes the Oz
/// path through `create_error_child_agent_conversation` instead.
#[cfg(not(target_family = "wasm"))]
fn launch_local_no_harness_child(
    parent_pane_id: PaneId,
    request: StartAgentRequest,
    model_id: Option<String>,
    team_context: TeamContextForOperation,
    ctx: &mut ViewContext<PaneGroup>,
) {
    let request_id = request.id;
    let parent_conversation_id = request.parent_conversation_id;
    let prompt = request.prompt.clone();

    let request_team_scope = request.request_team_scope;

    let launch = prepare_local_oz_child_launch(
        &request.name,
        &request.prompt,
        request.parent_run_id.as_deref(),
        &request.metadata,
        request_team_scope,
        ctx,
    );
    let _ = ctx.spawn(launch, move |group, result, ctx| match result {
        Ok(prepared) => {
            let child_task_id = prepared.task_id;

            match create_hidden_child_agent_conversation(
                group,
                HiddenChildAgentConversationRequest {
                    parent_pane_id,
                    name: prepared.conversation_name.clone(),
                    parent_conversation_id,
                    orchestration_harness: Some(Harness::Oz),
                    env_vars: HashMap::new(),
                    task_context: Some(HiddenChildAgentTaskContext {
                        task_id: child_task_id,
                        working_dir: None,
                    }),
                },
                &team_context,
                ctx,
            ) {
                Some(HiddenChildAgentConversation {
                    terminal_view: new_terminal_view,
                    terminal_view_id,
                    conversation_id,
                    ..
                }) => {
                    apply_child_agent_model_override(
                        &team_context,
                        terminal_view_id,
                        model_id.as_deref(),
                        ctx,
                    );
                    finish_local_oz_child_conversation(
                        conversation_id,
                        terminal_view_id,
                        child_task_id,
                        request_id,
                        ctx,
                    );

                    new_terminal_view.update(ctx, |terminal_view, ctx| {
                        terminal_view
                            .ai_controller()
                            .update(ctx, |controller, ctx| {
                                controller.send_agent_query_in_conversation(
                                    prompt.clone(),
                                    conversation_id,
                                    ctx,
                                );
                            });

                        terminal_view.enter_agent_view(
                            None,
                            Some(conversation_id),
                            AgentViewEntryOrigin::ChildAgent,
                            ctx,
                        );
                    });
                }
                _ => {
                    let _ = create_error_child_agent_conversation(
                        group,
                        ErrorChildAgentConversationRequest {
                            parent_pane_id,
                            name: prepared.conversation_name,
                            parent_conversation_id,
                            request_id: Some(request_id),
                            orchestration_harness: Some(Harness::Oz),
                            error_message:
                                "Failed to create a hidden pane for the local child agent."
                                    .to_string(),
                        },
                        ctx,
                    );
                }
            }
        }
        Err(error) => {
            let _ = create_error_child_agent_conversation(
                group,
                ErrorChildAgentConversationRequest {
                    parent_pane_id,
                    name: normalize_orchestrator_agent_name(&request.name).unwrap_or_default(),
                    parent_conversation_id,
                    request_id: Some(request_id),
                    orchestration_harness: Some(Harness::Oz),
                    error_message: format!("Failed to create local child task: {error}"),
                },
                ctx,
            );
        }
    });
}

/// Asynchronously prepares a local harness launch, then creates the
/// hidden child pane and executes the launch command.
#[cfg(not(target_family = "wasm"))]
#[allow(clippy::too_many_arguments)]
fn launch_local_harness_child(
    group: &mut PaneGroup,
    parent_pane_id: PaneId,
    terminal_pane_id: TerminalPaneId,
    request: StartAgentRequest,
    harness_type: String,
    model_id: Option<String>,
    team_context: TeamContextForOperation,
    ctx: &mut ViewContext<PaneGroup>,
) {
    let startup_directory = group.startup_path_for_new_session(Some(terminal_pane_id), ctx);
    let ai_client = ServerApiProvider::handle(ctx).as_ref(ctx).get_ai_client();
    let request_id = request.id;
    let agent_name = normalize_orchestrator_agent_name(&request.name);
    let request_name = agent_name.clone().unwrap_or_default();
    let parent_conversation_id = request.parent_conversation_id;
    let parent_run_id = request.parent_run_id.clone();
    let prompt = request.prompt.clone();
    let orchestration_harness =
        Harness::parse_orchestration_harness(&harness_type).unwrap_or(Harness::Unknown);
    let shell_type = group
        .terminal_view_from_pane_id(parent_pane_id, ctx)
        .and_then(|terminal_view| terminal_view.as_ref(ctx).active_session_shell_type(ctx));

    let model_id_for_harness_env = model_id.clone();
    let agent_name_for_task = agent_name.clone();
    let request_team_scope = request.request_team_scope;
    let metadata = request.metadata.clone();
    let _ = ctx.spawn(
        async move {
            prepare_local_harness_child_launch(
                prompt,
                harness_type,
                model_id_for_harness_env,
                parent_run_id,
                agent_name_for_task,
                metadata,
                shell_type,
                startup_directory,
                ai_client,
                request_team_scope,
            )
            .await
        },
        move |group, result, ctx| match result {
            Ok(launch) => {
                let PreparedLocalHarnessLaunch {
                    command,
                    env_vars,
                    run_id,
                    task_id,
                } = launch;
                match create_hidden_child_agent_conversation(
                    group,
                    HiddenChildAgentConversationRequest {
                        parent_pane_id,
                        name: request_name.clone(),
                        parent_conversation_id,
                        orchestration_harness: Some(orchestration_harness),
                        env_vars,
                        task_context: None,
                    },
                    &team_context,
                    ctx,
                ) {
                    Some(HiddenChildAgentConversation {
                        terminal_view: new_terminal_view,
                        terminal_view_id,
                        conversation_id,
                        ..
                    }) => {
                        apply_child_agent_model_override(
                            &team_context,
                            terminal_view_id,
                            model_id.as_deref(),
                            ctx,
                        );

                        BlocklistAIHistoryModel::handle(ctx).update(ctx, |model, ctx| {
                            model.record_new_conversation_request_complete(
                                request_id,
                                conversation_id,
                                ctx,
                            );
                        });

                        BlocklistAIHistoryModel::handle(ctx).update(ctx, |history_model, ctx| {
                            history_model.assign_run_id_for_conversation(
                                conversation_id,
                                run_id,
                                Some(task_id),
                                terminal_view_id,
                                ctx,
                            );
                        });

                        new_terminal_view.update(ctx, |terminal_view, ctx| {
                            terminal_view.execute_command_or_set_pending(&command, ctx);
                            terminal_view.enter_agent_view(
                                None,
                                Some(conversation_id),
                                AgentViewEntryOrigin::ChildAgent,
                                ctx,
                            );
                        });
                    }
                    _ => {
                        let _ = create_error_child_agent_conversation(
                            group,
                            ErrorChildAgentConversationRequest {
                                parent_pane_id,
                                name: request_name,
                                parent_conversation_id,
                                request_id: Some(request_id),
                                orchestration_harness: Some(orchestration_harness),
                                error_message:
                                    "Failed to create a hidden pane for the local child harness."
                                        .to_string(),
                            },
                            ctx,
                        );
                    }
                }
            }
            Err(error_message) => {
                let _ = create_error_child_agent_conversation(
                    group,
                    ErrorChildAgentConversationRequest {
                        parent_pane_id,
                        name: request_name,
                        parent_conversation_id,
                        request_id: Some(request_id),
                        orchestration_harness: Some(orchestration_harness),
                        error_message,
                    },
                    ctx,
                );
            }
        },
    );
}

#[cfg(feature = "local_fs")]
fn handle_ai_history_event(
    event: &BlocklistAIHistoryEvent,
    terminal_view_id: EntityId,
    terminal_pane_id: TerminalPaneId,
    model_event_sender: SyncSender<ModelEvent>,
    ctx: &mut ViewContext<PaneGroup>,
) {
    use crate::ai::blocklist::maybe_build_ai_query_upsert_event;

    if event
        .terminal_surface_id()
        .is_some_and(|id| id != terminal_view_id)
    {
        return;
    }

    match event {
        BlocklistAIHistoryEvent::AppendedExchange { .. }
        | BlocklistAIHistoryEvent::UpdatedStreamingExchange { .. } => {
            // Check if session restoration is enabled.
            if !*GeneralSettings::as_ref(ctx).restore_session
                || !AppExecutionMode::as_ref(ctx).can_save_session()
            {
                return;
            }
            let Some(upsert_ai_query_event) =
                maybe_build_ai_query_upsert_event(event, terminal_view_id, ctx)
            else {
                return;
            };
            let _ = ctx.spawn(
                // Sending over a sync sender can block the current thread, so we
                // do this async.
                async move { model_event_sender.send(upsert_ai_query_event) },
                move |_, res, _| {
                    if let Err(err) = res {
                        report_error!(
                            anyhow::Error::new(err).context("Error sending upsert AI query event"),
                            extra: { "terminal_pane_id" => ?terminal_pane_id }
                        );
                    }
                },
            );
        }
        BlocklistAIHistoryEvent::ClearedConversationsForTerminalSurface { .. }
        | BlocklistAIHistoryEvent::ClearedActiveConversation { .. } => {
            ctx.emit(pane_group::Event::InvalidatedActiveConversation);
        }
        BlocklistAIHistoryEvent::RemoveConversation {
            conversation_id, ..
        } => {
            let conversation_id = conversation_id.to_string();
            // On remove, delete all related AI query and multi-agent conversation data for this conversation.
            let _ = ctx.spawn(
                async move {
                    model_event_sender.send(ModelEvent::DeleteAIConversation {
                        conversation_id: conversation_id.clone(),
                    })?;
                    model_event_sender.send(ModelEvent::DeleteMultiAgentConversations {
                        conversation_ids: vec![conversation_id],
                    })
                },
                |_, res, _| {
                    if let Err(err) = res {
                        report_error!(
                            anyhow::Error::new(err)
                                .context("Error sending delete events for conversation")
                        );
                    }
                },
            );
        }
        // DeletedConversation SQL cleanup is handled directly in delete_conversation().
        BlocklistAIHistoryEvent::DeletedConversation { .. }
        | BlocklistAIHistoryEvent::StartedNewConversation { .. }
        | BlocklistAIHistoryEvent::UpdatedConversationStatus { .. }
        | BlocklistAIHistoryEvent::ReassignedExchange { .. }
        | BlocklistAIHistoryEvent::SetActiveConversation { .. }
        | BlocklistAIHistoryEvent::UpdatedTodoList { .. }
        | BlocklistAIHistoryEvent::UpdatedAutoexecuteOverride { .. }
        | BlocklistAIHistoryEvent::SplitConversation { .. }
        | BlocklistAIHistoryEvent::RestoredConversations { .. }
        | BlocklistAIHistoryEvent::CreatedSubtask { .. }
        | BlocklistAIHistoryEvent::UpgradedTask { .. }
        | BlocklistAIHistoryEvent::UpdatedConversationTitle { .. }
        | BlocklistAIHistoryEvent::UpdatedConversationMetadata { .. }
        | BlocklistAIHistoryEvent::UpdatedConversationArtifacts { .. }
        | BlocklistAIHistoryEvent::ConversationServerTokenAssigned { .. }
        | BlocklistAIHistoryEvent::ConversationTransferredBetweenTerminalSurfaces { .. }
        | BlocklistAIHistoryEvent::NewConversationRequestComplete { .. }
        | BlocklistAIHistoryEvent::OrchestrationConfigUpdated { .. }
        | BlocklistAIHistoryEvent::ConversationUsageMetadataUpdated { .. } => (),
    }
}

fn resumable_agent_session(
    terminal_view_id: EntityId,
    app: &AppContext,
) -> Option<ResumableAgentSession> {
    let session = CLIAgentSessionsModel::as_ref(app).session(terminal_view_id)?;
    ResumableAgentSession::new(
        session.agent,
        session.session_context.session_id.as_deref()?,
    )
}
