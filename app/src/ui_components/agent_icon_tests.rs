use super::{CLISessionInputs, agent_icon_variant_from_cli_session};
use crate::agent_status::ConversationStatus;
use crate::terminal::CLIAgent;
use crate::ui_components::icon_with_status::IconWithStatusVariant;

#[test]
fn plain_terminal_renders_no_agent_icon() {
    assert!(agent_icon_variant_from_cli_session(None).is_none());
}

#[test]
fn plugin_backed_cli_session_surfaces_rich_status() {
    let variant = agent_icon_variant_from_cli_session(Some(&CLISessionInputs {
        agent: CLIAgent::Claude,
        has_listener: true,
        status: ConversationStatus::InProgress,
        supports_rich_status: true,
    }));
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
    let variant = agent_icon_variant_from_cli_session(Some(&CLISessionInputs {
        agent: CLIAgent::Claude,
        has_listener: false,
        status: ConversationStatus::InProgress,
        supports_rich_status: false,
    }));
    assert!(matches!(
        variant,
        Some(IconWithStatusVariant::CLIAgent {
            agent: CLIAgent::Claude,
            status: None,
            ..
        })
    ));
}
