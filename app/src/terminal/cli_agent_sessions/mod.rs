pub mod event;
pub mod listener;
pub(crate) mod notification;
#[cfg(not(target_family = "wasm"))]
pub(crate) mod plugin_manager;
pub mod resume;

use std::collections::{HashMap, HashSet};

use event::{CLIAgentEvent, CLIAgentEventSource, CLIAgentEventType};
use instant::Instant;
use torch_core::execution_mode::AppExecutionMode;
use torchui::{Entity, EntityId, ModelContext, ModelHandle, SingletonEntity};

use self::listener::CLIAgentSessionListener;
use super::CLIAgent;
use crate::agent_status::ConversationStatus;

const NEEDS_INPUT_STATUS_MESSAGE: &str = "Agent requires user input";

/// What caused a session to become [`CLIAgentSessionStatus::Blocked`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockedSource {
    PermissionRequest,
    QuestionAsked,
    NeedsInput,
}

/// Status of a tracked CLI agent session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CLIAgentSessionStatus {
    InProgress,
    Success,
    Failed {
        error_type: Option<String>,
        message: Option<String>,
    },
    Blocked {
        message: Option<String>,
        source: BlockedSource,
    },
}

impl CLIAgentSessionStatus {
    pub fn to_conversation_status(&self) -> ConversationStatus {
        match self {
            CLIAgentSessionStatus::InProgress => ConversationStatus::InProgress,
            CLIAgentSessionStatus::Success => ConversationStatus::Success,
            CLIAgentSessionStatus::Failed { .. } => ConversationStatus::Error,
            CLIAgentSessionStatus::Blocked { message, .. } => ConversationStatus::Blocked {
                blocked_action: message.clone().unwrap_or_default(),
            },
        }
    }
}

/// Rich context accumulated from CLI agent session events.
#[derive(Debug, Clone, Default)]
pub struct CLIAgentSessionContext {
    pub cwd: Option<String>,
    pub project: Option<String>,
    pub session_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input_preview: Option<String>,
    pub summary: Option<String>,
    pub query: Option<String>,
    pub response: Option<String>,
}

/// State of the rich input editor for composing a prompt to send to a CLI agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CLIAgentInputState {
    /// The rich input editor is not open.
    Closed,
    /// The rich input editor is open.
    Open,
}

impl CLIAgentSessionContext {
    pub(crate) fn display_title(&self) -> Option<String> {
        self.latest_user_prompt().or_else(|| self.title_like_text())
    }

    pub(crate) fn latest_user_prompt(&self) -> Option<String> {
        self.query
            .as_deref()
            .map(str::trim)
            .filter(|query| !query.is_empty())
            .map(str::to_owned)
    }

    /// Returns summary text suitable as a fallback title when no user prompt is available.
    pub(crate) fn title_like_text(&self) -> Option<String> {
        self.summary
            .as_deref()
            .map(str::trim)
            .filter(|summary| !summary.is_empty())
            .map(str::to_owned)
    }
}

/// A tracked CLI agent session.
#[derive(Debug, Clone)]
pub struct CLIAgentSession {
    pub agent: CLIAgent,
    pub status: CLIAgentSessionStatus,
    pub session_context: CLIAgentSessionContext,
    /// Rich input editor state.
    pub input_state: CLIAgentInputState,
    /// Whether status-driven auto-toggle is enabled for this session.
    pub should_auto_toggle_input: bool,
    /// Event listener for plugin-backed sessions or Codex OSC9 fallback.
    /// `None` for non-Codex sessions created by command detection alone.
    /// Dropping this handle cleans up the listener's PTY event subscription.
    pub listener: Option<ModelHandle<CLIAgentSessionListener>>,
    /// The plugin version reported by structured plugin events.
    /// `None` if the plugin predates version reporting or Codex is using OSC9 fallback.
    pub plugin_version: Option<String>,
    /// `None` when the session is local.
    /// `Some("user@hostname")` when running over SSH (warpified or legacy).
    /// Used as a key for per-host plugin install failure tracking.
    pub remote_host: Option<String>,
    /// Draft text saved from the rich input composer when it was closed.
    /// Restored into the editor when the composer is reopened.
    pub draft_text: Option<String>,
    /// When the session was detected via a custom toolbar command pattern,
    /// the first word of the command (the binary/alias the user typed).
    /// Used to customize plugin instructions and force manual install mode.
    pub custom_command_prefix: Option<String>,
    /// Set once the session has received any structured OSC 777 (rich)
    /// notification. Codex's OSC 9 fallback never sets it, so this is the
    /// single source of truth for whether the session is plugin-backed.
    pub received_rich_notification: bool,
}

impl CLIAgentSession {
    pub fn is_remote(&self) -> bool {
        self.remote_host.is_some()
    }

    /// Whether the session surfaces trustworthy fine-grained status
    /// (in-progress / blocked / success). True only after receiving a rich OSC
    /// 777 notification. Codex's OSC 9 fallback emits only opaque `Stop`
    /// notifications and never sets `received_rich_notification`, so it does
    /// not qualify. Synthetic listener registration also does not qualify until
    /// an actual rich notification arrives.
    pub fn supports_rich_status(&self) -> bool {
        self.received_rich_notification
    }

    /// Clears state populated by `PermissionRequest`. Called whenever the
    /// session leaves the permission flow (the user replied, a blocking tool
    /// completed, a new prompt is submitted, or the session ends successfully)
    /// so the permission summary doesn't leak into later UI surfaces — most
    /// visibly the tab title, which can fall back to `summary` when `query`
    /// is unset.
    fn clear_permission_scoped_state(&mut self) {
        self.session_context.summary = None;
        self.session_context.tool_name = None;
        self.session_context.tool_input_preview = None;
    }

    /// Applies an event to this session, updating context and status.
    /// Returns the new status if it changed, or `None` if the event was irrelevant.
    fn apply_event(&mut self, event: &CLIAgentEvent) -> Option<CLIAgentSessionStatus> {
        self.session_context.cwd = event.cwd.clone().or(self.session_context.cwd.take());
        self.session_context.project = event
            .project
            .clone()
            .or(self.session_context.project.take());
        self.session_context.session_id = event
            .session_id
            .clone()
            .or(self.session_context.session_id.take());

        let new_status = match &event.event {
            CLIAgentEventType::PromptSubmit => {
                self.session_context.query = event.payload.query.clone();
                self.session_context.response = None;
                self.clear_permission_scoped_state();
                CLIAgentSessionStatus::InProgress
            }
            CLIAgentEventType::ToolComplete => {
                if !matches!(self.status, CLIAgentSessionStatus::Blocked { .. }) {
                    return None;
                }
                self.clear_permission_scoped_state();
                CLIAgentSessionStatus::InProgress
            }
            CLIAgentEventType::Stop => {
                self.session_context.query = event.payload.query.clone();
                self.session_context.response = event.payload.response.clone();
                self.clear_permission_scoped_state();
                // The agent will wake itself again once that work finishes, so this turn
                // boundary is a pause rather than the end of the session.
                if event.payload.has_pending_background_work() {
                    return None;
                }
                CLIAgentSessionStatus::Success
            }
            CLIAgentEventType::StopFailure => {
                self.session_context.query = event.payload.query.clone();
                self.session_context.response = event.payload.response.clone();
                self.clear_permission_scoped_state();
                CLIAgentSessionStatus::Failed {
                    error_type: event.payload.error_type.clone(),
                    message: event.payload.response.clone(),
                }
            }
            CLIAgentEventType::PermissionRequest => {
                self.session_context.summary = event.payload.summary.clone();
                self.session_context.tool_name = event.payload.tool_name.clone();
                self.session_context.tool_input_preview = event.payload.tool_input_preview.clone();
                CLIAgentSessionStatus::Blocked {
                    message: event.payload.summary.clone(),
                    source: BlockedSource::PermissionRequest,
                }
            }
            CLIAgentEventType::QuestionAsked => CLIAgentSessionStatus::Blocked {
                message: event
                    .payload
                    .summary
                    .clone()
                    .or_else(|| Some("Waiting for your answer".to_owned())),
                source: BlockedSource::QuestionAsked,
            },
            CLIAgentEventType::NeedsInput => CLIAgentSessionStatus::Blocked {
                message: event
                    .payload
                    .summary
                    .clone()
                    .or_else(|| Some(NEEDS_INPUT_STATUS_MESSAGE.to_owned())),
                source: BlockedSource::NeedsInput,
            },
            CLIAgentEventType::PermissionReplied => {
                if !matches!(self.status, CLIAgentSessionStatus::Blocked { .. }) {
                    return None;
                }
                self.clear_permission_scoped_state();
                CLIAgentSessionStatus::InProgress
            }
            // IdlePrompt means the agent is sitting at its prompt waiting for input.
            // This should not affect status — otherwise it would override Success after a Stop event.
            CLIAgentEventType::IdlePrompt => return None,
            CLIAgentEventType::SessionStart => {
                self.plugin_version = event.payload.plugin_version.clone();
                return None;
            }
            CLIAgentEventType::Unknown(_) => return None,
        };

        self.status = new_status.clone();
        Some(new_status)
    }
}

/// Events emitted by `CLIAgentSessionsModel` for subscribers (e.g., `AgentNotificationsModel`).
#[allow(dead_code)] // `agent` fields on Started/InputSessionChanged/Ended are used for logging and future subscribers.
#[derive(Debug, Clone)]
pub enum CLIAgentSessionsModelEvent {
    Started {
        terminal_view_id: EntityId,
        agent: CLIAgent,
    },
    StatusChanged {
        terminal_view_id: EntityId,
        agent: CLIAgent,
        status: CLIAgentSessionStatus,
        is_prompt_submit: bool,
        session_context: Box<CLIAgentSessionContext>,
    },
    InputSessionChanged {
        terminal_view_id: EntityId,
        agent: CLIAgent,
        /// The input state BEFORE this change. When transitioning from
        /// `Open` → `Closed`, contains the saved input config to restore.
        previous_input_state: CLIAgentInputState,
        /// The input state AFTER this change.
        new_input_state: CLIAgentInputState,
    },
    Ended {
        terminal_view_id: EntityId,
        agent: CLIAgent,
    },
    /// The agent session has been updated. Subscribers may use this as a trigger for best-effort
    /// saving of state derived from the agent's session.
    SessionUpdated {
        terminal_view_id: EntityId,
        agent: CLIAgent,
    },
}

impl CLIAgentSessionsModelEvent {
    pub fn terminal_view_id(&self) -> EntityId {
        match self {
            CLIAgentSessionsModelEvent::Started {
                terminal_view_id, ..
            }
            | CLIAgentSessionsModelEvent::StatusChanged {
                terminal_view_id, ..
            }
            | CLIAgentSessionsModelEvent::InputSessionChanged {
                terminal_view_id, ..
            }
            | CLIAgentSessionsModelEvent::Ended {
                terminal_view_id, ..
            }
            | CLIAgentSessionsModelEvent::SessionUpdated {
                terminal_view_id, ..
            } => *terminal_view_id,
        }
    }
}

/// Singleton model that tracks pane-scoped CLI agent state and plugin-enriched session context.
pub struct CLIAgentSessionsModel {
    sessions: HashMap<EntityId, CLIAgentSession>,
    /// Tracks (agent, remote_host) pairs where an auto plugin operation (install or update) has failed.
    /// Shared across all views so failure in one tab is reflected everywhere.
    plugin_auto_failures: HashSet<(CLIAgent, Option<String>)>,
    /// Sessions whose status has come from Codex's OSC 9 fallback notifications.
    fallback_status_sessions: HashSet<EntityId>,
    /// When each session's status last changed.
    status_since: HashMap<EntityId, Instant>,
}

impl Entity for CLIAgentSessionsModel {
    type Event = CLIAgentSessionsModelEvent;
}

impl SingletonEntity for CLIAgentSessionsModel {}

impl CLIAgentSessionsModel {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            plugin_auto_failures: HashSet::new(),
            fallback_status_sessions: HashSet::new(),
            status_since: HashMap::new(),
        }
    }

    pub fn session(&self, terminal_view_id: EntityId) -> Option<&CLIAgentSession> {
        self.sessions.get(&terminal_view_id)
    }

    /// When the session's status last changed.
    pub fn status_since(&self, terminal_view_id: EntityId) -> Option<Instant> {
        self.status_since.get(&terminal_view_id).copied()
    }

    /// Whether the session's status reflects notifications from the agent rather than the
    /// optimistic state set at registration. Unlike `supports_rich_status`, this includes coarse
    /// OSC 9 fallback status (turn complete or approval requested).
    pub fn reports_status(&self, terminal_view_id: EntityId) -> bool {
        self.sessions.get(&terminal_view_id).is_some_and(|session| {
            session.supports_rich_status()
                || self.fallback_status_sessions.contains(&terminal_view_id)
        })
    }

    pub fn blocked_session_count(&self) -> usize {
        self.sessions
            .values()
            .filter(|session| matches!(session.status, CLIAgentSessionStatus::Blocked { .. }))
            .count()
    }

    /// Returns `true` if the rich input editor is currently open for this terminal.
    pub fn is_input_open(&self, terminal_view_id: EntityId) -> bool {
        self.sessions
            .get(&terminal_view_id)
            .is_some_and(|s| matches!(s.input_state, CLIAgentInputState::Open { .. }))
    }

    /// Registers a plugin-backed listener on the session for this terminal.
    ///
    /// If a session for the same agent already exists (e.g. created earlier by
    /// command detection), it is upgraded with the listener and plugin context.
    /// Otherwise a new session is created.
    ///
    /// The optional `cwd` / `project` / `session_id` fields supply initial
    /// context when available (e.g. from a `SessionStart` event). Passing
    /// `None` for all three is fine — happens when the plugin is installed
    /// mid-session and there is no start event to extract context from.
    #[allow(clippy::too_many_arguments)]
    pub fn register_listener(
        &mut self,
        terminal_view_id: EntityId,
        agent: CLIAgent,
        cwd: Option<String>,
        project: Option<String>,
        session_id: Option<String>,
        plugin_version: Option<String>,
        remote_host: Option<String>,
        should_auto_toggle_input: bool,
        listener: ModelHandle<CLIAgentSessionListener>,
        ctx: &mut ModelContext<Self>,
    ) {
        if let Some(session) = self
            .sessions
            .get_mut(&terminal_view_id)
            .filter(|s| s.agent == agent)
        {
            // Upgrade existing session with plugin context.
            session.status = CLIAgentSessionStatus::InProgress;
            self.status_since.insert(terminal_view_id, Instant::now());
            session.listener = Some(listener);
            session.plugin_version = plugin_version;
            session.remote_host = remote_host;
            session.should_auto_toggle_input = should_auto_toggle_input;
            session.session_context.cwd = cwd.or(session.session_context.cwd.take());
            session.session_context.project = project.or(session.session_context.project.take());
            session.session_context.session_id =
                session_id.or(session.session_context.session_id.take());
            return;
        }

        self.set_session(
            terminal_view_id,
            CLIAgentSession {
                agent,
                status: CLIAgentSessionStatus::InProgress,
                session_context: CLIAgentSessionContext {
                    cwd,
                    project,
                    session_id,
                    ..Default::default()
                },
                input_state: CLIAgentInputState::Closed,
                should_auto_toggle_input,
                listener: Some(listener),
                plugin_version,
                remote_host,
                draft_text: None,
                custom_command_prefix: None,
                received_rich_notification: false,
            },
            ctx,
        );
    }

    pub fn remove_session(&mut self, terminal_view_id: EntityId, ctx: &mut ModelContext<Self>) {
        self.fallback_status_sessions.remove(&terminal_view_id);
        self.status_since.remove(&terminal_view_id);
        if let Some(session) = self.sessions.remove(&terminal_view_id) {
            ctx.emit(CLIAgentSessionsModelEvent::Ended {
                terminal_view_id,
                agent: session.agent,
            });
        }
    }

    /// Updates the session's status and context from a parsed CLI agent event.
    /// Rich plugin events latch `received_rich_notification` so rich-status
    /// surfaces stay consistent even if the first event was not SessionStart.
    pub fn update_from_event(
        &mut self,
        terminal_view_id: EntityId,
        event: &CLIAgentEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        if !self.sessions.contains_key(&terminal_view_id) {
            return;
        }

        let session = self
            .sessions
            .get_mut(&terminal_view_id)
            .expect("session presence checked above");

        match event.source {
            CLIAgentEventSource::RichPlugin => session.received_rich_notification = true,
            CLIAgentEventSource::CodexOsc9Fallback => {
                self.fallback_status_sessions.insert(terminal_view_id);
            }
        }

        let event_type = &event.event;
        // Interactive clients keep ignoring this event: only an unattended run has no one to
        // answer the prompt.
        let new_status = if matches!(event.event, CLIAgentEventType::NeedsInput)
            && !AppExecutionMode::as_ref(ctx).is_autonomous()
        {
            None
        } else {
            session.apply_event(event)
        };
        if let Some(new_status) = new_status {
            let agent = session.agent;
            self.status_since.insert(terminal_view_id, Instant::now());
            ctx.emit(CLIAgentSessionsModelEvent::StatusChanged {
                terminal_view_id,
                agent,
                status: new_status,
                is_prompt_submit: matches!(event_type, CLIAgentEventType::PromptSubmit),
                session_context: Box::new(session.session_context.clone()),
            });
        }

        if matches!(
            event_type,
            CLIAgentEventType::SessionStart
                | CLIAgentEventType::PromptSubmit
                | CLIAgentEventType::ToolComplete
        ) {
            ctx.emit(CLIAgentSessionsModelEvent::SessionUpdated {
                terminal_view_id,
                agent: session.agent,
            });
        }
    }

    /// Observes keyboard or paste input the user sent to this session's PTY. A blocked agent
    /// that receives input has been answered, even before its next status event arrives.
    pub fn observe_user_input(&mut self, terminal_view_id: EntityId, ctx: &mut ModelContext<Self>) {
        let Some(session) = self.sessions.get_mut(&terminal_view_id) else {
            return;
        };
        if !matches!(session.status, CLIAgentSessionStatus::Blocked { .. }) {
            return;
        }
        session.clear_permission_scoped_state();
        session.status = CLIAgentSessionStatus::InProgress;
        self.status_since.insert(terminal_view_id, Instant::now());
        ctx.emit(CLIAgentSessionsModelEvent::StatusChanged {
            terminal_view_id,
            agent: session.agent,
            status: CLIAgentSessionStatus::InProgress,
            is_prompt_submit: false,
            session_context: Box::new(session.session_context.clone()),
        });
    }

    pub fn open_input(
        &mut self,
        terminal_view_id: EntityId,
        should_auto_toggle_input: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&terminal_view_id) else {
            return;
        };

        let previous_input_state = session.input_state;
        session.input_state = CLIAgentInputState::Open;
        session.should_auto_toggle_input = should_auto_toggle_input;

        ctx.emit(CLIAgentSessionsModelEvent::InputSessionChanged {
            terminal_view_id,
            agent: session.agent,
            previous_input_state,
            new_input_state: session.input_state,
        });
    }

    pub fn close_input(
        &mut self,
        terminal_view_id: EntityId,
        should_auto_toggle_input: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&terminal_view_id) else {
            return;
        };
        if session.input_state == CLIAgentInputState::Closed {
            return;
        }

        let previous_input_state = session.input_state;
        session.input_state = CLIAgentInputState::Closed;
        session.should_auto_toggle_input = should_auto_toggle_input;
        ctx.emit(CLIAgentSessionsModelEvent::InputSessionChanged {
            terminal_view_id,
            agent: session.agent,
            previous_input_state,
            new_input_state: CLIAgentInputState::Closed,
        });
    }

    pub fn set_session(
        &mut self,
        terminal_view_id: EntityId,
        session: CLIAgentSession,
        ctx: &mut ModelContext<Self>,
    ) {
        let agent = session.agent;
        // Close any open rich input before replacing, so subscribers can
        // restore input config before the session ends.
        self.close_input(terminal_view_id, false, ctx);
        self.fallback_status_sessions.remove(&terminal_view_id);
        self.status_since.insert(terminal_view_id, Instant::now());
        if let Some(old) = self.sessions.insert(terminal_view_id, session) {
            ctx.emit(CLIAgentSessionsModelEvent::Ended {
                terminal_view_id,
                agent: old.agent,
            });
        }

        ctx.emit(CLIAgentSessionsModelEvent::Started {
            terminal_view_id,
            agent,
        });
    }

    /// Records that an auto plugin operation (install or update) failed for the given agent/host.
    /// `remote_host` is `None` for local sessions, `Some("user@hostname")` for remote.
    #[cfg(not(target_family = "wasm"))]
    pub fn record_plugin_auto_failure(&mut self, agent: CLIAgent, remote_host: Option<String>) {
        self.plugin_auto_failures.insert((agent, remote_host));
    }

    /// Saves draft text from the rich input composer for the given terminal.
    /// Stores `None` for empty or whitespace-only text.
    pub fn set_draft(&mut self, terminal_view_id: EntityId, text: String) {
        if let Some(session) = self.sessions.get_mut(&terminal_view_id) {
            session.draft_text = if text.trim().is_empty() {
                None
            } else {
                Some(text)
            };
        }
    }

    /// Clears any saved draft text for the given terminal.
    pub fn clear_draft(&mut self, terminal_view_id: EntityId) {
        if let Some(session) = self.sessions.get_mut(&terminal_view_id) {
            session.draft_text = None;
        }
    }

    /// Returns and clears the draft text for the given terminal, if any.
    pub fn take_draft(&mut self, terminal_view_id: EntityId) -> Option<String> {
        self.sessions
            .get_mut(&terminal_view_id)
            .and_then(|s| s.draft_text.take())
    }

    /// Whether an auto plugin operation has previously failed for this agent on this host.
    pub fn has_plugin_auto_failed(&self, agent: CLIAgent, remote_host: &Option<String>) -> bool {
        self.plugin_auto_failures
            .contains(&(agent, remote_host.clone()))
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
