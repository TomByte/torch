//! Derives the agent-icon shape ([`IconWithStatusVariant`]) for a terminal. A CLI agent run
//! renders as the same brand color and glyph regardless of which surface is rendering it
//! (vertical tabs, pane header), and surfaces feed the resulting variant into
//! [`render_icon_with_status`]. The pure inner function is exercised directly by the tests in
//! `agent_icon_tests.rs`.
use torchui::{AppContext, SingletonEntity};

use crate::agent_status::ConversationStatus;
use crate::terminal::CLIAgent;
use crate::terminal::cli_agent_sessions::CLIAgentSessionsModel;
use crate::terminal::view::TerminalView;
use crate::ui_components::icon_with_status::IconWithStatusVariant;

/// Returns the agent-icon variant for a live [`TerminalView`], or `None` when the terminal is
/// not an agent surface (plain terminal / shell).
pub(crate) fn terminal_view_agent_icon_variant(
    terminal_view: &TerminalView,
    app: &AppContext,
) -> Option<IconWithStatusVariant> {
    let cli_session = CLIAgentSessionsModel::as_ref(app)
        .session(terminal_view.id())
        .map(|session| CLISessionInputs {
            agent: session.agent,
            has_listener: session.listener.is_some(),
            status: session.status.to_conversation_status(),
            supports_rich_status: session.supports_rich_status(),
        });
    agent_icon_variant_from_cli_session(cli_session.as_ref())
}

/// CLI-session-derived inputs for the icon.
struct CLISessionInputs {
    agent: CLIAgent,
    /// Whether the session is backed by a plugin listener. Plugin-backed sessions report
    /// rich status; command-detected sessions only know that an agent is running.
    has_listener: bool,
    status: ConversationStatus,
    /// Whether the agent's session handler exposes rich status (plugin-backed handlers report
    /// rich status; Codex's OSC 9 handler does not).
    supports_rich_status: bool,
}

/// A session with a known (non-Unknown) agent gets that agent's icon. Status is only meaningful
/// when the session is plugin-backed and the handler exposes rich status.
fn agent_icon_variant_from_cli_session(
    session: Option<&CLISessionInputs>,
) -> Option<IconWithStatusVariant> {
    let session = session.filter(|s| !matches!(s.agent, CLIAgent::Unknown))?;
    let status =
        (session.has_listener && session.supports_rich_status).then(|| session.status.clone());
    Some(IconWithStatusVariant::CLIAgent {
        agent: session.agent,
        status,
    })
}

#[cfg(test)]
#[path = "agent_icon_tests.rs"]
mod tests;
