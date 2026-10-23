use std::cell::RefCell;
use std::rc::Rc;

use torch_terminal::model::escape_sequences::{BRACKETED_PASTE_END, BRACKETED_PASTE_START};
use torchui::{App, SingletonEntity};

use super::*;
use crate::terminal::cli_agent_sessions::{
    CLIAgentInputState, CLIAgentSession, CLIAgentSessionContext, CLIAgentSessionStatus,
    CLIAgentSessionsModel,
};
use crate::terminal::model::ansi::{BootstrappedValue, Handler as _, InitShellValue};
use crate::terminal::{CLIAgent, Event};
use crate::test_util::add_window_with_terminal;
use crate::test_util::terminal::initialize_app_for_terminal_view;

fn simulate_user_started_long_running_command(view: &mut TerminalView) {
    {
        let mut model = view.model.lock();
        model.init_shell(InitShellValue {
            session_id: 0.into(),
            shell: "zsh".to_owned(),
            ..Default::default()
        });
        model.bootstrapped(BootstrappedValue {
            shell: "zsh".to_owned(),
            ..Default::default()
        });
        model.simulate_long_running_block("ssh localhost", "Password:");
    }
}

#[test]
fn cli_agent_footer_does_not_render_for_warp_tui_session() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |view, ctx| {
            simulate_user_started_long_running_command(view);

            CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
                sessions.set_session(
                    view.id(),
                    CLIAgentSession {
                        agent: CLIAgent::WarpTui,
                        status: CLIAgentSessionStatus::InProgress,
                        session_context: CLIAgentSessionContext::default(),
                        input_state: CLIAgentInputState::Closed,
                        listener: None,
                        plugin_version: None,
                        remote_host: None,
                        draft_text: None,
                        custom_command_prefix: None,
                        received_rich_notification: false,
                        should_auto_toggle_input: false,
                    },
                    ctx,
                );
            });

            view.maybe_show_use_agent_footer_in_blocklist(ctx);

            let model = view.model.lock();
            assert!(!view.should_render_use_agent_footer(&model, ctx));
            let active_block_index = model.block_list().active_block_index();
            assert!(
                model
                    .block_list()
                    .last_non_hidden_rich_content_block_after_block(Some(active_block_index))
                    .is_none()
            );
        });
    })
}
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

#[test]
fn insert_cli_agent_voice_text_hermes_multiline_uses_bracketed_paste_without_submitting() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        let pty_writes = Rc::new(RefCell::new(Vec::new()));
        let writes = pty_writes.clone();
        app.update(|ctx| {
            ctx.subscribe_to_view(&terminal, move |_, event, _| {
                if let Event::WriteBytesToPty { bytes } = event {
                    writes.borrow_mut().push(bytes.to_vec());
                }
            });
        });

        terminal.update(&mut app, |view, ctx| {
            CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
                sessions.set_session(
                    view.view_id,
                    CLIAgentSession {
                        agent: CLIAgent::Hermes,
                        status: CLIAgentSessionStatus::InProgress,
                        session_context: CLIAgentSessionContext::default(),
                        input_state: CLIAgentInputState::Closed,
                        should_auto_toggle_input: false,
                        listener: None,
                        remote_host: None,
                        plugin_version: None,
                        draft_text: None,
                        custom_command_prefix: None,
                        received_rich_notification: false,
                    },
                    ctx,
                );
            });

            view.handle_use_agent_footer_event(
                &UseAgentToolbarEvent::InsertIntoCLIPty("line1\nline2".to_owned()),
                ctx,
            );
        });

        let writes = pty_writes.borrow();
        assert_eq!(
            writes.len(),
            1,
            "voice transcription should be inserted without a separate submit"
        );

        let mut expected_paste =
            Vec::with_capacity(BRACKETED_PASTE_START.len() + 11 + BRACKETED_PASTE_END.len());
        expected_paste.extend_from_slice(BRACKETED_PASTE_START);
        expected_paste.extend_from_slice(b"line1\nline2");
        expected_paste.extend_from_slice(BRACKETED_PASTE_END);
        assert_eq!(writes[0], expected_paste);
    })
}
