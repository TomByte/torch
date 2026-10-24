use super::*;
use crate::terminal::CLIAgent;

#[test]
fn test_rich_input_submit_strategy_for_muse_uses_bracketed_paste() {
    assert_eq!(
        rich_input_submit_strategy(CLIAgent::Muse),
        RichInputSubmitStrategy::BracketedPaste
    );
}

#[test]
fn test_rich_input_submit_strategy_for_oh_my_pi() {
    assert_eq!(
        rich_input_submit_strategy(CLIAgent::OhMyPi),
        RichInputSubmitStrategy::BracketedPaste
    );
}

#[test]
fn test_rich_input_submit_strategy_for_antigravity() {
    assert_eq!(
        rich_input_submit_strategy(CLIAgent::Antigravity),
        RichInputSubmitStrategy::DelayedEnter
    );
}

/// Hermes interprets embedded newlines as submit actions when text is written
/// directly. Bracketed paste preserves them as part of one input payload.
#[test]
fn test_rich_input_submit_strategy_for_hermes_uses_bracketed_paste() {
    assert_eq!(
        rich_input_submit_strategy(CLIAgent::Hermes),
        RichInputSubmitStrategy::BracketedPaste
    );
}
