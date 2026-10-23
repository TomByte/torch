use super::{
    ASK_AGENT_HINT, COMMANDS_HINT, CONVERSATIONS_HINT, SHELL_HINT, SHELL_MODE_HINT, SHORTCUTS_HINT,
    TuiAgentEditorState, TuiBlockSessionState, TuiComposerMode, TuiInteractionState, TuiPtyState,
    TuiTerminalSessionState, TuiTerminalSessionStateResolveError, agent_input_hint,
    upgrade_terminal_model,
};
use crate::input_suggestions_mode::TuiInputSuggestionsMode;
use crate::read_only_menu::TuiReadOnlyMenuKind;
use crate::terminal_session_view::BlockingInputSource;
use crate::terminal_use::TuiInputTarget;

fn agent_editor_state(
    mode: TuiComposerMode,
    orchestration_available: bool,
) -> TuiTerminalSessionState {
    TuiTerminalSessionState::Block(TuiBlockSessionState {
        interaction: TuiInteractionState::AgentEditor(TuiAgentEditorState {
            mode,
            suggestions_mode: TuiInputSuggestionsMode::Closed,
        }),
        transcript_is_empty: false,
        orchestration_available,
        plan_available: false,
        can_attach_agent_to_running_command: false,
        agent_is_tagged_in: false,
    })
}

fn alt_screen_state(
    input_target: TuiInputTarget,
    interaction: TuiInteractionState,
) -> TuiTerminalSessionState {
    TuiTerminalSessionState::AltScreen {
        input_target,
        state: TuiBlockSessionState {
            interaction,
            transcript_is_empty: false,
            orchestration_available: false,
            plan_available: false,
            can_attach_agent_to_running_command: false,
            agent_is_tagged_in: false,
        },
    }
}

fn block_state(interaction: TuiInteractionState) -> TuiTerminalSessionState {
    TuiTerminalSessionState::Block(TuiBlockSessionState {
        interaction,
        transcript_is_empty: false,
        orchestration_available: false,
        plan_available: false,
        can_attach_agent_to_running_command: false,
        agent_is_tagged_in: false,
    })
}

#[test]
fn resolve_returns_error_after_terminal_model_owner_drops() {
    let terminal_model = std::sync::Arc::new(parking_lot::FairMutex::new(
        torch::tui_export::TerminalModel::mock(None, None),
    ));
    let weak_terminal_model = std::sync::Arc::downgrade(&terminal_model);
    drop(terminal_model);

    assert!(matches!(
        upgrade_terminal_model(&weak_terminal_model),
        Err(TuiTerminalSessionStateResolveError::TerminalModel)
    ));
}

#[test]
fn transcript_state_selects_the_applicable_hint_segments() {
    let zero_state = agent_input_hint(true, false);
    assert!(zero_state.contains(COMMANDS_HINT));
    assert!(zero_state.contains(CONVERSATIONS_HINT));
    assert!(zero_state.contains(SHORTCUTS_HINT));
    assert!(!zero_state.contains(ASK_AGENT_HINT));
    assert!(!zero_state.contains(SHELL_MODE_HINT));

    let started = agent_input_hint(false, false);
    assert!(started.contains(ASK_AGENT_HINT));
    assert!(started.contains(SHORTCUTS_HINT));
    assert!(started.contains(SHELL_MODE_HINT));
    assert!(started.contains(COMMANDS_HINT));
    assert!(!started.contains(CONVERSATIONS_HINT));
    assert!(SHELL_HINT.contains(SHORTCUTS_HINT));
}

#[test]
fn only_composer_interactions_produce_input_hints() {
    for state in [
        alt_screen_state(
            TuiInputTarget::Pty,
            TuiInteractionState::Pty(TuiPtyState::Process),
        ),
        block_state(TuiInteractionState::Blocking(
            BlockingInputSource::LongRunningCommand,
        )),
        block_state(TuiInteractionState::StartingShell),
        block_state(TuiInteractionState::Pty(TuiPtyState::Process)),
        block_state(TuiInteractionState::Pty(
            TuiPtyState::UserControlledTerminalUse,
        )),
    ] {
        assert_eq!(state.hint_text(), None);
    }

    let mut state = agent_editor_state(
        TuiComposerMode::Agent {
            agent_controlled_terminal_use: false,
        },
        false,
    );
    assert!(state.hint_text().is_some());
    {
        let TuiTerminalSessionState::Block(block) = &mut state else {
            unreachable!();
        };
        let TuiInteractionState::AgentEditor(agent_editor) = &mut block.interaction else {
            unreachable!();
        };
        agent_editor.suggestions_mode =
            TuiInputSuggestionsMode::ReadOnlyMenu(TuiReadOnlyMenuKind::Shortcuts);
    }
    assert_eq!(state.hint_text(), None);
    // The dedicated status overlay also suppresses the footer hint.
    {
        let TuiTerminalSessionState::Block(block) = &mut state else {
            unreachable!();
        };
        let TuiInteractionState::AgentEditor(agent_editor) = &mut block.interaction else {
            unreachable!();
        };
        agent_editor.suggestions_mode =
            TuiInputSuggestionsMode::ReadOnlyMenu(TuiReadOnlyMenuKind::Status);
    }
    assert_eq!(state.hint_text(), None);
}

#[test]
fn hierarchy_selects_one_input_surface() {
    assert_eq!(
        alt_screen_state(
            TuiInputTarget::Pty,
            TuiInteractionState::Pty(TuiPtyState::Process),
        )
        .input_target(),
        TuiInputTarget::Pty
    );
    assert_eq!(
        block_state(TuiInteractionState::Blocking(
            BlockingInputSource::LongRunningCommand,
        ))
        .input_target(),
        TuiInputTarget::Pty
    );
    assert_eq!(
        block_state(TuiInteractionState::StartingShell).input_target(),
        TuiInputTarget::Disabled
    );
    assert_eq!(
        block_state(TuiInteractionState::Pty(TuiPtyState::Process)).input_target(),
        TuiInputTarget::Pty
    );

    let state = agent_editor_state(TuiComposerMode::Shell, false);
    assert_eq!(state.input_target(), TuiInputTarget::AgentEditor);
    assert!(state.composer_shortcuts_active());
}

#[test]
fn suggestions_overlay_disables_composer_shortcuts_without_changing_the_input_surface() {
    let mut state = agent_editor_state(TuiComposerMode::Shell, false);
    let TuiTerminalSessionState::Block(block) = &mut state else {
        unreachable!();
    };
    let TuiInteractionState::AgentEditor(agent_editor) = &mut block.interaction else {
        unreachable!();
    };
    agent_editor.suggestions_mode = TuiInputSuggestionsMode::SlashCommands;

    assert_eq!(state.input_target(), TuiInputTarget::AgentEditor);
    assert!(!state.composer_shortcuts_active());
}
