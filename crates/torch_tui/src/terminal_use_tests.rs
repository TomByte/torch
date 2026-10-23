use std::sync::Arc;

use parking_lot::FairMutex;
use torch::tui_export::{BlockId, TerminalModel, TranscriptScope};

use super::{
    TuiInputTarget, hide_agent_requested_command_from_top_level, inline_process_owns_input,
    tui_input_target, tui_input_target_for_state, user_controlled_running_command,
};

fn model_with_finished_block(command: &str) -> (TerminalModel, BlockId) {
    let mut model = TerminalModel::mock(None, None);
    model
        .block_list_mut()
        .set_transcript_scope(TranscriptScope::Unfiltered);
    model.simulate_block(command, "output\r\n");
    let block_id = model
        .block_list()
        .blocks()
        .iter()
        .rev()
        .find(|block| block.finished())
        .expect("simulated block should exist")
        .id()
        .clone();
    (model, block_id)
}

#[test]
fn ordinary_long_running_command_owns_inline_input() {
    let mut model = TerminalModel::mock(None, None);
    model.simulate_long_running_block("cat", "");

    assert!(inline_process_owns_input(&model));
    assert_eq!(
        user_controlled_running_command(&model).map(|block| block.id()),
        Some(model.block_list().active_block().id())
    );
}

#[test]
fn unfinished_background_output_does_not_own_inline_input() {
    let mut model = TerminalModel::mock(None, None);
    model.simulate_block("true", "");
    model.process_bytes("starship: scanning files timed out");

    let background_block = model
        .block_list()
        .blocks()
        .iter()
        .find(|block| block.is_background())
        .expect("early output should create a background block");
    assert!(!background_block.finished());
    assert!(background_block.is_active_and_long_running());
    assert!(user_controlled_running_command(&model).is_none());
    assert!(!inline_process_owns_input(&model));
    assert_eq!(tui_input_target(&model), TuiInputTarget::AgentEditor);
}

#[test]
fn shell_startup_routes_input_by_bootstrap_stage() {
    let mut model = TerminalModel::mock(None, None);
    assert_eq!(tui_input_target(&model), TuiInputTarget::AgentEditor);

    model.block_list_mut().reinit_shell(None);
    assert_eq!(tui_input_target(&model), TuiInputTarget::Disabled);
    assert_eq!(
        tui_input_target_for_state(false, true, false, false, false, false),
        TuiInputTarget::Disabled,
        "silent startup-script execution should keep the bootstrap editor visible",
    );
    assert_eq!(
        tui_input_target_for_state(false, true, true, false, false, false),
        TuiInputTarget::Pty,
        "visible startup-script output should accept interactive PTY input",
    );
}

#[test]
fn submit_policy_blocks_bootstrap_but_allows_ready_prompt() {
    assert!(
        !tui_input_target_for_state(false, false, false, false, false, false)
            .agent_editor_owns_input(),
        "bootstrap submission must remain disabled"
    );
    assert!(
        tui_input_target_for_state(false, false, false, true, false, false)
            .agent_editor_owns_input(),
        "the normal prompt must accept submission"
    );
}

#[test]
fn alternate_screen_routes_input_to_its_current_owner() {
    assert_eq!(
        tui_input_target_for_state(true, false, false, true, false, false),
        TuiInputTarget::Pty,
        "a user-controlled alt-screen process should receive PTY input",
    );
    assert_eq!(
        tui_input_target_for_state(true, false, false, true, false, true),
        TuiInputTarget::AgentEditor,
        "agent control should keep alt-screen keystrokes in the composer",
    );
}

#[test]
fn spawned_user_command_keeps_its_top_level_height() {
    let (model, block_id) = model_with_finished_block("sleep 10");
    let model = Arc::new(FairMutex::new(model));
    let height_before = model
        .lock()
        .block_list()
        .block_heights()
        .summary()
        .height
        .as_f64();

    assert!(!hide_agent_requested_command_from_top_level(&model, None));

    let model = model.lock();
    let block_list = model.block_list();
    let block = block_list
        .block_with_id(&block_id)
        .expect("block should exist");
    assert!(block.is_visible(block_list.transcript_scope()));
    assert_eq!(
        block_list.block_heights().summary().height.as_f64(),
        height_before
    );
}
