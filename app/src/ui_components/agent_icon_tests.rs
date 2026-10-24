use super::{CLISessionInputs, TerminalIconInputs, agent_icon_variant_from_terminal_inputs};
use crate::agent_status::ConversationStatus;
use crate::terminal::CLIAgent;
use crate::ui_components::icon_with_status::IconWithStatusVariant;

fn no_session_inputs() -> TerminalIconInputs {
    TerminalIconInputs {
        cli_session: None,
        selected_conversation_status: None,
        has_selected_conversation: false,
    }
}

#[test]
fn plain_terminal_renders_no_agent_icon() {
    assert!(agent_icon_variant_from_terminal_inputs(&no_session_inputs()).is_none());
}

#[test]
fn selected_conversation_renders_torch_agent_with_status() {
    let variant = agent_icon_variant_from_terminal_inputs(&TerminalIconInputs {
        selected_conversation_status: Some(ConversationStatus::InProgress),
        has_selected_conversation: true,
        ..no_session_inputs()
    });
    assert!(matches!(
        variant,
        Some(IconWithStatusVariant::OzAgent {
            status: Some(ConversationStatus::InProgress),
            ..
        })
    ));
}

#[test]
fn plugin_backed_cli_session_surfaces_rich_status() {
    let variant = agent_icon_variant_from_terminal_inputs(&TerminalIconInputs {
        cli_session: Some(CLISessionInputs {
            agent: CLIAgent::Claude,
            has_listener: true,
            status: ConversationStatus::InProgress,
            supports_rich_status: true,
        }),
        ..no_session_inputs()
    });
    assert!(matches!(
        variant,
        Some(IconWithStatusVariant::CLIAgent {
            agent: CLIAgent::Claude,
            status: Some(ConversationStatus::InProgress),
            ..
        })
    ));
}

#[test]
fn command_detected_cli_session_hides_status() {
    let variant = agent_icon_variant_from_terminal_inputs(&TerminalIconInputs {
        cli_session: Some(CLISessionInputs {
            agent: CLIAgent::Claude,
            has_listener: false,
            status: ConversationStatus::InProgress,
            supports_rich_status: false,
        }),
        ..no_session_inputs()
    });
    assert!(matches!(
        variant,
        Some(IconWithStatusVariant::CLIAgent {
            agent: CLIAgent::Claude,
            status: None,
            ..
        })
    ));
}
