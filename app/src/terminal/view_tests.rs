use std::cell::RefCell;
use std::pin::pin;
use std::rc::Rc;

use torch_terminal::model::escape_sequences::{BRACKETED_PASTE_END, BRACKETED_PASTE_START, C0};
use torchui::notification::UserNotification;
use torchui::{App, EntityIdSet, Presenter, WindowInvalidation};

use super::*;
use crate::ai::blocklist::{InputConfig, InputType};
use crate::editor::{AutosuggestionLocation, AutosuggestionType};
use crate::features::FeatureFlag;
use crate::pane_group::focus_state::PaneGroupFocusState;
use crate::pane_group::{BackingView, TerminalPaneId};
use crate::settings::{AISettings, AppEditorSettings, RightClickBehavior};
use crate::tab::NewSessionMenuItem;
use crate::terminal::alt_screen::should_intercept_mouse;
use crate::terminal::block_list_element::{SnackbarPoint, SnackbarTranslationMode};
use crate::terminal::block_list_viewport::{ClampingMode, ScrollLines};
use crate::terminal::cli_agent_sessions::event::{
    CLI_AGENT_NOTIFICATION_SENTINEL, CLIAgentEvent, CLIAgentEventPayload, CLIAgentEventSource,
    CLIAgentEventType,
};
use crate::terminal::cli_agent_sessions::listener::CLIAgentSessionListener;
use crate::terminal::cli_agent_sessions::{
    CLIAgentInputEntrypoint, CLIAgentInputState, CLIAgentRichInputCloseReason, CLIAgentSession,
    CLIAgentSessionContext, CLIAgentSessionStatus, CLIAgentSessionsModel,
};
use crate::terminal::model::ansi::{
    self, BootstrappedValue, CommandFinishedValue, CompletionMetadata, InitShellValue, PrecmdValue,
    PreexecValue, PromptMetadata,
};
use crate::terminal::model::block::BlockId;
use crate::terminal::model::blocks::{TotalIndex, insert_block};
use crate::terminal::model::grid::Dimensions as _;
use crate::terminal::model::terminal_model::WithinBlock;
use crate::terminal::{CLIAgent, MockTerminalManager, TerminalModel, should_right_click_paste};
use crate::test_util::terminal::{
    add_window_with_id_and_terminal, initialize_app_for_terminal_view,
};
use crate::test_util::{add_window_with_terminal, assert_eventually};
use crate::view_components::find::FindWithinBlockState;
use crate::workspace::WorkspaceAction;
use crate::workspace::view::tests::{initialize_app as initialize_workspace_app, mock_workspace};

#[test]
fn focus_reporting_writes_focus_events_in_normal_screen() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);
        let pty_writes: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
        let writes = pty_writes.clone();

        app.update(|ctx| {
            ctx.subscribe_to_view(&terminal, move |_, event, _| {
                if let Event::WriteBytesToPty { bytes } = event {
                    writes.borrow_mut().push(bytes.to_vec());
                }
            });
        });

        terminal.update(&mut app, |view, ctx| {
            let mut model = view.model.lock();
            model.simulate_long_running_block("python3 /tmp/warp_focus_test.py", "");
            assert!(!model.is_alt_screen_active());
            ansi::Handler::set_mode(&mut *model, ansi::Mode::ReportFocusInOut);
            assert!(model.is_term_mode_set(TermMode::FOCUS_IN_OUT));
            drop(model);
            assert!(view.should_report_focus(ctx));

            view.maybe_report_focus_out(ctx);
            view.maybe_report_focus_in(ctx);
        });

        assert_eq!(
            *pty_writes.borrow(),
            vec![
                escape_sequences::EscCodes::FOCUS_OUT.to_vec(),
                escape_sequences::EscCodes::FOCUS_IN.to_vec(),
            ]
        );
    })
}

#[test]
fn should_right_click_paste_true_only_without_shift_when_setting_enabled() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        SelectionSettings::handle(&app).update(&mut app, |settings, ctx| {
            let _ = settings
                .right_click_behavior
                .set_value(RightClickBehavior::Paste, ctx);
        });

        terminal.update(&mut app, |_view, ctx| {
            assert!(
                should_right_click_paste(false, ctx),
                "a bare right-click should paste once the setting is enabled"
            );
            assert!(
                !should_right_click_paste(true, ctx),
                "Shift+right-click should always reveal the context menu, even with the setting enabled"
            );
        });
    })
}

/// Right-clicking a long-running block that owns the mouse (SGR mouse reporting on) must forward
/// the raw click to the PTY as a mouse report, under both `right_click_behavior` values -- it must
/// never fall through to Paste or the block list's own context menu.
#[test]
fn block_list_right_click_forwards_to_pty_when_long_running_block_owns_mouse() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let (window_id, terminal) = add_window_with_id_and_terminal(&mut app, None);

        let pty_writes: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
        let writes = pty_writes.clone();
        app.update(|ctx| {
            ctx.subscribe_to_view(&terminal, move |_, event, _| {
                if let Event::WriteBytesToPty { bytes } = event {
                    writes.borrow_mut().push(bytes.to_vec());
                }
            });
        });

        let mut updated = EntityIdSet::default();
        updated.insert(app.root_view_id(window_id).unwrap());
        let invalidation = WindowInvalidation {
            updated,
            ..Default::default()
        };
        let presenter = Rc::new(RefCell::new(Presenter::new(window_id)));

        let size_info = terminal.update(&mut app, |view, ctx| {
            let mut model = view.model.lock();
            model.simulate_long_running_block("cmd", "output");
            model.set_mode(ansi::Mode::SgrMouse);
            model.set_mode(ansi::Mode::ReportMouseClicks);
            assert!(!model.is_alt_screen_active());
            assert!(
                !should_intercept_mouse(&model, false, ctx),
                "the running command should own the mouse with SGR reporting enabled"
            );
            *view.size_info
        });

        macro_rules! rerender {
            () => {
                app.update(enclose!((presenter, invalidation) move |ctx| {
                    presenter
                        .borrow_mut()
                        .invalidate(invalidation, ctx);
                    presenter.borrow_mut().build_scene(
                        vec2f(size_info.pane_width_px, size_info.pane_height_px),
                        1.,
                        None,
                        ctx,
                    );
                }));
            };
        }

        // The block list is pinned to the bottom of the pane by default, so a lone, short block
        // sits just above the input box rather than at the top of the viewport.
        let position = vec2f(
            2. * size_info.cell_width_px.as_f32(),
            size_info.pane_height_px - 3. * size_info.cell_height_px.as_f32(),
        );

        for right_click_behavior in [RightClickBehavior::ContextMenu, RightClickBehavior::Paste] {
            SelectionSettings::handle(&app).update(&mut app, |settings, ctx| {
                let _ = settings
                    .right_click_behavior
                    .set_value(right_click_behavior, ctx);
            });
            pty_writes.borrow_mut().clear();

            rerender!();
            app.update(enclose!((presenter) move |ctx| {
                ctx.simulate_window_event(
                    torchui::Event::RightMouseDown {
                        position,
                        cmd: false,
                        shift: false,
                        click_count: 1,
                    },
                    window_id,
                    presenter.clone(),
                );
            }));

            let writes = pty_writes.borrow();
            assert_eq!(
                writes.len(),
                1,
                "exactly one raw mouse report should reach the PTY under {right_click_behavior:?}, got {writes:?}"
            );
            assert!(
                writes[0].starts_with(b"\x1b[<2;"),
                "expected an SGR right-button-press mouse report under {right_click_behavior:?}, got {:?}",
                writes[0]
            );
        }

        // The input box must never have received a paste from either right-click.
        let input = terminal.read(&app, |terminal, _ctx| terminal.input().clone());
        input.read(&app, |input, ctx| {
            assert_eq!(
                input.buffer_text(ctx),
                "",
                "a long-running block's right-click must never be treated as Paste"
            );
        });
    })
}

#[test]
fn block_list_shift_right_click_opens_context_menu_when_right_click_pastes() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let (window_id, terminal) = add_window_with_id_and_terminal(&mut app, None);

        let pty_writes: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
        let writes = pty_writes.clone();
        app.update(|ctx| {
            ctx.subscribe_to_view(&terminal, move |_, event, _| {
                if let Event::WriteBytesToPty { bytes } = event {
                    writes.borrow_mut().push(bytes.to_vec());
                }
            });
        });

        let mut updated = EntityIdSet::default();
        updated.insert(app.root_view_id(window_id).unwrap());
        let invalidation = WindowInvalidation {
            updated,
            ..Default::default()
        };
        let presenter = Rc::new(RefCell::new(Presenter::new(window_id)));

        let size_info = terminal.update(&mut app, |view, ctx| {
            let mut model = view.model.lock();
            model.simulate_long_running_block("cmd", "output");
            assert!(!model.is_alt_screen_active());
            // No mouse reporting is enabled, so Torch -- not the running command -- owns this
            // right-click regardless of Shift.
            assert!(should_intercept_mouse(&model, false, ctx));
            *view.size_info
        });

        SelectionSettings::handle(&app).update(&mut app, |settings, ctx| {
            let _ = settings
                .right_click_behavior
                .set_value(RightClickBehavior::Paste, ctx);
        });

        macro_rules! rerender {
            () => {
                app.update(enclose!((presenter, invalidation) move |ctx| {
                    presenter
                        .borrow_mut()
                        .invalidate(invalidation, ctx);
                    presenter.borrow_mut().build_scene(
                        vec2f(size_info.pane_width_px, size_info.pane_height_px),
                        1.,
                        None,
                        ctx,
                    );
                }));
            };
        }

        // Same position as the long-running block above: a lone, short block sitting just
        // above the input box.
        let position = vec2f(
            2. * size_info.cell_width_px.as_f32(),
            size_info.pane_height_px - 3. * size_info.cell_height_px.as_f32(),
        );

        let input = terminal.read(&app, |terminal, _ctx| terminal.input().clone());
        let input_text_before = input.read(&app, |input, ctx| input.buffer_text(ctx));
        assert!(!terminal.read(&app, |view, _ctx| view.is_context_menu_open()));

        rerender!();
        app.update(enclose!((presenter) move |ctx| {
            ctx.simulate_window_event(
                torchui::Event::RightMouseDown {
                    position,
                    cmd: false,
                    shift: true,
                    click_count: 1,
                },
                window_id,
                presenter.clone(),
            );
        }));

        assert!(
            terminal.read(&app, |view, _ctx| view.is_context_menu_open()),
            "Shift+right-click must open the block's context menu, even when right-click-pastes is enabled"
        );
        assert!(
            pty_writes.borrow().is_empty(),
            "Shift+right-click must never paste to the PTY, got {:?}",
            pty_writes.borrow()
        );
        input.read(&app, |input, ctx| {
            assert_eq!(
                input.buffer_text(ctx),
                input_text_before,
                "Shift+right-click must never paste into the input box"
            );
        });
    })
}

#[test]
fn alt_screen_shift_right_click_opens_context_menu_when_right_click_pastes() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let (window_id, terminal) = add_window_with_id_and_terminal(&mut app, None);

        let pty_writes: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
        let writes = pty_writes.clone();
        app.update(|ctx| {
            ctx.subscribe_to_view(&terminal, move |_, event, _| {
                if let Event::WriteBytesToPty { bytes } = event {
                    writes.borrow_mut().push(bytes.to_vec());
                }
            });
        });

        let mut updated = EntityIdSet::default();
        updated.insert(app.root_view_id(window_id).unwrap());
        let invalidation = WindowInvalidation {
            updated,
            ..Default::default()
        };
        let presenter = Rc::new(RefCell::new(Presenter::new(window_id)));

        let size_info = terminal.update(&mut app, |view, ctx| {
            let mut model = view.model.lock();
            model.set_mode(ansi::Mode::SwapScreen {
                save_cursor_and_clear_screen: true,
            });
            assert!(model.is_alt_screen_active());
            // No mouse reporting is enabled, so Torch -- not the alt-screen application -- owns
            // this right-click, with or without Shift.
            assert!(should_intercept_mouse(&model, false, ctx));
            assert!(should_intercept_mouse(&model, true, ctx));
            *view.size_info
        });

        SelectionSettings::handle(&app).update(&mut app, |settings, ctx| {
            let _ = settings
                .right_click_behavior
                .set_value(RightClickBehavior::Paste, ctx);
        });

        macro_rules! rerender {
            () => {
                app.update(enclose!((presenter, invalidation) move |ctx| {
                    presenter
                        .borrow_mut()
                        .invalidate(invalidation, ctx);
                    presenter.borrow_mut().build_scene(
                        vec2f(size_info.pane_width_px, size_info.pane_height_px),
                        1.,
                        None,
                        ctx,
                    );
                }));
            };
        }

        let position = vec2f(
            2. * size_info.cell_width_px.as_f32(),
            2. * size_info.cell_height_px.as_f32() - 1.,
        );

        let input = terminal.read(&app, |terminal, _ctx| terminal.input().clone());
        let input_text_before = input.read(&app, |input, ctx| input.buffer_text(ctx));
        assert!(!terminal.read(&app, |view, _ctx| view.is_context_menu_open()));

        rerender!();
        app.update(enclose!((presenter) move |ctx| {
            ctx.simulate_window_event(
                torchui::Event::RightMouseDown {
                    position,
                    cmd: false,
                    shift: true,
                    click_count: 1,
                },
                window_id,
                presenter.clone(),
            );
        }));

        assert!(
            terminal.read(&app, |view, _ctx| view.is_context_menu_open()),
            "Shift+right-click must open the alt-screen context menu, even when right-click-pastes is enabled"
        );
        assert!(
            pty_writes.borrow().is_empty(),
            "Shift+right-click must never paste to the PTY, got {:?}",
            pty_writes.borrow()
        );
        input.read(&app, |input, ctx| {
            assert_eq!(
                input.buffer_text(ctx),
                input_text_before,
                "Shift+right-click must never paste into the input box"
            );
        });
    })
}

#[test]
fn input_shift_right_click_opens_context_menu_when_right_click_pastes() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let (window_id, terminal) = add_window_with_id_and_terminal(&mut app, None);

        let pty_writes: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
        let writes = pty_writes.clone();
        app.update(|ctx| {
            ctx.subscribe_to_view(&terminal, move |_, event, _| {
                if let Event::WriteBytesToPty { bytes } = event {
                    writes.borrow_mut().push(bytes.to_vec());
                }
            });
        });

        let mut updated = EntityIdSet::default();
        updated.insert(app.root_view_id(window_id).unwrap());
        let invalidation = WindowInvalidation {
            updated,
            ..Default::default()
        };
        let presenter = Rc::new(RefCell::new(Presenter::new(window_id)));

        let size_info = terminal.read(&app, |view, _ctx| *view.size_info);

        SelectionSettings::handle(&app).update(&mut app, |settings, ctx| {
            let _ = settings
                .right_click_behavior
                .set_value(RightClickBehavior::Paste, ctx);
        });

        macro_rules! rerender {
            () => {
                app.update(enclose!((presenter, invalidation) move |ctx| {
                    presenter
                        .borrow_mut()
                        .invalidate(invalidation, ctx);
                    presenter.borrow_mut().build_scene(
                        vec2f(size_info.pane_width_px, size_info.pane_height_px),
                        1.,
                        None,
                        ctx,
                    );
                }));
            };
        }

        // The input box is docked to the very bottom of the pane, below the block list.
        let position = vec2f(
            2. * size_info.cell_width_px.as_f32(),
            size_info.pane_height_px - 0.5 * size_info.cell_height_px.as_f32(),
        );

        let input = terminal.read(&app, |terminal, _ctx| terminal.input().clone());
        let input_text_before = input.read(&app, |input, ctx| input.buffer_text(ctx));
        assert!(!terminal.read(&app, |view, _ctx| view.is_context_menu_open()));

        rerender!();
        app.update(enclose!((presenter) move |ctx| {
            ctx.simulate_window_event(
                torchui::Event::RightMouseDown {
                    position,
                    cmd: false,
                    shift: true,
                    click_count: 1,
                },
                window_id,
                presenter.clone(),
            );
        }));

        assert!(
            terminal.read(&app, |view, _ctx| view.is_context_menu_open()),
            "Shift+right-click on the input box must open its context menu, even when right-click-pastes is enabled"
        );
        assert!(
            pty_writes.borrow().is_empty(),
            "Shift+right-click must never paste to the PTY, got {:?}",
            pty_writes.borrow()
        );
        input.read(&app, |input, ctx| {
            assert_eq!(
                input.buffer_text(ctx),
                input_text_before,
                "Shift+right-click must never paste into the input box"
            );
        });
    })
}

#[test]
fn waterfall_background_right_click_honors_right_click_pastes_setting() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let (window_id, terminal) = add_window_with_id_and_terminal(&mut app, None);

        terminal.update(&mut app, |_view, ctx| {
            InputModeSettings::handle(ctx).update(ctx, |input_mode_settings, ctx| {
                let _ = input_mode_settings
                    .input_mode
                    .set_value(InputMode::Waterfall, ctx);
            });
        });

        SelectionSettings::handle(&app).update(&mut app, |settings, ctx| {
            let _ = settings
                .right_click_behavior
                .set_value(RightClickBehavior::Paste, ctx);
        });

        app.update(|ctx| {
            ctx.clipboard().write(ClipboardContent::plain_text(
                "waterfall-paste-test".to_string(),
            ));
        });

        let mut updated = EntityIdSet::default();
        updated.insert(app.root_view_id(window_id).unwrap());
        let invalidation = WindowInvalidation {
            updated,
            ..Default::default()
        };
        let presenter = Rc::new(RefCell::new(Presenter::new(window_id)));

        let size_info = terminal.read(&app, |view, _ctx| *view.size_info);

        macro_rules! rerender {
            () => {
                app.update(enclose!((presenter, invalidation) move |ctx| {
                    presenter
                        .borrow_mut()
                        .invalidate(invalidation, ctx);
                    presenter.borrow_mut().build_scene(
                        vec2f(size_info.pane_width_px, size_info.pane_height_px),
                        1.,
                        None,
                        ctx,
                    );
                }));
            };
        }

        // With no blocks, both the block content height and the input's saved position height
        // are zero, so any position within the pane satisfies "outside the block"; pick a point
        // near the bottom of the pane, comfortably inside its bounds.
        let position = vec2f(
            2. * size_info.cell_width_px.as_f32(),
            size_info.pane_height_px - 0.1,
        );

        let input = terminal.read(&app, |terminal, _ctx| terminal.input().clone());
        assert!(!terminal.read(&app, |view, _ctx| view.is_context_menu_open()));

        rerender!();
        app.update(enclose!((presenter) move |ctx| {
            ctx.simulate_window_event(
                torchui::Event::RightMouseDown {
                    position,
                    cmd: false,
                    shift: false,
                    click_count: 1,
                },
                window_id,
                presenter.clone(),
            );
        }));

        assert!(
            !terminal.read(&app, |view, _ctx| view.is_context_menu_open()),
            "a bare right-click on the waterfall background must paste, not open the context menu"
        );
        input.read(&app, |input, ctx| {
            assert_eq!(
                input.buffer_text(ctx),
                "waterfall-paste-test",
                "a bare right-click on the waterfall background must paste the clipboard into the input"
            );
        });

        // Reset the input, then confirm Shift still reveals the context menu instead.
        input.update(&mut app, |input, ctx| {
            input.replace_buffer_content("", ctx);
        });

        rerender!();
        app.update(enclose!((presenter) move |ctx| {
            ctx.simulate_window_event(
                torchui::Event::RightMouseDown {
                    position,
                    cmd: false,
                    shift: true,
                    click_count: 1,
                },
                window_id,
                presenter.clone(),
            );
        }));

        assert!(
            terminal.read(&app, |view, _ctx| view.is_context_menu_open()),
            "Shift+right-click on the waterfall background must open the context menu, even when right-click-pastes is enabled"
        );
        input.read(&app, |input, ctx| {
            assert_eq!(
                input.buffer_text(ctx),
                "",
                "Shift+right-click must never paste into the input box"
            );
        });
    })
}

/// Test to verify that blocks created through normal execution
/// have the correct local status set
#[test]
fn test_create_new_block_with_local_status() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        // Set up a terminal with a local session
        terminal.update(&mut app, |view, _ctx| {
            let mut model = view.model.lock();

            // Initialize a local session
            model.init_shell(InitShellValue {
                session_id: 0.into(),
                shell: "bash".to_owned(),
                ..Default::default()
            });
            model.bootstrapped(BootstrappedValue {
                shell: "bash".to_owned(),
                ..Default::default()
            });
        });

        assert_eventually!(
            terminal.read(&app, |view, ctx| !view
                .active_block_is_considered_remote(ctx)),
            "Block should be local"
        );

        // No remote blocks should exist
        assert_eventually!(
            terminal.read(&app, |view, _ctx| !view.contains_restored_remote_blocks()),
            "No remote blocks should exist"
        );

        // Update the view's flags
        // view.update_focused_terminal_info(ctx);
        assert_eventually!(
            terminal.read(&app, |view, _ctx| !view.any_session_contains_remote_blocks),
            "No remote blocks should exist"
        );

        // Now test with a remote session
        terminal.update(&mut app, |view, _ctx| {
            let mut model = view.model.lock();

            // Create a new block with a remote session ID and remote_shell
            model.init_shell(InitShellValue {
                session_id: 1.into(),
                shell: "bash".to_owned(),
                user: "user".to_owned(),
                hostname: "remote".to_owned(),
                ..Default::default()
            });
            model.bootstrapped(BootstrappedValue {
                shell: "bash".to_owned(),
                ..Default::default()
            });

            // Create a block in the remote session
            model.simulate_block("echo remote", "remote output");
        });

        // Verify block is non-local (remote)
        assert_eventually!(
            terminal.read(&app, |view, ctx| view
                .active_block_is_considered_remote(ctx)),
            "Block should be non-local (remote)"
        );

        // Remote blocks should be detected
        assert_eventually!(
            terminal.read(&app, |view, _ctx| view.any_session_contains_remote_blocks),
            "Remote blocks should be detected"
        );
    })
}

#[test]
fn submit_cli_agent_rich_input_restores_unlocked_input_config() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _agent_view = FeatureFlag::AgentView.override_enabled(true);
        let _cli_agent_rich_input = FeatureFlag::CLIAgentRichInput.override_enabled(true);
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            let _ = settings
                .auto_dismiss_rich_input_after_submit
                .set_value(true, ctx);
        });

        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |view, ctx| {
            view.input.update(ctx, |input, ctx| {
                input.ai_input_model().update(ctx, |ai_input, ctx| {
                    ai_input.set_input_config(
                        InputConfig {
                            input_type: InputType::Shell,
                            is_locked: false,
                        },
                        true,
                        None,
                        ctx,
                    );
                });
            });

            CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
                sessions.set_session(
                    view.view_id,
                    CLIAgentSession {
                        agent: CLIAgent::Droid,
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

            view.open_cli_agent_rich_input(CLIAgentInputEntrypoint::FooterButton, ctx);
            assert!(view.has_active_cli_agent_input_session(ctx));

            view.submit_cli_agent_rich_input("hello!".to_owned(), ctx);
            assert!(!view.has_active_cli_agent_input_session(ctx));
        });

        terminal.read(&app, |view, ctx| {
            let input = view.input.as_ref(ctx);
            let ai_input_model = input.ai_input_model().as_ref(ctx);

            assert_eq!(
                ai_input_model.input_config(),
                InputConfig {
                    input_type: InputType::Shell,
                    is_locked: false,
                }
            );
            assert!(input.editor().as_ref(ctx).buffer_text(ctx).is_empty());
        });
    })
}

#[test]
fn command_first_word_and_suffix_preserves_leading_whitespace() {
    assert_eq!(
        command_first_word_and_suffix("  myssh arg"),
        Some(("myssh", " arg"))
    );
}

#[test]
fn command_first_word_and_suffix_handles_alias_without_args() {
    assert_eq!(
        command_first_word_and_suffix("  myssh"),
        Some(("myssh", ""))
    );
}

/// Test clearing of session flag state when terminal is cleared
#[test]
fn test_clear_session_flag_state() {
    use torch_terminal::shell::ShellType;

    use crate::ai::blocklist::SerializedBlockListItem;
    use crate::terminal::ShellHost;
    use crate::terminal::model::block::SerializedBlock;

    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        // Create a remote restored block
        let mut remote_block =
            SerializedBlock::new_for_test("echo remote".into(), "remote output".into());
        remote_block.is_local = Some(false); // Mark it as a remote block
        remote_block.shell_host = Some(ShellHost {
            shell_type: ShellType::Bash,
            user: "user".to_string(),
            hostname: "remote".to_string(), // Remote hostname indicates a remote session
        });

        // Convert to SerializedBlockListItem
        let restored_blocks = [SerializedBlockListItem::Command {
            block: Box::new(remote_block),
        }];

        // Create terminal with the restored remote block
        let terminal = add_window_with_terminal(&mut app, Some(&restored_blocks));

        terminal.update(&mut app, |view, ctx| {
            // Verify initial state - block was created as remote and restored
            assert!(
                !view.any_session_contains_remote_blocks,
                "Terminal should not have remote blocks"
            );
            assert!(
                view.any_session_contains_restored_remote_blocks,
                "Terminal should have restored remote blocks"
            );

            {
                // Verify the block was properly created with correct properties
                let model = view.model.lock();
                let blocks = model.block_list().blocks();

                // The first block should be our restored remote block
                assert!(!blocks.is_empty(), "At least one block should exist");
                if let Some(first_block) = blocks.first() {
                    assert_eq!(
                        first_block.restored_block_was_local(),
                        Some(false),
                        "First block should be marked as a remote restored block"
                    );
                }
            }

            // Now clear the terminal
            view.clear_buffer_for_testing(ctx);

            // Flags should be reset
            assert!(
                !view.any_session_contains_remote_blocks,
                "Terminal should not have remote blocks after clearing"
            );
            assert!(
                !view.any_session_contains_restored_remote_blocks,
                "Terminal should not have restored remote blocks after clearing"
            );
        });
    })
}

fn assert_block_has_find_match(find_model: &TerminalFindModel, block_index: BlockIndex) {
    assert!(
        find_model
            .block_list_find_run()
            .is_some_and(|run| run.matches_for_block(block_index).next().is_some())
    );
}

impl TerminalView {
    fn is_top_of_active_block_in_viewport(
        &self,
        model: &TerminalModel,
        input_mode: InputMode,
        app: &AppContext,
    ) -> bool {
        let active_block_index = model.block_list().active_block_index();
        let viewport = self.viewport_state(model.block_list(), input_mode, app);
        viewport.is_block_in_view(active_block_index, BlockVisibilityMode::TopOfBlockVisible)
    }

    fn scroll_top_in_lines(
        &self,
        model: &TerminalModel,
        input_mode: InputMode,
        app: &AppContext,
    ) -> Lines {
        let viewport = self.viewport_state(model.block_list(), input_mode, app);
        viewport.scroll_top_in_lines()
    }

    fn is_vertically_scrollable(&self, app: &AppContext) -> bool {
        let total_block_heights = self
            .model
            .lock()
            .block_list()
            .block_heights()
            .summary()
            .height;
        let visible_rows = self.content_element_height_lines(app);
        heights_approx_gt(total_block_heights, visible_rows)
    }
}

fn read_from_clipboard(ctx: &mut ViewContext<TerminalView>) -> String {
    TerminalView::read_from_clipboard(Some(ShellFamily::Posix), ctx)
}

#[test]
fn test_insert() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        let select_text = |view: &mut TerminalView, ctx: &mut ViewContext<TerminalView>| {
            {
                let mut model = view.model.lock();
                model.start_command_execution();
                let blocks = model.block_list_mut();
                blocks.input('f');
                blocks.linefeed();
                blocks.preexec(PreexecValue::default());
                blocks.on_finish_byte_processing(&ansi::ProcessorInput::new(&[]));
            }
            view.begin_block_text_selection(
                BlockListPoint::new(1.0, 1),
                Side::Right,
                SelectionType::Semantic,
                Vector2F::zero(),
                ctx,
            );
            view.end_text_selection(ctx);
        };
        let assert_input_text_eq = |app: &mut App, expected_text: &str| {
            terminal.read(app, |view, _ctx| {
                view.input.read(app, |view, ctx| {
                    assert_eq!(view.buffer_text(ctx), String::from(expected_text));
                });
            });
        };
        let assert_selected_blocks_cardinality_eq =
            |app: &mut App, expected_cardinality: BlockSelectionCardinality| {
                terminal.read(app, |view, _ctx| {
                    assert_eq!(
                        view.selected_blocks.cardinality().as_keymap_context_value(),
                        expected_cardinality.as_keymap_context_value()
                    );
                });
            };
        let assert_selected_text_eq = |app: &mut App, expected_text: Option<String>| {
            terminal.update(app, |view, ctx| {
                let semantic_selection = SemanticSelection::as_ref(ctx);
                let model = view.model.lock();
                let context_selected_text =
                    model.selection_to_string(semantic_selection, false, ctx);
                assert_eq!(context_selected_text, expected_text);
            });
        };

        // Shell Mode: Nothing selected
        terminal.update(&mut app, |view, ctx| {
            view.focus_terminal(ctx);
            view.typed_characters_on_terminal("hello", ctx);
        });
        assert_input_text_eq(&mut app, "hello");
        assert_selected_blocks_cardinality_eq(&mut app, BlockSelectionCardinality::None);
        assert_selected_text_eq(&mut app, None);

        // Shell Mode: Block selected
        terminal.update(&mut app, |view, ctx| {
            view.selected_blocks.reset_to_single(BlockIndex::zero());
            view.focus_terminal(ctx);
            view.typed_characters_on_terminal("_this", ctx);
        });
        assert_input_text_eq(&mut app, "hello_this");
        assert_selected_blocks_cardinality_eq(&mut app, BlockSelectionCardinality::None);
        assert_selected_text_eq(&mut app, None);

        // Shell Mode: Text selected
        terminal.update(&mut app, |view, ctx| {
            select_text(view, ctx);
            view.focus_terminal(ctx);
            view.typed_characters_on_terminal("_is", ctx);
        });
        assert_input_text_eq(&mut app, "hello_this_is");
        assert_selected_blocks_cardinality_eq(&mut app, BlockSelectionCardinality::None);
        assert_selected_text_eq(&mut app, None);

        // Activate Agent Mode, which should no longer allow text insertion to clear the selected block(s) or text
        terminal.update(&mut app, |view, ctx| {
            view.set_ai_input_mode_with_query(None, ctx);
        });

        // Agent Mode: Nothing selected
        terminal.update(&mut app, |view, ctx| {
            view.focus_terminal(ctx);
            view.typed_characters_on_terminal("_your", ctx);
        });
        assert_input_text_eq(&mut app, "hello_this_is_your");
        assert_selected_blocks_cardinality_eq(&mut app, BlockSelectionCardinality::None);
        assert_selected_text_eq(&mut app, None);

        // Agent Mode: Block selected
        terminal.update(&mut app, |view, ctx| {
            view.selected_blocks.reset_to_single(BlockIndex::zero());
            view.focus_terminal(ctx);
            view.typed_characters_on_terminal("_captain", ctx);
        });
        assert_input_text_eq(&mut app, "hello_this_is_your_captain");
        assert_selected_blocks_cardinality_eq(&mut app, BlockSelectionCardinality::One);
        assert_selected_text_eq(&mut app, None);

        // Agent Mode: Text selected
        terminal.update(&mut app, |view, ctx| {
            select_text(view, ctx);
            view.focus_terminal(ctx);
            view.typed_characters_on_terminal("_speaking", ctx);
        });
        assert_input_text_eq(&mut app, "hello_this_is_your_captain_speaking");
        assert_selected_blocks_cardinality_eq(&mut app, BlockSelectionCardinality::None);
        assert_selected_text_eq(&mut app, Some("f".to_owned()));
    })
}

const BODY_PREFIX: &str = "Latest output: ";

/// Regression test for CORE-1654. Tests the "Insert into Input" functionality from the context menu.
#[test]
fn test_insert_into_input() {
    // Note that this is defined as a unit test rather than an integration test since it requires precise selections
    // (where we don't want UI updates making the test brittle, due to hardcoded mouse positions).
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        // TODO: Potentially explore if we can re-use helpers from `input_test.rs` (`select_first_command_line_of_block` and `insert_dummy_block`).
        terminal.update(&mut app, |terminal_view, ctx| {
            {
                let mut terminal_model = terminal_view.model.lock();
                let blocks = terminal_model.block_list_mut();
                // Add two lines to the command grid and output grid in a new block.
                let block_index = insert_block(blocks, "cmd_a\ncmd_b\n", "output_a\noutput_b\n");
                let block = blocks.block_at(block_index).expect("block should exist");
                // Selections are inclusive of endpoint, hence we need to identify the last column to select the first command.
                let block_command_columns =
                    block.prompt_and_command_grid().grid_handler().columns();
                let command_grid_offset = block.command_grid_offset();
                // Create a selection that just spans the first line of the command grid in the block.
                blocks.start_selection(
                    BlockListPoint::new(command_grid_offset, 0),
                    SelectionType::Simple,
                    Side::Left,
                );
                blocks.update_selection(
                    BlockListPoint::new(command_grid_offset, block_command_columns),
                    Side::Right,
                );
                let selection = blocks.selection();
                assert!(selection.is_some());
            }

            terminal_view.context_menu_insert_selected_text(ctx);
        });

        // Confirm that the blocklist selection is cleared upon inserting into the input box.
        terminal.read(&app, |terminal_view, _ctx| {
            let terminal_model = terminal_view.model.lock();
            let blocks = terminal_model.block_list();
            let selection = blocks.selection();
            assert!(
                selection.is_none(),
                "Expected no selections in the blocklist but got {selection:?}"
            );
        });
        let input = terminal.read(&app, |terminal, _ctx| terminal.input().clone());
        // Confirm that the input box has the correct text (the first line of the command grid was selected above).
        input.read(&app, |input, ctx| {
            assert_eq!(input.buffer_text(ctx), "cmd_a");
        });
    });
}

#[test]
fn test_copy_on_select() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        // Add some text and make sure we update the selection
        terminal.update(&mut app, |view, ctx| {
            {
                let mut model = view.model.lock();
                model.start_command_execution();
                let blocks = model.block_list_mut();

                blocks.input('f');
                blocks.input('o');
                blocks.input('o');

                blocks.linefeed();

                blocks.preexec(PreexecValue::default());

                blocks.on_finish_byte_processing(&ansi::ProcessorInput::new(&[]));
            }

            view.begin_block_text_selection(
                BlockListPoint::new(1.0, 1),
                Side::Right,
                SelectionType::Semantic,
                Vector2F::zero(),
                ctx,
            );

            let selection_settings = SelectionSettings::as_ref(ctx);
            assert!(selection_settings.copy_on_select_enabled());
            assert_eq!("", &read_from_clipboard(ctx));
            view.end_text_selection(ctx);
            assert_eq!("foo", &read_from_clipboard(ctx));
        });
    })
}

#[test]
fn test_alt_screen_copy_on_select() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            {
                // Enter alt screen and add text
                let mut model = view.model.lock();
                model.set_mode(ansi::Mode::SwapScreen {
                    save_cursor_and_clear_screen: true,
                });
                assert!(model.is_alt_screen_active());

                model.alt_screen_mut().input('h');
            }
            // Ensure copy on select is enabled
            let selection_settings = SelectionSettings::as_ref(ctx);
            assert!(selection_settings.copy_on_select_enabled());

            // Select input
            view.begin_alt_selection(Point::new(0, 0), Side::Left, SelectionType::Simple, ctx);
            assert_eq!("", &read_from_clipboard(ctx));
            view.update_alt_selection(Point::new(0, 2), Side::Left, &Lines::zero(), ctx);
            view.end_alt_selection(ctx);
            // Ensure selection is copied
            assert_eq!("h", &read_from_clipboard(ctx));
        });
    })
}

#[test]
fn test_alt_screen_select_with_sgr_mouse() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let (window_id, terminal) = add_window_with_id_and_terminal(&mut app, None);

        let mut updated = EntityIdSet::default();
        updated.insert(app.root_view_id(window_id).unwrap());
        let invalidation = WindowInvalidation {
            updated,
            ..Default::default()
        };
        let presenter = Rc::new(RefCell::new(Presenter::new(window_id)));

        let semantic_selection = SemanticSelection::mock(true, "");

        let size_info = terminal.update(&mut app, |view, ctx| {
            {
                // Enter alt screen and enable SGR Mouse
                let mut model = view.model.lock();
                model.set_mode(ansi::Mode::SwapScreen {
                    save_cursor_and_clear_screen: true,
                });
                model.set_mode(ansi::Mode::SgrMouse);
                assert!(model.is_alt_screen_active());
                assert!(!should_intercept_mouse(&model, false, ctx));
                assert!(should_intercept_mouse(&model, true, ctx));

                // Write a bunch of characters into the alt screen.
                // ABCDEFG
                // HIJKLMN
                // OPQRSTU
                // VWXYZ[\
                // ]^_`abc
                // defghij
                // klmnopq
                // rstuvwx
                // yz{|}~
                // € ‚ƒ„…†
                // ‡ˆ‰Š‹Œ
                let mut ascii: u8 = 65;
                for _ in 0..view.size_info.rows {
                    for _ in 0..view.size_info.columns {
                        model.alt_screen_mut().input(ascii as char);
                        ascii += 1;
                    }
                }

                *view.size_info
            }
        });

        // We need to manually trigger re-renders to ensure the AltScreenElement is recreated, e.g.
        // so its `is_terminal_selecting` property will be up-to-date.
        macro_rules! rerender {
            ($app:ident, $presenter:expr_2021, $invalidation:expr_2021, $size_info:expr_2021) => {
                app.update(enclose!((presenter, invalidation) move |ctx| {
                    presenter
                        .borrow_mut()
                        .invalidate(invalidation, ctx);
                    presenter.borrow_mut().build_scene(
                        vec2f(size_info.pane_width_px, size_info.pane_height_px),
                        1.,
                        None,
                        ctx,
                    );
                }));
            }
        }

        // The start and end positions corresponds to 'J'
        // and 'a' in the grid, respectively.
        //
        // We adjust the vertical coordinates to account for padding
        // in the alt-screen.
        let start_position = vec2f(
            2. * size_info.cell_width_px.as_f32(),
            2. * size_info.cell_height_px.as_f32() - 1.,
        );
        let end_position = vec2f(
            5. * size_info.cell_width_px.as_f32(),
            5. * size_info.cell_height_px.as_f32() - 1.,
        );

        // Simulate a mouse drag from the "J" to the "a" cell.
        rerender!(app, presenter, invalidation, size_info);
        app.update(enclose!((presenter) move |ctx| {
            ctx.simulate_window_event(
                torchui::Event::LeftMouseDown {
                    position: start_position,
                    modifiers: Default::default(),
                    click_count: 1,
                    is_first_mouse: false,
                },
                window_id,
                presenter.clone(),
            );
        }));
        rerender!(app, presenter, invalidation, size_info);
        app.update(enclose!((presenter) move |ctx| {
            ctx.simulate_window_event(
                torchui::Event::LeftMouseDragged {
                    position: end_position,
                    modifiers: Default::default(),
                },
                window_id,
                presenter.clone(),
            );
        }));
        rerender!(app, presenter, invalidation, size_info);
        app.update(enclose!((presenter) move |ctx| {
            ctx.simulate_window_event(
                torchui::Event::LeftMouseUp {
                    position: end_position,
                    modifiers: Default::default(),
                },
                window_id,
                presenter.clone(),
            );
        }));

        // No selection should've occurred as we aren't intercepting mouse events.
        terminal.read(&app, |view, ctx| {
            let selected_text =
                view.model
                    .lock()
                    .selection_to_string(&semantic_selection, false, ctx);
            assert_eq!(selected_text, None);
        });

        // This time, hold Shift key for all mouse events.
        rerender!(app, presenter, invalidation, size_info);
        app.update(enclose!((presenter) move |ctx| {
            ctx.simulate_window_event(
                torchui::Event::LeftMouseDown {
                    position: start_position,
                    modifiers: ModifiersState {
                        shift: true,
                        ..Default::default()
                    },
                    click_count: 1,
                    is_first_mouse: false,
                },
                window_id,
                presenter.clone(),
            );
        }));
        rerender!(app, presenter, invalidation, size_info);
        app.update(enclose!((presenter) move |ctx| {
            ctx.simulate_window_event(
                torchui::Event::LeftMouseDragged {
                    position: end_position,
                    modifiers: ModifiersState {
                        shift: true,
                        ..Default::default()
                    },
                },
                window_id,
                presenter.clone(),
            );
        }));
        rerender!(app, presenter, invalidation, size_info);
        app.update(enclose!((presenter) move |ctx| {
            ctx.simulate_window_event(
                torchui::Event::LeftMouseUp {
                    position: end_position,
                    modifiers: ModifiersState {
                        shift: true,
                        ..Default::default()
                    },
                },
                window_id,
                presenter.clone(),
            );
        }));

        // This time we expect a selection since the Shift key had been held for this mouse drag.
        terminal.read(&app, |view, ctx| {
            let selected_text =
                view.model
                    .lock()
                    .selection_to_string(&semantic_selection, false, ctx);
            assert_eq!(selected_text.as_ref().unwrap(), "JKLMNOPQRSTUVWXYZ[\\]^_`a");
        });
    })
}

// Regression test for WAR-3433 on find bar selection crash.
#[test]
fn test_find_bar_select() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        // Add some text and make sure we update the selection
        terminal.update(&mut app, |view, ctx| {
            // Mock a block with content 'foo g'.
            {
                let mut model = view.model.lock();
                model.start_command_execution();
                let blocks = model.block_list_mut();

                blocks.input('f');
                blocks.input('o');
                blocks.input('o');

                blocks.input(' ');
                blocks.input('g');

                blocks.linefeed();

                blocks.preexec(PreexecValue::default());

                blocks.on_finish_byte_processing(&ansi::ProcessorInput::new(&[]));
            }

            // Select 'foo'.
            view.begin_block_text_selection(
                BlockListPoint::new(1.0, 1),
                Side::Right,
                SelectionType::Semantic,
                Vector2F::zero(),
                ctx,
            );

            let selection_settings = SelectionSettings::as_ref(ctx);
            assert!(selection_settings.copy_on_select_enabled());
            assert_eq!("", &read_from_clipboard(ctx));
            view.end_text_selection(ctx);
            assert_eq!("foo", &read_from_clipboard(ctx));

            // Show find bar. The find bar should have selected text 'foo' in its editor.
            view.show_find_bar(ctx);
            view.find_bar.read(ctx, |find, ctx| {
                find.editor().read(ctx, |editor, ctx| {
                    assert_eq!("foo".to_string(), editor.selected_text(ctx));
                })
            });

            // Now select 'foo g'.
            view.begin_block_text_selection(
                BlockListPoint::new(1.0, 1),
                Side::Right,
                SelectionType::Lines,
                Vector2F::zero(),
                ctx,
            );

            let selection_settings = SelectionSettings::as_ref(ctx);
            assert!(selection_settings.copy_on_select_enabled());
            assert_eq!("foo", &read_from_clipboard(ctx));
            view.end_text_selection(ctx);
            assert_eq!("foo g", &read_from_clipboard(ctx));

            // Show find bar. The find bar should have selected text 'foo g' in its editor.
            view.show_find_bar(ctx);
            view.find_bar.read(ctx, |find, ctx| {
                find.editor().read(ctx, |editor, ctx| {
                    assert_eq!("foo g".to_string(), editor.selected_text(ctx));
                })
            });
        });
    })
}

#[test]
fn test_viewport_iter_most_recent_at_bottom() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            let mut model = view.model.lock();
            model.simulate_block("ls", "foo");
            model.simulate_block("echo multiline", "bar\nhey");
            let viewport = view.viewport_state(model.block_list(), InputMode::PinnedToBottom, ctx);
            let mut iter = viewport.iter();
            let first_block = iter.next().expect("item 1");
            assert_eq!(
                Some(std::convert::Into::<BlockIndex>::into(1)),
                first_block.block_index
            );
            assert_eq!(
                std::convert::Into::<TotalIndex>::into(1),
                first_block.entry_index
            );
            assert!(first_block.block_height_item.height().into_lines() > Lines::zero());
            assert_eq!(
                Some(std::convert::Into::<BlockIndex>::into(1)),
                viewport.topmost_visible_block()
            );

            let second_block = iter.next().expect("item 2");
            assert_eq!(
                Some(std::convert::Into::<BlockIndex>::into(2)),
                second_block.block_index
            );
            assert_eq!(
                std::convert::Into::<TotalIndex>::into(2),
                second_block.entry_index
            );
            assert!(
                second_block.block_height_item.height() > first_block.block_height_item.height()
            );
            assert!(viewport.is_block_in_view(
                std::convert::Into::<BlockIndex>::into(2),
                BlockVisibilityMode::TopOfBlockVisible
            ));

            let third_block = iter.next().expect("item 3");
            assert_eq!(
                Some(std::convert::Into::<BlockIndex>::into(3)),
                third_block.block_index
            );
            assert_eq!(
                std::convert::Into::<TotalIndex>::into(3),
                third_block.entry_index
            );
            assert_eq!(0., third_block.block_height_item.height().as_f64());
        });
    })
}

#[test]
fn test_viewport_iter_most_recent_at_top() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx: &mut ViewContext<'_, TerminalView>| {
            let mut model = view.model.lock();
            model.simulate_block("ls", "foo");
            model.simulate_block("echo multiline", "bar\nhey");
            let viewport = view.viewport_state(model.block_list(), InputMode::PinnedToTop, ctx);
            let mut iter = viewport.iter();
            let echo_block = iter.next().expect("item 2");
            assert_eq!(
                Some(std::convert::Into::<BlockIndex>::into(2)),
                echo_block.block_index
            );
            assert_eq!(
                std::convert::Into::<TotalIndex>::into(2),
                echo_block.entry_index
            );
            assert!(echo_block.block_height_item.height().into_lines() > Lines::zero());
            assert_eq!(Pixels::zero(), viewport.offset_to_top_of_first_block(ctx));
            assert_eq!(
                Some(std::convert::Into::<BlockIndex>::into(2)),
                viewport.topmost_visible_block()
            );
            assert!(viewport.is_block_in_view(
                std::convert::Into::<BlockIndex>::into(2),
                BlockVisibilityMode::TopOfBlockVisible
            ));

            let ls_block = iter.next().expect("item 1");
            assert_eq!(
                Some(std::convert::Into::<BlockIndex>::into(1)),
                ls_block.block_index
            );
            assert_eq!(
                std::convert::Into::<TotalIndex>::into(1),
                ls_block.entry_index
            );
            assert!(
                echo_block.block_height_item.height().as_f64()
                    > ls_block.block_height_item.height().as_f64()
            );
        });
    })
}

#[test]
fn test_viewport_most_recent_at_top() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            let mut model = view.model.lock();
            model.simulate_block("ls", "foo");
            model.simulate_block("echo multiline", "bar\nhey");
            let viewport = view.viewport_state(model.block_list(), InputMode::PinnedToTop, ctx);
            // Most recent block should be visible.
            let topmost_visible_block = viewport.topmost_visible_block().unwrap();
            assert!(viewport.is_block_in_view(
                topmost_visible_block,
                BlockVisibilityMode::TopOfBlockVisible
            ));
            assert_eq!(Pixels::zero(), viewport.offset_to_top_of_first_block(ctx));
            assert_eq!(0., viewport.scroll_top_in_lines().as_f64());
            assert!(matches!(
                viewport.next_scroll_position(
                    ScrollPositionUpdate::AfterScrollEvent {
                        scroll_delta: 1.0.into_lines()
                    },
                    ctx
                ),
                ScrollPosition::FixedAtPosition { .. }
            ));
            assert_eq!(
                Lines::zero(),
                viewport.top_of_block_in_lines(topmost_visible_block)
            );
            assert!(matches!(
                viewport.scroll_position_at_bottom_of_block(topmost_visible_block),
                ScrollPosition::FollowsBottomOfMostRecentBlock
            ));
            let block_list_point = viewport
                .screen_coord_to_blocklist_point(
                    vec2f(0., 0.),
                    SnackbarPoint {
                        coord: vec2f(0., 0.),
                        translation_mode: SnackbarTranslationMode::WithinSnackbar,
                    },
                    ClampingMode::ClampToGrid,
                )
                .unwrap();
            assert_eq!(
                Some(2.into()),
                viewport.block_index_from_point(block_list_point)
            );
        });
    })
}

#[test]
fn test_scroll_fixed_to_bottom() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.read(&app, |view, _| {
            assert_eq!(
                view.scroll_position(),
                ScrollPosition::FollowsBottomOfMostRecentBlock
            );
        });
        terminal.update(&mut app, |view, ctx| {
            {
                let mut model = view.model.lock();
                // Put in enough blocks so that the view should be scrollable
                for _ in 0..100 {
                    model.simulate_block("ls", "foo");
                }
            }
            assert!(view.is_vertically_scrollable(ctx));
            assert_eq!(
                view.scroll_position(),
                ScrollPosition::FollowsBottomOfMostRecentBlock
            );
            view.scroll(1.0.into_lines(), ctx);

            let expected_scroll_top = {
                let model = view.model.lock();
                model.block_list().block_heights().summary().height
                    - view.content_element_height_lines(ctx)
                    - 1.0.into_lines()
            };
            assert_eq!(
                view.scroll_position(),
                ScrollPosition::FixedAtPosition {
                    scroll_lines: ScrollLines::ScrollTop(expected_scroll_top)
                },
            );
            // Now add to the active block and make sure we don't scroll
            {
                let mut model = view.model.lock();
                model.simulate_cmd("test");
            }
            {
                let mut model = view.model.lock();
                for _ in 0..100 {
                    model.linefeed();
                }
            }
            assert_eq!(
                view.scroll_position(),
                ScrollPosition::FixedAtPosition {
                    scroll_lines: ScrollLines::ScrollTop(expected_scroll_top)
                },
            );
        });
    })
}

#[test]
fn test_scroll_to_row() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            {
                let mut model = view.model.lock();
                // Put in enough blocks so that the view should be scrollable
                for _ in 0..50 {
                    model.simulate_block("ls", "foo\nfie\nfay\nfoe\nfum");
                }
            }

            assert!(view.is_vertically_scrollable(ctx));
            assert_eq!(
                view.scroll_position(),
                ScrollPosition::FollowsBottomOfMostRecentBlock
            );

            // Scroll upwards (no snackbar)
            let a = BlockListPoint::new(30.0, 0);
            view.scroll_to_row_if_not_visible(a.row.into_lines(), ctx);
            assert_eq!(
                view.scroll_position(),
                ScrollPosition::FixedAtPosition {
                    scroll_lines: ScrollLines::ScrollTop(30.0.into_lines())
                }
            );

            // Don't scroll at all
            let b = BlockListPoint::new(38.0, 0);
            view.scroll_to_row_if_not_visible(b.row.into_lines(), ctx);
            assert_eq!(
                view.scroll_position(),
                ScrollPosition::FixedAtPosition {
                    scroll_lines: ScrollLines::ScrollTop(30.0.into_lines())
                }
            );

            // Scroll downwards
            let c = BlockListPoint::new(100.0, 0);
            view.scroll_to_row_if_not_visible(c.row.into_lines(), ctx);
            assert_eq!(
                view.scroll_position(),
                ScrollPosition::FixedAtPosition {
                    scroll_lines: ScrollLines::ScrollTop(90.5.into_lines())
                }
            );
        });
    })
}

#[test]
fn test_stable_scrolling_during_grid_truncation() {
    App::test((), |mut app| async move {
        const MAX_GRID_SIZE: usize = 50;
        const INPUT_MODE: InputMode = InputMode::PinnedToBottom;

        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        // Note: this test is done in a single `update` to prevent
        // any changes in the presenter's position cache throughout.
        terminal.update(&mut app, |view, ctx| {
            // Set up the block list by creating a long-running
            // block that spans the entire viewport.
            {
                let mut model = view.model.lock();
                model.update_max_grid_size(MAX_GRID_SIZE);

                // Create a dummy, finished block and a long-running block.
                model.simulate_block("ls", "foo");
                model.simulate_long_running_block("cat", "");
                assert!(
                    model
                        .block_list()
                        .active_block()
                        .is_active_and_long_running()
                );

                // Add enough newlines so that the long-running block spans at
                // least the viewport and surely exceeds the grid size.
                let mut i = 0;
                while view.is_top_of_active_block_in_viewport(&model, INPUT_MODE, ctx)
                    || i < MAX_GRID_SIZE * 2
                {
                    model.process_bytes("\n");
                    i += 1;
                }
            }

            // Scroll up one line.
            assert_eq!(
                view.scroll_position(),
                ScrollPosition::FollowsBottomOfMostRecentBlock
            );
            view.scroll(1.into_lines(), ctx);
            assert!(matches!(
                view.scroll_position(),
                ScrollPosition::FixedWithinLongRunningBlock { .. }
            ));

            // Introduce new lines and make sure the scroll-top is adjusted as expected.
            {
                let mut model = view.model.lock();
                let active_block_index = model.block_list().active_block_index();
                let scroll_top_before_scrolling = view.scroll_top_in_lines(&model, INPUT_MODE, ctx);

                // To get to the top of the block, we need 50 lines for output grid and
                // then one line for command grid.
                for i in 1..=(MAX_GRID_SIZE + 1) {
                    model.process_bytes("\n");

                    let actual_scroll_top = view.scroll_top_in_lines(&model, INPUT_MODE, ctx);
                    let expected_scroll_top = scroll_top_before_scrolling - i.into_lines();
                    assert_eq!(actual_scroll_top, expected_scroll_top);
                }

                // Flush one full line in case the top of the block doesn't perfectly
                // line up with full lines (e.g. due to padding).
                model.process_bytes("\n");

                // Any remaining newlines should not move the scroll-top;
                // it should be "locked" at the top of the block.
                for _ in 0..MAX_GRID_SIZE {
                    model.process_bytes("\n");

                    let viewport = view.viewport_state(model.block_list(), INPUT_MODE, ctx);
                    let actual_scroll_top = viewport.scroll_top_in_lines();
                    let expected_scroll_top = viewport.top_of_block_in_lines(active_block_index);
                    assert_eq!(actual_scroll_top, expected_scroll_top);
                }
            }

            // Scroll up one line, bringing the previous block into the viewport.
            view.scroll(1.into_lines(), ctx);
            assert!(matches!(
                view.scroll_position(),
                ScrollPosition::FixedAtPosition { .. }
            ));

            // Introduce newlines and make sure the scroll-top does _not_ change anymore.
            {
                let mut model = view.model.lock();
                let scroll_top_before_newlines = view.scroll_top_in_lines(&model, INPUT_MODE, ctx);

                for _ in 0..MAX_GRID_SIZE {
                    model.process_bytes("\n");

                    let new_scroll_top = view.scroll_top_in_lines(&model, INPUT_MODE, ctx);
                    assert_eq!(scroll_top_before_newlines, new_scroll_top);
                }
            }
        });
    })
}

#[test]
fn test_clear_buffer() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            {
                let mut model = view.model.lock();
                for _ in 0..10 {
                    model.simulate_block("ls", "foo");
                }

                assert!(!model.block_list().blocks().is_empty());
            }

            view.bookmark_block(&BlockIndex::zero(), ctx);
            view.clear_buffer(ctx);

            {
                let model = view.model.lock();

                // There should be only one precmd block.
                assert_eq!(model.block_list().blocks().len(), 1);
                assert_eq!(view.bookmarked_blocks.len(), 0);
            }
        });
    })
}

#[test]
fn test_context_menu_includes_clear_when_block_list_non_empty() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            {
                let mut model = view.model.lock();
                model.simulate_block("ls", "foo");
                assert!(!model.is_block_list_empty());
            }

            let menu_source = BlockListMenuSource::OutsideBlockRightClick {
                position_in_terminal_view: Vector2F::zero(),
            };
            let items = view.context_menu_items(&menu_source, ctx);
            let labels: Vec<&str> = items
                .iter()
                .filter_map(|item| item.fields().map(|fields| fields.label()))
                .collect();
            assert!(
                labels.contains(&"Clear Blocks"),
                "Expected `Clear Blocks` menu item, got {labels:?}"
            );
        });
    })
}

#[test]
fn test_context_menu_omits_clear_when_block_list_empty() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            {
                let model = view.model.lock();
                assert!(model.is_block_list_empty());
            }

            let menu_source = BlockListMenuSource::OutsideBlockRightClick {
                position_in_terminal_view: Vector2F::zero(),
            };
            let items = view.context_menu_items(&menu_source, ctx);
            let labels: Vec<&str> = items
                .iter()
                .filter_map(|item| item.fields().map(|fields| fields.label()))
                .collect();
            assert!(
                !labels.contains(&"Clear Blocks"),
                "Did not expect `Clear Blocks` menu item when block list is empty, got {labels:?}"
            );
        });
    })
}

#[test]
fn test_context_menu_omits_clear_for_text_right_click() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            {
                let mut model = view.model.lock();
                model.simulate_block("ls", "foo");
                assert!(!model.is_block_list_empty());
            }

            let menu_source = BlockListMenuSource::RegularTextRightClick {
                position_in_terminal_view: Vector2F::zero(),
            };
            let items = view.context_menu_items(&menu_source, ctx);
            let labels: Vec<&str> = items
                .iter()
                .filter_map(|item| item.fields().map(|fields| fields.label()))
                .collect();
            assert!(
                !labels.contains(&"Clear Blocks"),
                "Did not expect `Clear Blocks` in text-selection right-click menu, got {labels:?}"
            );
        });
    })
}

#[test]
fn test_clear_buffer_clears_autosuggestion() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            // Set a next command suggestion (empty input)
            view.input.update(ctx, |input, ctx| {
                input.editor().update(ctx, |editor, ctx| {
                    editor.set_autosuggestion(
                        "git status",
                        AutosuggestionLocation::EndOfBuffer,
                        AutosuggestionType::Command {
                            was_intelligent_autosuggestion: true,
                        },
                        ctx,
                    );
                });
            });

            // Verify autosuggestion is present
            view.input.read(ctx, |input, ctx| {
                input.editor().read(ctx, |editor, _ctx| {
                    assert!(
                        editor.active_autosuggestion(),
                        "Autosuggestion should be active before clear_buffer"
                    );
                });
            });

            // Clear the buffer
            view.clear_buffer(ctx);

            // Verify autosuggestion is cleared
            view.input.read(ctx, |input, ctx| {
                input.editor().read(ctx, |editor, _ctx| {
                    assert!(
                        !editor.active_autosuggestion(),
                        "Autosuggestion should be cleared after clear_buffer"
                    );
                });
            });
        });
    })
}

#[test]
fn test_bookmark_blocks_navigation() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            {
                let mut model = view.model.lock();
                for _ in 0..10 {
                    model.simulate_block("ls", "foo");
                }

                assert!(!model.block_list().blocks().is_empty());
            }

            view.bookmark_block(&BlockIndex::zero(), ctx);
            view.bookmark_block(&BlockIndex::from(1), ctx);
            view.bookmark_block(&BlockIndex::from(4), ctx);

            view.bookmark_up(ctx);
            assert_eq!(view.selected_blocks.tail(), Some(4.into()));
            view.bookmark_down(ctx);
            assert_eq!(view.selected_blocks.tail(), Some(0.into()));
            view.bookmark_up(ctx);
            assert_eq!(view.selected_blocks.tail(), Some(4.into()));
            view.bookmark_up(ctx);
            assert_eq!(view.selected_blocks.tail(), Some(1.into()));
            view.bookmark_up(ctx);
            assert_eq!(view.selected_blocks.tail(), Some(0.into()));
            view.bookmark_down(ctx);
            assert_eq!(view.selected_blocks.tail(), Some(1.into()));
            view.bookmark_down(ctx);
            assert_eq!(view.selected_blocks.tail(), Some(4.into()));
        });
    })
}

fn run_navigation_test(input_mode: InputMode) {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.read(&app, |view, _ctx| {
            assert_eq!(
                view.scroll_position(),
                ScrollPosition::FollowsBottomOfMostRecentBlock
            );
        });
        terminal.update(&mut app, |view, ctx| {
            InputModeSettings::handle(ctx).update(ctx, |input_mode_settings, ctx| {
                let _ = input_mode_settings.input_mode.set_value(input_mode, ctx);
            });

            {
                let mut model = view.model.lock();
                // Put in enough blocks so that the view should be scrollable
                for _ in 0..100 {
                    model.simulate_block("ls", "foo");
                }

                // Put in one block that is larger than the viewport height.
                model.simulate_block("ls", "foo\n".repeat(100).as_str())
            }

            assert!(view.is_vertically_scrollable(ctx));
            assert_eq!(
                view.scroll_position(),
                ScrollPosition::FollowsBottomOfMostRecentBlock
            );

            view.select_most_recent_blocks(1, ctx);
            assert_eq!(view.selected_blocks.tail(), Some(101.into()));

            view.select_less_recent_block(false /* is_shift_down */, ctx);
            assert_eq!(view.selected_blocks.tail(), Some(100.into()));

            view.select_more_recent_block(
                true,  /* is_cmd_down */
                false, /* is_shift_down */
                ctx,
            );
            assert_ne!(
                view.scroll_position(),
                ScrollPosition::FollowsBottomOfMostRecentBlock
            );
            assert_eq!(view.selected_blocks.tail(), Some(101.into()));

            view.select_more_recent_block(
                true,  /* is_cmd_down */
                false, /* is_shift_down */
                ctx,
            );
            if input_mode.is_inverted_blocklist() {
                // In the inverted case, we intentionally align to the
                // top of the most recent block here, not to its bottom
                assert!(matches!(
                    view.scroll_position(),
                    ScrollPosition::FixedAtPosition { .. }
                ));
            } else {
                assert_eq!(
                    view.scroll_position(),
                    ScrollPosition::FollowsBottomOfMostRecentBlock
                );
            }
            assert_eq!(view.selected_blocks.tail(), Some(101.into()));

            view.select_more_recent_block(
                true,  /* is_cmd_down */
                false, /* is_shift_down */
                ctx,
            );
            assert_eq!(view.selected_blocks.tail(), None);
        });
    });
}

#[test]
fn test_navigate_blocks() {
    run_navigation_test(InputMode::PinnedToBottom);
}

// #[test]
// fn test_navigate_blocks_inverted_blocklist() {
//     run_navigation_test(InputMode::PinnedToTop);
// }

#[test]
fn test_not_bootstrapped() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            let model = view.model.lock();
            assert!(view.is_input_box_visible(&model, ctx));
            drop(model);

            assert_eq!(view.active_session_path_if_local(ctx), None);
        });
    })
}

#[test]
fn test_block_select() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |view, ctx| {
            view.selected_blocks
                .toggle(10.into(), Some(11.into()), Some(9.into()));

            let single_mouse_down = BlockSelectAction::MouseDown(Some(1.into()));
            // On Mac, we use cmd-click to toggle block selections, but
            // we use ctrl-click on non-Mac platforms.
            let single_mouse_up = if cfg!(target_os = "macos") {
                BlockSelectAction::MouseUp {
                    block_index: 1.into(),
                    is_ctrl_down: false,
                    is_cmd_down: true,
                    is_shift_down: false,
                }
            } else {
                BlockSelectAction::MouseUp {
                    block_index: 1.into(),
                    is_ctrl_down: true,
                    is_cmd_down: false,
                    is_shift_down: false,
                }
            };
            view.block_select(&single_mouse_down, true, ctx);
            view.block_select(&single_mouse_up, true, ctx);
            assert!(view.selected_blocks.is_selected(1.into()));
            assert!(view.selected_blocks.is_selected(10.into()));

            let range_mouse_down = BlockSelectAction::MouseDown(Some(5.into()));
            let range_mouse_up = BlockSelectAction::MouseUp {
                block_index: 5.into(),
                is_ctrl_down: false,
                is_cmd_down: false,
                is_shift_down: true,
            };
            view.block_select(&range_mouse_down, true, ctx);
            view.block_select(&range_mouse_up, true, ctx);
            assert!(!view.selected_blocks.is_selected(10.into()));
            assert_eq!(view.selected_blocks_pivot_index(), Some(1.into()));
            assert_eq!(view.selected_blocks_tail_index(), Some(5.into()));
        });
    })
}

#[test]
fn test_select_all_blocks() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            {
                let mut model = view.model.lock();
                // Put in enough blocks so that the view should be scrollable
                for _ in 0..100 {
                    model.simulate_block("ls", "foo");
                }
            }
            assert!(view.is_vertically_scrollable(ctx));

            view.select_all_blocks(ctx);
            assert_eq!(view.selected_blocks_pivot_index().unwrap(), 1.into());
            assert_eq!(view.selected_blocks_tail_index().unwrap(), 100.into());
            for i in 1..100 {
                assert!(view.selected_blocks.is_selected(i.into()));
            }
        });
    })
}

#[test]
fn test_expand_selection_above_and_below() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            {
                let mut model = view.model.lock();
                // Put in enough blocks so that the view should be scrollable
                for _ in 0..100 {
                    model.simulate_block("ls", "foo");
                }
            }
            assert!(view.is_vertically_scrollable(ctx));

            // helper to ensure indices are all selected
            fn assert_all_selected(selected_blocks: &SelectedBlocks, indices: Vec<BlockIndex>) {
                for &idx in indices.iter() {
                    assert!(selected_blocks.is_selected(idx));
                }
            }

            view.selected_blocks
                .toggle(5.into(), Some(6.into()), Some(4.into()));
            assert_all_selected(&view.selected_blocks, vec![5.into()]);

            view.select_more_recent_block(
                false, /* is_cmd_down */
                true,  /* is_shift_down */
                ctx,
            );
            assert_all_selected(&view.selected_blocks, vec![5.into(), 6.into()]);

            view.select_more_recent_block(
                false, /* is_cmd_down */
                true,  /* is_shift_down */
                ctx,
            );
            assert_all_selected(&view.selected_blocks, vec![5.into(), 6.into(), 7.into()]);

            view.select_less_recent_block(true /* is_shift_down */, ctx);
            assert_all_selected(&view.selected_blocks, vec![5.into(), 6.into()]);

            view.select_less_recent_block(true /* is_shift_down */, ctx);
            assert_all_selected(&view.selected_blocks, vec![5.into()]);

            view.select_less_recent_block(true /* is_shift_down */, ctx);
            assert_all_selected(&view.selected_blocks, vec![5.into(), 4.into()]);

            view.select_more_recent_block(
                false, /* is_cmd_down */
                true,  /* is_shift_down */
                ctx,
            );
            assert_all_selected(&view.selected_blocks, vec![5.into()]);
        });
    })
}

#[test]
fn test_copy_blocks() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            let (first_command, first_output) = ("ls", "foo");
            let (second_command, second_output) = ("pwd", "bar");

            {
                let mut model = view.model.lock();
                model.simulate_block(first_command, first_output);
                model.simulate_block(second_command, second_output);
            }

            // select a single block
            view.selected_blocks.toggle(2.into(), None, Some(1.into()));

            // test copy for a single block
            view.copy_blocks(BlockEntity::Command, ctx);
            assert_eq!(read_from_clipboard(ctx), second_command.to_string());

            view.copy_blocks(BlockEntity::Output, ctx);
            assert_eq!(read_from_clipboard(ctx), second_output.to_string());

            view.copy_blocks(BlockEntity::CommandAndOutput, ctx);
            assert_eq!(
                read_from_clipboard(ctx),
                format!("{second_command}\n{second_output}")
            );

            // select another block (in reverse)
            view.selected_blocks.toggle(1.into(), Some(2.into()), None);

            // test copy semantics for multiple blocks
            view.copy_blocks(BlockEntity::Command, ctx);
            let expected_commands_str = format!("{first_command}\n{second_command}");
            assert_eq!(read_from_clipboard(ctx), expected_commands_str);

            view.copy_blocks(BlockEntity::Output, ctx);
            let expected_outputs_str = format!("{first_output}\n{second_output}");
            assert_eq!(read_from_clipboard(ctx), expected_outputs_str);

            view.copy_blocks(BlockEntity::CommandAndOutput, ctx);
            let expected_both_str =
                format!("{first_command}\n{first_output}\n{second_command}\n{second_output}");
            assert_eq!(read_from_clipboard(ctx), expected_both_str);
        });
    })
}

#[test]
fn test_reinput_blocks() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            let (first_command, first_output) = ("ls", "foo");
            let (second_command, second_output) = ("pwd", "bar");

            {
                let mut model = view.model.lock();
                model.simulate_block(first_command, first_output);
                model.simulate_block(second_command, second_output);
            }

            // test reinput command for single block
            view.selected_blocks.toggle(2.into(), None, Some(1.into()));
            view.reinput_commands(false /* as_root */, ctx);
            assert_eq!(view.input().as_ref(ctx).buffer_text(ctx), second_command);

            view.selected_blocks.toggle(2.into(), None, Some(1.into()));
            view.reinput_commands(true /* as_root */, ctx);
            assert_eq!(
                view.input().as_ref(ctx).buffer_text(ctx),
                format!("sudo {second_command}")
            );

            // test reinput commands for multiple blocks (selected in reverse)
            view.selected_blocks.toggle(2.into(), None, Some(1.into()));
            view.selected_blocks.toggle(1.into(), Some(2.into()), None);
            view.reinput_commands(false /* as_root */, ctx);
            assert_eq!(
                view.input().as_ref(ctx).buffer_text(ctx),
                format!("{first_command}\n{second_command}")
            );

            view.selected_blocks.toggle(2.into(), None, Some(1.into()));
            view.selected_blocks.toggle(1.into(), Some(2.into()), None);
            view.reinput_commands(true /* as_root */, ctx);
            assert_eq!(
                view.input().as_ref(ctx).buffer_text(ctx),
                format!("sudo {first_command}\nsudo {second_command}")
            );
        });
    })
}

fn run_find_test(input_mode: InputMode) {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            InputModeSettings::handle(ctx).update(ctx, |input_mode_settings, ctx| {
                let _ = input_mode_settings.input_mode.set_value(input_mode, ctx);
            });

            let (first_command, first_output) = ("ls", "foo");
            let (second_command, second_output) = ("pwd", "foobar foo beans");
            let (third_command, third_output) = ("fools", "baz");

            {
                let mut model = view.model.lock();
                model.simulate_block(first_command, first_output);
                model.simulate_block(second_command, second_output);
                model.simulate_block(third_command, third_output);
            }

            view.show_find_bar(ctx);

            // Test without find_in_block enabled (results should be selection-agnostic)
            view.find_bar.update(ctx, |view, _ctx| {
                view.display_find_within_block = FindWithinBlockState::Disabled;
            });

            // find when no block is selected
            view.handle_find_event(
                &FindEvent::Update {
                    query: Some("foo".to_string()),
                },
                ctx,
            );
            assert_eq!(
                view.find_model.as_ref(ctx).visible_block_list_match_count(),
                4
            );
            assert_eq!(
                view.find_model
                    .as_ref(ctx)
                    .block_list_find_run()
                    .expect("BlockListFindRun exists.")
                    .focused_match_block_index()
                    .expect("Focused match exists."),
                3.into()
            );
            view.handle_find_event(
                &FindEvent::NextMatch {
                    direction: FindDirection::Down,
                },
                ctx,
            );
            if input_mode.is_inverted_blocklist() {
                // should go "down" to middle block
                assert_eq!(
                    view.find_model
                        .as_ref(ctx)
                        .block_list_find_run()
                        .expect("BlockListFindRun exists.")
                        .focused_match_block_index()
                        .expect("Focused match exists."),
                    2.into()
                );
            } else {
                // should loop to earliest block
                assert_eq!(
                    view.find_model
                        .as_ref(ctx)
                        .block_list_find_run()
                        .expect("BlockListFindRun exists.")
                        .focused_match_block_index()
                        .expect("Focused match exists."),
                    1.into()
                );
            }

            // find when a single block is selected
            view.selected_blocks.reset_to_single(2.into());
            view.handle_find_event(
                &FindEvent::Update {
                    query: Some("ls".to_string()),
                },
                ctx,
            );
            assert_eq!(
                view.find_model.as_ref(ctx).visible_block_list_match_count(),
                2
            );
            assert_block_has_find_match(view.find_model.as_ref(ctx), 1.into());
            assert_block_has_find_match(view.find_model.as_ref(ctx), 3.into());

            // Test with find_in_block enabled
            view.find_bar.update(ctx, |view, _ctx| {
                view.display_find_within_block = FindWithinBlockState::Enabled;
            });

            // find when no block is selected
            view.selected_blocks.reset();
            view.handle_find_event(
                &FindEvent::Update {
                    query: Some("foo".to_string()),
                },
                ctx,
            );
            assert_eq!(
                view.find_model.as_ref(ctx).visible_block_list_match_count(),
                0
            );

            // find when a single block is selected
            view.selected_blocks.reset_to_single(2.into());
            view.handle_find_event(
                &FindEvent::Update {
                    query: Some("pwd".to_string()),
                },
                ctx,
            );
            assert_eq!(
                view.find_model.as_ref(ctx).visible_block_list_match_count(),
                1
            );
            assert_block_has_find_match(view.find_model.as_ref(ctx), 2.into());

            // find when multiple blocks are selected, and find in block is enabled
            view.selected_blocks.toggle(3.into(), Some(2.into()), None);
            view.handle_find_event(
                &FindEvent::Update {
                    query: Some("foo".to_string()),
                },
                ctx,
            );
            assert_eq!(
                view.find_model.as_ref(ctx).visible_block_list_match_count(),
                3
            );
            assert_block_has_find_match(view.find_model.as_ref(ctx), 2.into());
            assert_block_has_find_match(view.find_model.as_ref(ctx), 3.into());
        });
    })
}

#[test]
fn test_find_in_blocks() {
    run_find_test(InputMode::PinnedToBottom);
}

#[test]
fn test_find_in_blocks_inverted_blocklist() {
    run_find_test(InputMode::PinnedToTop);
}

#[test]
fn test_case_sensitive_find() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            let (first_command, first_output) = ("ls", "foo");
            let (second_command, second_output) = ("pwd", "fOObar");
            let (third_command, third_output) = ("FoOls", "baz");

            {
                let mut model = view.model.lock();
                model.simulate_block(first_command, first_output);
                model.simulate_block(second_command, second_output);
                model.simulate_block(third_command, third_output);
            }

            view.show_find_bar(ctx);

            // Test without case sensitivity enabled (no blocks enabled)
            view.handle_find_event(
                &FindEvent::Update {
                    query: Some("fOO".to_string()),
                },
                ctx,
            );
            assert_eq!(
                view.find_model.as_ref(ctx).visible_block_list_match_count(),
                3
            );

            // Test without case sensitivity enabled, but with find in block
            view.find_bar.update(ctx, |view, _ctx| {
                view.display_find_within_block = FindWithinBlockState::Enabled;
            });
            view.selected_blocks.reset_to_single(1.into());
            view.update_find_selection(ctx);
            assert_eq!(
                view.find_model.as_ref(ctx).visible_block_list_match_count(),
                1
            );
            assert_block_has_find_match(view.find_model.as_ref(ctx), 1.into());

            // Test with case sensitivity enabled (one block enabled)
            view.handle_find_event(
                &FindEvent::ToggleCaseSensitivity {
                    is_case_sensitive: true,
                },
                ctx,
            );
            view.selected_blocks.reset_to_single(1.into());
            view.update_find_selection(ctx);
            assert_eq!(
                view.find_model.as_ref(ctx).visible_block_list_match_count(),
                0
            );

            view.selected_blocks.reset_to_single(2.into());
            view.update_find_selection(ctx);
            assert_eq!(
                view.find_model.as_ref(ctx).visible_block_list_match_count(),
                1
            );
            assert_block_has_find_match(view.find_model.as_ref(ctx), 2.into());

            // Test with case sensitivity enabled (no blocks enabled)
            view.selected_blocks.reset();
            view.find_bar.update(ctx, |view, _ctx| {
                view.display_find_within_block = FindWithinBlockState::Disabled;
            });
            assert_eq!(
                view.find_model.as_ref(ctx).visible_block_list_match_count(),
                1
            );
            assert_block_has_find_match(view.find_model.as_ref(ctx), 2.into());

            // Change regex to mismatch case sensitivity across all blocks
            view.handle_find_event(
                &FindEvent::Update {
                    query: Some("FOO".to_string()),
                },
                ctx,
            );
            assert_eq!(
                view.find_model.as_ref(ctx).visible_block_list_match_count(),
                0
            );
        });
    })
}

#[test]
fn test_find_bar_prefix_search() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            let (command_1, output_1) = ("echo foo", "foo");
            let (command_2, output_2) = ("echo bar foo", "bar foo");

            {
                let mut model = view.model.lock();
                model.simulate_block(command_1, output_1);
                model.simulate_block(command_2, output_2);
            }

            view.show_find_bar(ctx);

            // Test without regex enabled
            view.handle_find_event(
                &FindEvent::Update {
                    query: Some("^foo".to_string()),
                },
                ctx,
            );

            assert_eq!(
                view.find_model.as_ref(ctx).visible_block_list_match_count(),
                0
            );

            view.handle_find_event(
                &FindEvent::ToggleRegexSearch {
                    is_regex_enabled: true,
                },
                ctx,
            );

            assert_eq!(
                view.find_model.as_ref(ctx).visible_block_list_match_count(),
                1
            );
        });
    });
}

#[test]
fn test_create_notification_shorter_than_max() {
    let command = "cargo run";
    let output = "error: failed to find directory";
    let command_succeeded = false;
    let block_duration = Duration::new(4, 2);

    let trigger = NotificationsTrigger::LongRunningCommand(command_succeeded, block_duration);

    let actual_content =
        trigger.create_notification_content(command.to_string(), output.to_string());

    let expected_title = format!("'{command}' failed after 4s");
    let expected_body = format!("{BODY_PREFIX}{output}");

    assert_eq!(actual_content.title, expected_title);
    assert_eq!(actual_content.body, expected_body);
}

#[test]
fn test_create_notification_as_long_as_max() {
    let expected_title_suffix = " finished after 4s";
    let max_command_len = UserNotification::MAX_TITLE_LENGTH - expected_title_suffix.len() - 2;
    let command = "a".repeat(max_command_len);

    let max_output_len = UserNotification::MAX_BODY_LENGTH - BODY_PREFIX.len();
    let output = "a".repeat(max_output_len);

    let command_succeeded = true;
    let block_duration = Duration::new(4, 2);

    let trigger = NotificationsTrigger::LongRunningCommand(command_succeeded, block_duration);

    let actual_content =
        trigger.create_notification_content(command.to_string(), output.to_string());

    let expected_title = format!("'{command}'{expected_title_suffix}");
    let expected_body = format!("{BODY_PREFIX}{output}");

    assert_eq!(actual_content.title, expected_title);
    assert_eq!(actual_content.body, expected_body);
}

#[test]
fn test_create_notification_longer_than_max() {
    let expected_title_suffix = " finished after 4s";
    let max_command_len = UserNotification::MAX_TITLE_LENGTH - expected_title_suffix.len() - 2;
    let command = "a".repeat(max_command_len + 1);

    let max_output_len = UserNotification::MAX_BODY_LENGTH - BODY_PREFIX.len();
    let output = "a".repeat(max_output_len + 1);

    let command_succeeded = true;
    let block_duration = Duration::new(4, 2);

    let trigger = NotificationsTrigger::LongRunningCommand(command_succeeded, block_duration);

    let actual_content =
        trigger.create_notification_content(command.to_string(), output.to_string());

    let expected_title = format!(
        "'{}...'{expected_title_suffix}",
        &command[..max_command_len - 3]
    );
    let expected_body = format!("{BODY_PREFIX}...{}", &output[..max_output_len - 3]);

    assert_eq!(actual_content.title, expected_title);
    assert_eq!(actual_content.body, expected_body);
}

#[test]
fn test_create_notification_char_boundaries_respected() {
    let expected_title_suffix = " finished after 4s";
    let max_command_len = UserNotification::MAX_TITLE_LENGTH - expected_title_suffix.len() - 2;
    let command = "😊".repeat(max_command_len + 1);

    let output = "error: failed to find directory";
    let command_succeeded = true;
    let block_duration = Duration::new(4, 2);

    let trigger = NotificationsTrigger::LongRunningCommand(command_succeeded, block_duration);

    let actual_content = trigger.create_notification_content(command, output.to_string());

    let expected_command_prefix = "😊".repeat(max_command_len - 3);
    let expected_title = format!("'{expected_command_prefix}...'{expected_title_suffix}",);
    assert_eq!(actual_content.title, expected_title);
}

#[test]
fn test_banner_for_incompatible_plugins() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal =
            MockTerminalManager::create_new_terminal_view_window_for_test(&mut app, None);

        SessionSettings::handle(&app).update(&mut app, |session_settings, ctx| {
            let _ = session_settings.honor_ps1.set_value(true, ctx);
        });

        terminal.update(&mut app, |view, _ctx| {
            let mut model = view.model.lock();
            model.init_shell(InitShellValue {
                session_id: 0.into(),
                shell: "zsh".to_owned(),
                ..Default::default()
            });
            model.bootstrapped(BootstrappedValue {
                shell: "zsh".to_owned(),
                shell_plugins: Some(HashSet::from(["p10k_unsupported".to_string()])),
                ..Default::default()
            });
        });

        // This is asynchronous because we're waiting for the bootstrap event
        // to be sent from the terminal model to the terminal view.
        assert_eventually!(
            200 => terminal.read(&app, |view, _ctx| view
                .is_incompatible_configuration_banner_open),
            "Banner did not open in time"
        );
    })
}

/// Regression test for #9011: the slow-bootstrap banner used to persist
/// indefinitely when shell integration never sent the bootstrap signal
/// (e.g. the user's shell `exec`s into `expect` before Torch's integration
/// runs). The auto-dismiss timer scheduled when the banner opens must
/// eventually close it.
#[test]
fn test_slow_bootstrap_banner_auto_dismisses() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal =
            MockTerminalManager::create_new_terminal_view_window_for_test(&mut app, None);

        // Open the banner directly and schedule a short-duration auto-dismiss
        // timer. We bypass `on_bootstrap_failed_timer_complete` (which itself
        // waits the 7-second bootstrap timeout) to keep this test fast — the
        // important behavior under test is the auto-dismiss path.
        terminal.update(&mut app, |view, ctx| {
            view.is_slow_bootstrap_banner_open = true;
            view.slow_bootstrap_banner_auto_dismiss_handle = Some(
                view.start_slow_bootstrap_banner_auto_dismiss_timer(Duration::from_millis(50), ctx),
            );
        });

        assert!(terminal.read(&app, |view, _ctx| view.is_slow_bootstrap_banner_open));

        assert_eventually!(
            200 => terminal.read(&app, |view, _ctx| !view.is_slow_bootstrap_banner_open
                && view.slow_bootstrap_banner_auto_dismiss_handle.is_none()),
            "Slow bootstrap banner did not auto-dismiss"
        );
    })
}

/// Regression test for #9011: when the banner is dismissed by another path
/// (manual user dismissal or a successful bootstrap event), any pending
/// auto-dismiss timer should be aborted so it can't fire after the fact.
#[test]
fn test_hide_slow_bootstrap_banner_aborts_pending_auto_dismiss() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal =
            MockTerminalManager::create_new_terminal_view_window_for_test(&mut app, None);

        terminal.update(&mut app, |view, ctx| {
            view.is_slow_bootstrap_banner_open = true;
            view.slow_bootstrap_banner_auto_dismiss_handle =
                Some(view.start_slow_bootstrap_banner_auto_dismiss_timer(
                    // Long enough that the timer can't fire before we hide.
                    Duration::from_secs(60),
                    ctx,
                ));
            view.hide_slow_bootstrap_banner(ctx);
        });

        terminal.read(&app, |view, _ctx| {
            assert!(!view.is_slow_bootstrap_banner_open);
            assert!(view.slow_bootstrap_banner_auto_dismiss_handle.is_none());
        });
    })
}

// Regression test for GH#3548 / GH#6093: the "Seems like your completions are not
// working" banner must offer a permanent "Don't show me again" dismissal that is
// persisted, while the "x" close button keeps its existing per-session behavior.
#[test]
fn test_control_master_banner_permanent_dismissal_persists() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal =
            MockTerminalManager::create_new_terminal_view_window_for_test(&mut app, None);

        terminal.update(&mut app, |view, ctx| {
            // Temporary dismissal (the "x" button) clears the banner for this session
            // but must not persist a "don't show again" preference.
            view.control_master_error_banner_state.is_open = true;
            view.handle_controlmaster_error_banner_event(
                &BannerEvent::Dismiss(DismissalType::Temporary),
                ctx,
            );
            assert!(!view.control_master_error_banner_state.is_open);
            assert!(!view.control_master_error_banner_suppressed);
            assert_eq!(
                ctx.private_user_preferences()
                    .read_value(CONTROL_MASTER_BANNER_SUPPRESSED_KEY)
                    .unwrap(),
                None,
                "temporary dismissal should not persist a preference"
            );

            // Permanent dismissal ("Don't show me again") clears the banner and persists
            // the choice so it never reopens.
            view.control_master_error_banner_state.is_open = true;
            view.handle_controlmaster_error_banner_event(
                &BannerEvent::Dismiss(DismissalType::Permanent),
                ctx,
            );
            assert!(!view.control_master_error_banner_state.is_open);
            assert!(view.control_master_error_banner_suppressed);
            assert_eq!(
                ctx.private_user_preferences()
                    .read_value(CONTROL_MASTER_BANNER_SUPPRESSED_KEY)
                    .unwrap(),
                Some("true".to_owned()),
                "permanent dismissal should persist a preference"
            );
        });
    })
}

// Regression test for GH#3548 / GH#6093: once the banner has been permanently
// dismissed it must not reopen on subsequent sessions.
#[test]
fn test_control_master_banner_suppressed_does_not_reopen() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal =
            MockTerminalManager::create_new_terminal_view_window_for_test(&mut app, None);

        terminal.update(&mut app, |view, _ctx| {
            // With no remote server and no prior dismissal, the banner may open.
            view.control_master_error_banner_suppressed = false;
            assert!(view.should_open_control_master_banner(/* has_remote_server */ false));
            // A remote server makes the CTA irrelevant, so it stays closed.
            assert!(!view.should_open_control_master_banner(/* has_remote_server */ true));

            // Once permanently dismissed it must never reopen, even without a remote server.
            view.control_master_error_banner_suppressed = true;
            assert!(!view.should_open_control_master_banner(false));
        });
    })
}

#[test]
fn test_bash_vim_banner_already_shown() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal =
            MockTerminalManager::create_new_terminal_view_window_for_test(&mut app, None);

        // Ensure the terminal is the active session.
        terminal.update(&mut app, |view, ctx| {
            let terminal_pane_id = TerminalPaneId::dummy_terminal_pane_id();
            let focus_state = ctx.add_model(|_| {
                PaneGroupFocusState::new(terminal_pane_id.into(), Some(terminal_pane_id), false)
            });
            let focus_handle = PaneFocusHandle::new(terminal_pane_id.into(), focus_state);
            view.set_focus_handle(focus_handle, ctx);
        });

        // The banner has already been shown and dismissed.
        VimBannerSettings::handle(&app).update(&mut app, |banner_settings, ctx| {
            let _ = banner_settings
                .vim_keybindings_banner_state
                .set_value(BannerState::Dismissed, ctx);
        });

        // Ensure Torch's vim keybindings are off.
        AppEditorSettings::handle(&app).update(&mut app, |editor_settings, ctx| {
            let _ = editor_settings.vim_mode.set_value(false, ctx);
        });

        // Bootstrap a bash session with vi mode enabled.
        terminal.update(&mut app, |view, _ctx| {
            let mut model = view.model.lock();
            model.init_shell(InitShellValue {
                session_id: 0.into(),
                shell: "bash".to_owned(),
                ..Default::default()
            });
            model.bootstrapped(BootstrappedValue {
                shell: "bash".to_owned(),
                shell_options: Some(HashSet::from(["vi_mode".to_string()])),
                ..Default::default()
            });
        });

        // This is asynchronous because we're waiting for the bootstrap event
        // to be sent from the terminal model to the terminal view.
        assert_eventually!(
            // Since the user already dismissed the banner, it should not
            // be shown again.
            terminal.read(&app, |terminal, _terminal_ctx| {
                terminal.inline_banners_state.vim_banner_state.is_none()
            }),
            "Banner should not have opened"
        );
    })
}

#[test]
fn test_bash_vim_banner_on() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal =
            MockTerminalManager::create_new_terminal_view_window_for_test(&mut app, None);

        // Ensure the terminal is the active session.
        terminal.update(&mut app, |view, ctx| {
            let terminal_pane_id = TerminalPaneId::dummy_terminal_pane_id();
            let focus_state = ctx.add_model(|_| {
                PaneGroupFocusState::new(terminal_pane_id.into(), Some(terminal_pane_id), false)
            });
            let focus_handle = PaneFocusHandle::new(terminal_pane_id.into(), focus_state);
            view.set_focus_handle(focus_handle, ctx);
        });

        // Ensure the banner has never been shown.
        VimBannerSettings::handle(&app).update(&mut app, |banner_settings, ctx| {
            let _ = banner_settings
                .vim_keybindings_banner_state
                .set_value(BannerState::NotDismissed, ctx);
        });

        // Ensure Torch's vim keybindings are off.
        AppEditorSettings::handle(&app).update(&mut app, |editor_settings, ctx| {
            let _ = editor_settings.vim_mode.set_value(false, ctx);
        });

        // Bootstrap a bash session with vi mode enabled.
        terminal.update(&mut app, |view, _ctx| {
            let mut model = view.model.lock();
            model.init_shell(InitShellValue {
                session_id: 0.into(),
                shell: "bash".to_owned(),
                ..Default::default()
            });
            model.bootstrapped(BootstrappedValue {
                shell: "bash".to_owned(),
                shell_options: Some(HashSet::from(["vi_mode".to_string()])),
                ..Default::default()
            });
        });

        // This is asynchronous because we're waiting for the bootstrap event
        // to be sent from the terminal model to the terminal view.
        assert_eventually!(
            // The vim keybinding banner should display.
            200 => terminal.read(&app, |terminal, _terminal_ctx| {
                terminal.inline_banners_state.vim_banner_state.is_some()
            }),
            "Banner did not open in time"
        );
    })
}

#[test]
fn test_bash_vim_banner_off() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal =
            MockTerminalManager::create_new_terminal_view_window_for_test(&mut app, None);

        // Ensure the terminal is the active session.
        terminal.update(&mut app, |view, ctx| {
            let terminal_pane_id = TerminalPaneId::dummy_terminal_pane_id();
            let focus_state = ctx.add_model(|_| {
                PaneGroupFocusState::new(terminal_pane_id.into(), Some(terminal_pane_id), false)
            });
            let focus_handle = PaneFocusHandle::new(terminal_pane_id.into(), focus_state);
            view.set_focus_handle(focus_handle, ctx);
        });

        // Ensure the banner has never been shown.
        VimBannerSettings::handle(&app).update(&mut app, |banner_settings, ctx| {
            let _ = banner_settings
                .vim_keybindings_banner_state
                .set_value(BannerState::NotDismissed, ctx);
        });

        // Ensure Torch's vim keybindings are on.
        AppEditorSettings::handle(&app).update(&mut app, |editor_settings, ctx| {
            let _ = editor_settings.vim_mode.set_value(true, ctx);
        });

        // Bootstrap a bash session with vi mode enabled.
        terminal.update(&mut app, |view, _ctx| {
            let mut model = view.model.lock();
            model.init_shell(InitShellValue {
                session_id: 0.into(),
                shell: "bash".to_owned(),
                ..Default::default()
            });
            model.bootstrapped(BootstrappedValue {
                shell: "bash".to_owned(),
                shell_options: Some(HashSet::from(["vi_mode".to_string()])),
                ..Default::default()
            });
        });

        // This is asynchronous because we're waiting for the bootstrap event
        // to be sent from the terminal model to the terminal view.
        assert_eventually!(
            // The vim keybinding banner should NOT display
            // because the user already has vim keybindings turned on.
            terminal.read(&app, |terminal, _terminal_ctx| {
                terminal.inline_banners_state.vim_banner_state.is_none()
            }),
            "Banner should not have opened"
        );
    })
}

#[test]
fn test_zsh_vim_banner_on() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal =
            MockTerminalManager::create_new_terminal_view_window_for_test(&mut app, None);

        // Ensure the terminal is the active session.
        terminal.update(&mut app, |view, ctx| {
            let terminal_pane_id = TerminalPaneId::dummy_terminal_pane_id();
            let focus_state = ctx.add_model(|_| {
                PaneGroupFocusState::new(terminal_pane_id.into(), Some(terminal_pane_id), false)
            });
            let focus_handle = PaneFocusHandle::new(terminal_pane_id.into(), focus_state);
            view.set_focus_handle(focus_handle, ctx);
        });

        // Ensure the banner has never been shown.
        VimBannerSettings::handle(&app).update(&mut app, |banner_settings, ctx| {
            let _ = banner_settings
                .vim_keybindings_banner_state
                .set_value(BannerState::NotDismissed, ctx);
        });

        // Ensure Torch's vim keybindings are off.
        AppEditorSettings::handle(&app).update(&mut app, |editor_settings, ctx| {
            let _ = editor_settings.vim_mode.set_value(false, ctx);
        });

        // Bootstrap a zsh session with vi mode enabled.
        terminal.update(&mut app, |view, _ctx| {
            let mut model = view.model.lock();
            model.init_shell(InitShellValue {
                session_id: 0.into(),
                shell: "zsh".to_owned(),
                ..Default::default()
            });
            model.bootstrapped(BootstrappedValue {
                shell: "zsh".to_owned(),
                shell_plugins: Some(HashSet::from(["vi".to_string()])),
                ..Default::default()
            });
        });

        // This is asynchronous because we're waiting for the bootstrap event
        // to be sent from the terminal model to the terminal view.
        assert_eventually!(
            // The vim keybinding banner should display.
            200 => terminal.read(&app, |terminal, _terminal_ctx| {
                terminal.inline_banners_state.vim_banner_state.is_some()
            }),
            "Banner did not open in time"
        );
    })
}

#[test]
fn test_zsh_vim_banner_off() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal =
            MockTerminalManager::create_new_terminal_view_window_for_test(&mut app, None);

        // Ensure the terminal is the active session.
        terminal.update(&mut app, |view, ctx| {
            let terminal_pane_id = TerminalPaneId::dummy_terminal_pane_id();
            let focus_state = ctx.add_model(|_| {
                PaneGroupFocusState::new(terminal_pane_id.into(), Some(terminal_pane_id), false)
            });
            let focus_handle = PaneFocusHandle::new(terminal_pane_id.into(), focus_state);
            view.set_focus_handle(focus_handle, ctx);
        });

        // Ensure the banner has never been shown.
        VimBannerSettings::handle(&app).update(&mut app, |banner_settings, ctx| {
            let _ = banner_settings
                .vim_keybindings_banner_state
                .set_value(BannerState::NotDismissed, ctx);
        });

        // Ensure Torch's vim keybindings are on.
        AppEditorSettings::handle(&app).update(&mut app, |editor_settings, ctx| {
            let _ = editor_settings.vim_mode.set_value(true, ctx);
        });

        // Bootstrap a zsh session with vi mode enabled.
        terminal.update(&mut app, |view, _ctx| {
            let mut model = view.model.lock();
            model.init_shell(InitShellValue {
                session_id: 0.into(),
                shell: "zsh".to_owned(),
                ..Default::default()
            });
            model.bootstrapped(BootstrappedValue {
                shell: "zsh".to_owned(),
                shell_plugins: Some(HashSet::from(["vi".to_string()])),
                ..Default::default()
            });
        });

        // This is asynchronous because we're waiting for the bootstrap event
        // to be sent from the terminal model to the terminal view.
        assert_eventually!(
            // The vim keybinding banner should NOT display
            // because the user already has vim keybindings turned on.
            terminal.read(&app, |terminal, _terminal_ctx| {
                terminal.inline_banners_state.vim_banner_state.is_none()
            }),
            "Banner should not have opened"
        );
    })
}

#[test]
fn test_fish_vim_banner_on() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal =
            MockTerminalManager::create_new_terminal_view_window_for_test(&mut app, None);

        // Ensure the terminal is the active session.
        terminal.update(&mut app, |view, ctx| {
            let terminal_pane_id = TerminalPaneId::dummy_terminal_pane_id();
            let focus_state = ctx.add_model(|_| {
                PaneGroupFocusState::new(terminal_pane_id.into(), Some(terminal_pane_id), false)
            });
            let focus_handle = PaneFocusHandle::new(terminal_pane_id.into(), focus_state);
            view.set_focus_handle(focus_handle, ctx);
        });

        // Ensure Torch's vim keybindings are off.
        AppEditorSettings::handle(&app).update(&mut app, |editor_settings, ctx| {
            let _ = editor_settings.vim_mode.set_value(false, ctx);
        });

        // Bootstrap a fish session with vi mode enabled.
        terminal.update(&mut app, |view, _ctx| {
            let mut model = view.model.lock();
            model.init_shell(InitShellValue {
                session_id: 0.into(),
                shell: "fish".to_owned(),
                ..Default::default()
            });
            model.bootstrapped(BootstrappedValue {
                shell: "fish".to_owned(),
                shell_options: Some(HashSet::from(["vi_mode".to_string()])),
                ..Default::default()
            });
        });

        // This is asynchronous because we're waiting for the bootstrap event
        // to be sent from the terminal model to the terminal view.
        assert_eventually!(
            // The vim keybinding banner should display.
            200 => terminal.read(&app, |terminal, _terminal_ctx| {
                terminal.inline_banners_state.vim_banner_state.is_some()
            }),
            "Banner did not open in time"
        );
    })
}

#[test]
fn test_fish_vim_banner_off() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal =
            MockTerminalManager::create_new_terminal_view_window_for_test(&mut app, None);

        // Ensure the terminal is the active session.
        terminal.update(&mut app, |view, ctx| {
            let terminal_pane_id = TerminalPaneId::dummy_terminal_pane_id();
            let focus_state = ctx.add_model(|_| {
                PaneGroupFocusState::new(terminal_pane_id.into(), Some(terminal_pane_id), false)
            });
            let focus_handle = PaneFocusHandle::new(terminal_pane_id.into(), focus_state);
            view.set_focus_handle(focus_handle, ctx);
        });

        // Ensure Torch's vim keybindings are on.
        AppEditorSettings::handle(&app).update(&mut app, |editor_settings, ctx| {
            let _ = editor_settings.vim_mode.set_value(true, ctx);
        });

        // Bootstrap a fish session with vi mode enabled.
        terminal.update(&mut app, |view, _ctx| {
            let mut model = view.model.lock();
            model.init_shell(InitShellValue {
                session_id: 0.into(),
                shell: "fish".to_owned(),
                ..Default::default()
            });
            model.bootstrapped(BootstrappedValue {
                shell: "fish".to_owned(),
                shell_options: Some(HashSet::from(["vi_mode".to_string()])),
                ..Default::default()
            });
        });

        // This is asynchronous because we're waiting for the bootstrap event
        // to be sent from the terminal model to the terminal view.
        assert_eventually!(
            // The vim keybinding banner should NOT display
            // because the user already has vim keybindings turned on.
            terminal.read(&app, |terminal, _terminal_ctx| {
                terminal.inline_banners_state.vim_banner_state.is_none()
            }),
            "Banner should not have opened"
        );
    })
}

#[test]
fn test_prompt_context_menu_items_for_ps1() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        SessionSettings::handle(&app).update(&mut app, |session_settings, ctx| {
            let _ = session_settings.honor_ps1.set_value(true, ctx);
        });

        terminal.read(&app, |view, ctx| {
            let items = view.prompt_context_menu_items(ctx);
            let len = items.len();
            assert_eq!(len, 3);
            assert_eq!(items[0].fields().unwrap().label(), "Copy prompt");
            assert!(items[1].is_separator());
            assert_eq!(items[2].fields().unwrap().label(), "Edit prompt");
            assert!(!items[2].fields().unwrap().is_disabled());
        });
    })
}

#[test]
fn test_prompt_context_menu_items_for_torch_prompt() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        SessionSettings::handle(&app).update(&mut app, |session_settings, ctx| {
            let _ = session_settings.honor_ps1.set_value(false, ctx);
        });

        terminal.read(&app, |view, ctx| {
            let items = view.prompt_context_menu_items(ctx);
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].fields().unwrap().label(), "Edit prompt");
            assert!(!items[0].fields().unwrap().is_disabled());
        });
    })
}

/// Finishes the running block as the shell does when it's back at its prompt in `pwd`.
fn finish_block_in(model: &mut TerminalModel, pwd: &str) {
    let completion_metadata = CompletionMetadata {
        exit_code: ExitCode::from(0),
        next_block_id: BlockId::new(),
    };
    ansi::Handler::command_finished(
        model,
        CommandFinishedValue {
            completion_metadata: completion_metadata.clone(),
            session_id: None,
        },
    );
    ansi::Handler::precmd_with_completion_metadata(
        model,
        PrecmdValue {
            completion_metadata,
            prompt_metadata: PromptMetadata {
                pwd: Some(pwd.to_owned()),
                session_id: Some(0),
                ..Default::default()
            },
        },
    );
}

#[test]
fn block_prompt_keeps_the_directory_its_command_started_in() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        app.add_singleton_model(remote_server::manager::RemoteServerManager::new);
        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |view, _| {
            let mut model = view.model.lock();
            model.block_list_mut().active_block_mut().start();
            model.simulate_cmd("true");
            finish_block_in(&mut model, "/start");
        });
        // The shell can't run the next command before the view has handled its prompt.
        assert_eventually!(
            terminal.read(&app, |view, _| {
                let model = view.model.lock();
                model
                    .block_list()
                    .active_block()
                    .prompt_snapshot()
                    .is_some()
            }),
            "The view should snapshot the prompt"
        );

        let cd_block_index = terminal.update(&mut app, |view, _| {
            let mut model = view.model.lock();
            let cd_block_index = model.block_list().active_block_index();
            model.block_list_mut().active_block_mut().start();
            model.simulate_cmd("cd /elsewhere");
            ansi::Handler::set_current_working_directory(&mut *model, "/elsewhere".to_owned());
            finish_block_in(&mut model, "/elsewhere");
            cd_block_index
        });

        terminal.read(&app, |view, ctx| {
            let model = view.model.lock();
            assert_eq!(
                TerminalView::block_prompt(&model, view.sessions.as_ref(ctx), cd_block_index),
                "/start"
            );
        });
    })
}

#[test]
fn only_commands_that_outlast_a_quick_command_change_the_pane_title() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        app.add_singleton_model(remote_server::manager::RemoteServerManager::new);
        let terminal = add_window_with_terminal(&mut app, None);
        let pane_title = |app: &App| {
            terminal.read(app, |view, ctx| {
                view.pane_configuration.as_ref(ctx).title().to_owned()
            })
        };

        terminal.update(&mut app, |view, _| {
            view.is_login_shell_bootstrapped = true;
            ansi::Handler::set_title(&mut *view.model.lock(), Some("~".to_owned()));
        });
        assert_eventually!(pane_title(&app) == "~", "The prompt's title should show");

        terminal.update(&mut app, |view, _| {
            let mut model = view.model.lock();
            model.block_list_mut().active_block_mut().start();
            model.simulate_cmd("ls");
            ansi::Handler::set_title(&mut *model, Some("ls ~".to_owned()));
        });
        assert_eventually!(
            terminal.read(&app, |view, _| view.held_terminal_title.is_some()),
            "The view should hold the quick command's title"
        );
        assert_eq!(pane_title(&app), "~");
        terminal.update(&mut app, |view, _| {
            let mut model = view.model.lock();
            finish_block_in(&mut model, "/");
            ansi::Handler::set_title(&mut *model, Some("~".to_owned()));
        });
        assert_eventually!(
            terminal.read(&app, |view, _| view.held_terminal_title.is_none()),
            "The prompt's title should replace the held one"
        );
        assert_eq!(pane_title(&app), "~");

        terminal.update(&mut app, |view, _| {
            let mut model = view.model.lock();
            model.block_list_mut().active_block_mut().start();
            model.simulate_cmd("sleep 5");
            ansi::Handler::set_title(&mut *model, Some("sleep 5 ~".to_owned()));
        });
        assert_eventually!(
            200 => pane_title(&app) == "sleep 5 ~",
            "A command's title should show once it outlasts a quick command"
        );
    })
}

#[test]
fn a_command_shows_as_running_only_once_it_outlasts_a_quick_command() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |view, _| {
            let mut model = view.model.lock();
            model.init_shell(InitShellValue {
                session_id: 0.into(),
                shell: "fish".to_owned(),
                ..Default::default()
            });
            model.bootstrapped(BootstrappedValue {
                shell: "fish".to_owned(),
                ..Default::default()
            });
            // The first block finishes the bootstrap, so only the second one shows in the list.
            model.simulate_block("ls", "file.txt");
            model.simulate_block("pwd", "/");
            model.block_list_mut().active_block_mut().start();
            model.simulate_cmd("sleep 5");
        });
        terminal.read(&app, |view, _| {
            let activity = view.session_activity();
            assert!(!activity.is_running_command);
            assert_eq!(activity.age_minutes, Some(0));
        });
        assert_eventually!(
            200 => terminal.read(&app, |view, _| view.session_activity().is_running_command),
            "A command should show as running once it outlasts a quick command"
        );
    })
}

#[test]
fn test_link_at_range_trims_zero_width_spaces() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        // NOTE: this has two zero-width spaces, one after the '(', and one before the ')'
        let input_url = "(\u{200b}https://foo.test\u{200b})";
        // NOTE: the final character in this string is a zero-width space
        let non_escaped_url = "https://foo.test\u{200b}";
        let escaped_url = "https://foo.test";

        terminal.update(&mut app, |view, _ctx| {
            view.model.lock().simulate_block(
                r"printf '(%bhttps://foo.test%b)\n' '\U200b' '\U200b'",
                input_url,
            );
        });

        terminal.read(&app, |view, ctx| {
            let model = view.model.lock();

            let block = view
                .viewport_state(model.block_list(), InputMode::PinnedToBottom, ctx)
                .iter()
                .next()
                .expect("blocklist should have at least one item");

            let point = WithinModel::BlockList(WithinBlock::new(
                // I picked the point 0, 4 b/c it seemed to work. It's not clear to me
                // why 4 works when numbers like 9 do not. Either way, this is just to
                // get the actual url out (passing 9 fails on url_at_point), and does
                // not matter for testing link_at_range.
                Point::new(0, 4),
                block.block_index.expect("block index should exist"),
                crate::terminal::GridType::Output,
            ));

            let url = model
                .url_at_point(&point)
                .expect("url at the designated point should exist");

            // Assert that string_at_range preserves the ZW Space
            assert_eq!(
                model.string_at_range(&url, RespectObfuscatedSecrets::No),
                non_escaped_url
            );

            // Assert that link_at_range removes the ZW Space
            assert_eq!(
                model.link_at_range(&url, RespectObfuscatedSecrets::No),
                escaped_url
            );
        });
    })
}

#[test]
fn test_scroll_position_doesnt_change_when_block_finished() {
    use futures_lite::StreamExt;

    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        let (tx, rx) = async_channel::bounded(1);
        app.update(|ctx| {
            ctx.subscribe_to_view(&terminal, move |_, event, _| {
                if let Event::BlockCompleted { block, .. } = event {
                    let output = std::str::from_utf8(&block.stylized_output).unwrap();
                    if output.trim() == "lr" {
                        tx.try_send(()).expect("Can send over channel");
                    }
                }
            });
        });

        let scroll_position_before_finished = terminal.update(&mut app, |view, ctx| {
            // Finish a lengthy block.
            view.model.lock().simulate_block("ls", &"\n".repeat(1000));
            assert!(view.is_vertically_scrollable(ctx));
            assert_eq!(
                view.scroll_position(),
                ScrollPosition::FollowsBottomOfMostRecentBlock
            );

            // Start long-running block.
            view.model.lock().simulate_long_running_block("", "lr");

            // Before the block is finished, scroll up.
            view.scroll(1.0.into_lines(), ctx);
            let scroll_position_before_finished = view.scroll_position();
            assert!(matches!(
                scroll_position_before_finished,
                ScrollPosition::FixedAtPosition { .. }
            ));

            // Finish the block.
            view.model.lock().finish_block();

            scroll_position_before_finished
        });

        // Wait until the terminal view acknowledges the block as completed.
        assert!(pin!(rx).next().await.is_some());

        // Make sure the scroll position is unchanged when the block finishes.
        terminal.read(&app, |view, _| {
            let scroll_position_after_finished = view.scroll_position();
            assert_eq!(
                scroll_position_before_finished,
                scroll_position_after_finished
            );
        });
    })
}

/// Sets up a CLI agent session, opens rich input, submits `text`, and returns
/// the terminal handle and the collected PTY writes.
#[allow(clippy::type_complexity)]
fn submit_rich_input_and_collect_pty_writes(
    app: &mut App,
    agent: CLIAgent,
    text: &str,
) -> (ViewHandle<TerminalView>, Rc<RefCell<Vec<Vec<u8>>>>) {
    let terminal = add_window_with_terminal(app, None);
    let pty_writes: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
    let writes = pty_writes.clone();
    app.update(|ctx| {
        ctx.subscribe_to_view(&terminal, move |_, event, _| {
            if let Event::WriteBytesToPty { bytes } = event {
                writes.borrow_mut().push(bytes.to_vec());
            }
        });
    });

    terminal.update(app, |view, ctx| {
        CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
            sessions.set_session(
                view.view_id,
                CLIAgentSession {
                    agent,
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

        view.open_cli_agent_rich_input(CLIAgentInputEntrypoint::FooterButton, ctx);
        assert!(view.has_active_cli_agent_input_session(ctx));

        view.submit_cli_agent_rich_input(text.to_owned(), ctx);
    });

    (terminal, pty_writes)
}

fn open_cli_agent_rich_input_for_agent(app: &mut App, agent: CLIAgent) -> ViewHandle<TerminalView> {
    open_cli_agent_rich_input_for_agent_with_window_id(app, agent).1
}

fn open_cli_agent_rich_input_for_agent_with_window_id(
    app: &mut App,
    agent: CLIAgent,
) -> (WindowId, ViewHandle<TerminalView>) {
    let (window_id, terminal) = add_window_with_id_and_terminal(app, None);
    terminal.update(app, |view, ctx| {
        CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
            sessions.set_session(
                view.view_id,
                CLIAgentSession {
                    agent,
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

        view.open_cli_agent_rich_input(CLIAgentInputEntrypoint::FooterButton, ctx);
        assert!(view.has_active_cli_agent_input_session(ctx));
    });
    (window_id, terminal)
}

#[test]
fn cli_agent_rich_input_hint_text_mentions_active_cli_agent() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _agent_view = FeatureFlag::AgentView.override_enabled(true);
        let _cli_rich = FeatureFlag::CLIAgentRichInput.override_enabled(true);

        for (agent, expected_hint_text) in [
            (CLIAgent::Claude, "Enter prompt for Claude Code..."),
            (CLIAgent::Gemini, "Enter prompt for Gemini..."),
            (CLIAgent::Codex, "Enter prompt for Codex..."),
            (CLIAgent::Unknown, "Tell the agent what to build..."),
        ] {
            let terminal = open_cli_agent_rich_input_for_agent(&mut app, agent);
            terminal.read(&app, |view, ctx| {
                let placeholder_text = view
                    .input
                    .as_ref(ctx)
                    .editor()
                    .as_ref(ctx)
                    .placeholder_text("");
                assert_eq!(placeholder_text, Some(expected_hint_text));
            });
        }
    })
}

#[test]
fn cli_agent_rich_input_shell_mode_uses_run_commands_hint_text() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _agent_view = FeatureFlag::AgentView.override_enabled(true);
        let _cli_rich = FeatureFlag::CLIAgentRichInput.override_enabled(true);

        let terminal = open_cli_agent_rich_input_for_agent(&mut app, CLIAgent::Claude);
        terminal.update(&mut app, |view, ctx| {
            view.input.update(ctx, |input, ctx| {
                input.ai_input_model().update(ctx, |ai_input, ctx| {
                    ai_input.set_input_config(
                        InputConfig {
                            input_type: InputType::Shell,
                            is_locked: true,
                        },
                        true,
                        None,
                        ctx,
                    );
                });
                input.set_zero_state_hint_text(ctx);
            });
        });
        terminal.read(&app, |view, ctx| {
            let placeholder_text = view
                .input
                .as_ref(ctx)
                .editor()
                .as_ref(ctx)
                .placeholder_text("");
            assert_eq!(placeholder_text, Some("Run commands"));
        });
    })
}

#[test]
fn submit_cli_agent_rich_input_codex_uses_bracketed_paste() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _agent_view = FeatureFlag::AgentView.override_enabled(true);
        let _cli_rich = FeatureFlag::CLIAgentRichInput.override_enabled(true);

        let (_terminal, pty_writes) =
            submit_rich_input_and_collect_pty_writes(&mut app, CLIAgent::Codex, "hello");

        let writes = pty_writes.borrow();
        // BracketedPaste: first write is ESC[200~ + text + ESC[201~, second is \r.
        assert_eq!(
            writes.len(),
            2,
            "expected 2 PTY writes, got {}",
            writes.len()
        );

        let mut expected_paste =
            Vec::with_capacity(BRACKETED_PASTE_START.len() + 5 + BRACKETED_PASTE_END.len());
        expected_paste.extend_from_slice(BRACKETED_PASTE_START);
        expected_paste.extend_from_slice(b"hello");
        expected_paste.extend_from_slice(BRACKETED_PASTE_END);
        assert_eq!(writes[0], expected_paste);
        assert_eq!(writes[1], b"\r");
    })
}

/// Verifies that multi-line Hermes rich input is delivered as a single bracketed
/// paste payload with a standalone \r submit. Embedded newlines must remain
/// inside the paste instead of triggering separate submissions.
#[test]
fn submit_cli_agent_rich_input_hermes_multiline_uses_bracketed_paste() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _agent_view = FeatureFlag::AgentView.override_enabled(true);
        let _cli_rich = FeatureFlag::CLIAgentRichInput.override_enabled(true);

        let (_terminal, pty_writes) =
            submit_rich_input_and_collect_pty_writes(&mut app, CLIAgent::Hermes, "line1\nline2");

        let writes = pty_writes.borrow();
        // BracketedPaste: first write is ESC[200~ + both lines + ESC[201~, second is \r.
        // The embedded \n between lines must NOT split into a separate write or trigger
        // a second submission — that was the voice-input auto-submit regression.
        assert_eq!(
            writes.len(),
            2,
            "expected 2 PTY writes (paste payload + submit \r), got {}: {:?}",
            writes.len(),
            writes
        );

        let mut expected_paste =
            Vec::with_capacity(BRACKETED_PASTE_START.len() + 11 + BRACKETED_PASTE_END.len());
        expected_paste.extend_from_slice(BRACKETED_PASTE_START);
        expected_paste.extend_from_slice(b"line1\nline2");
        expected_paste.extend_from_slice(BRACKETED_PASTE_END);
        assert_eq!(
            writes[0], expected_paste,
            "first write should be the full bracketed paste payload"
        );
        assert_eq!(
            writes[1], b"\r",
            "second write should be the standalone submit \r"
        );
    })
}

#[test]
fn submit_cli_agent_rich_input_opencode_defers_enter_and_close() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _agent_view = FeatureFlag::AgentView.override_enabled(true);
        let _cli_rich = FeatureFlag::CLIAgentRichInput.override_enabled(true);

        let (_terminal, pty_writes) =
            submit_rich_input_and_collect_pty_writes(&mut app, CLIAgent::OpenCode, "hello");

        // Immediately after submit, only the text should have been written;
        // the \r is sent after a short delay.
        assert_eq!(pty_writes.borrow().len(), 1);
        assert_eq!(pty_writes.borrow()[0], b"hello");

        // Wait for the delayed \r to arrive.
        assert_eventually!(
            100 => pty_writes.borrow().len() == 2,
            "carriage return should be written after delay"
        );
        assert_eq!(pty_writes.borrow()[1], b"\r");
    })
}

#[test]
fn submit_cli_agent_rich_input_muse_uses_bracketed_paste() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _agent_view = FeatureFlag::AgentView.override_enabled(true);
        let _cli_rich = FeatureFlag::CLIAgentRichInput.override_enabled(true);

        let (_terminal, pty_writes) =
            submit_rich_input_and_collect_pty_writes(&mut app, CLIAgent::Muse, "hello");

        let writes = pty_writes.borrow();
        assert_eq!(
            writes.len(),
            2,
            "expected 2 PTY writes, got {}",
            writes.len()
        );

        let mut expected_paste =
            Vec::with_capacity(BRACKETED_PASTE_START.len() + 5 + BRACKETED_PASTE_END.len());
        expected_paste.extend_from_slice(BRACKETED_PASTE_START);
        expected_paste.extend_from_slice(b"hello");
        expected_paste.extend_from_slice(BRACKETED_PASTE_END);
        assert_eq!(writes[0], expected_paste);
        assert_eq!(writes[1], b"\r");
    })
}

#[test]
fn attach_path_as_context_routes_to_open_cli_agent_rich_input() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _agent_view = FeatureFlag::AgentView.override_enabled(true);
        let _cli_rich = FeatureFlag::CLIAgentRichInput.override_enabled(true);
        let _hoa_code_review = FeatureFlag::HoaCodeReview.override_enabled(true);

        let terminal = open_cli_agent_rich_input_for_agent(&mut app, CLIAgent::Claude);
        let pty_writes: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
        let writes = pty_writes.clone();
        app.update(|ctx| {
            ctx.subscribe_to_view(&terminal, move |_, event, _| {
                if let Event::WriteBytesToPty { bytes } = event {
                    writes.borrow_mut().push(bytes.to_vec());
                }
            });
        });

        terminal.update(&mut app, |view, ctx| {
            view.attach_path_as_context(std::path::Path::new("src/main.rs"), ctx);
        });

        terminal.read(&app, |view, ctx| {
            assert_eq!(view.input.as_ref(ctx).buffer_text(ctx), "src/main.rs");
        });
        assert!(
            pty_writes.borrow().is_empty(),
            "context should be inserted into rich input instead of written to PTY"
        );
    })
}
#[test]
fn drag_drop_image_in_cli_agent_long_running_command_pastes_via_clipboard() {
    // Regression test: dropping an image file into a tab where a CLI agent
    // (e.g. Claude Code) is the foreground long-running process should
    // mirror the Cmd+V image-paste path — write the image to the system
    // clipboard and send the agent's paste keystroke to the PTY — instead
    // of shell-escaping the path and typing it into the agent's prompt.
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _agent_view = FeatureFlag::AgentView.override_enabled(true);

        // The new path actually reads the file off disk, so we need a real
        // file. Bytes don't have to be a valid PNG.
        let mut image_path = std::env::temp_dir();
        image_path.push(format!(
            "warp-test-cli-agent-drop-{}.png",
            std::process::id()
        ));
        std::fs::write(&image_path, b"fake-png-bytes").expect("write tmp image");
        let image_path_str = image_path.to_string_lossy().into_owned();

        let terminal = add_window_with_terminal(&mut app, None);

        let pty_writes: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
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
                        agent: CLIAgent::Claude,
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

            // The CLI-agent paste branch is gated on the active block being
            // long-running (the agent's TUI). Without a long-running block
            // we'd fall through to the regular image-attach flow.
            {
                let mut model = view.model.lock();
                model.simulate_long_running_block("claude", "");
                assert!(
                    model
                        .block_list()
                        .active_block()
                        .is_active_and_long_running()
                );
            }

            view.drag_and_drop_files(&[image_path_str], ctx);
        });

        // The paste flow is async (off-thread file read, then hop back to
        // the view to write the clipboard + paste keystroke). Wait for the
        // single PTY write of the platform-appropriate paste byte: 0x16
        // (Ctrl+V) on macOS/Linux, or `ESC v` on Windows. Without the fix
        // a shell-escaped path string is written here instead.
        let expected_paste_bytes: Vec<u8> = if cfg!(windows) {
            vec![0x1b, b'v']
        } else {
            vec![0x16]
        };
        assert_eventually!(
            pty_writes.borrow().len() == 1 && pty_writes.borrow()[0] == expected_paste_bytes,
            "expected single paste-keystroke PTY write {:?}; got {:?}",
            expected_paste_bytes,
            pty_writes.borrow()
        );

        std::fs::remove_file(&image_path).ok();
    })
}

#[test]
fn paste_raw_image_clipboard_in_cli_agent_sends_correct_bytes() {
    fn run_for_agent(agent: CLIAgent) {
        App::test((), move |mut app| async move {
            initialize_app_for_terminal_view(&mut app);
            let _agent_view = FeatureFlag::AgentView.override_enabled(true);

            let terminal = add_window_with_terminal(&mut app, None);

            let pty_writes: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
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
                            agent,
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

                {
                    let mut model = view.model.lock();
                    model.simulate_long_running_block(agent.command_prefix(), "");
                    model.set_mode(ansi::Mode::BracketedPaste);
                }

                // Write image-only data to the clipboard (no text, no paths).
                ctx.clipboard().write(ClipboardContent {
                    images: Some(vec![torchui::clipboard::ImageData {
                        data: vec![0x89, 0x50, 0x4E, 0x47], // PNG magic bytes
                        mime_type: "image/png".to_string(),
                        filename: None,
                    }]),
                    ..Default::default()
                });

                view.handle_action(&TerminalAction::Paste, ctx);
            });

            let writes = pty_writes.borrow();
            assert_eq!(
                writes.len(),
                1,
                "expected 1 PTY write, got {}",
                writes.len()
            );

            if cfg!(windows) {
                if agent == CLIAgent::Claude {
                    assert_eq!(writes[0], vec![C0::ESC, b'v']);
                } else {
                    let mut expected = Vec::new();
                    expected.extend_from_slice(BRACKETED_PASTE_START);
                    expected.extend_from_slice(BRACKETED_PASTE_END);
                    assert_eq!(writes[0], expected);
                }
            } else {
                assert_eq!(writes[0], vec![C0::SYN]);
            }
        })
    }

    run_for_agent(CLIAgent::Claude);
    run_for_agent(CLIAgent::OpenCode);
    run_for_agent(CLIAgent::Codex);
}

#[test]
fn submit_without_auto_dismiss_keeps_rich_input_open() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _agent_view = FeatureFlag::AgentView.override_enabled(true);
        let _cli_rich = FeatureFlag::CLIAgentRichInput.override_enabled(true);
        // auto_dismiss defaults to false — leave it off.

        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |view, ctx| {
            CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
                sessions.set_session(
                    view.view_id,
                    CLIAgentSession {
                        agent: CLIAgent::Claude,
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

            view.open_cli_agent_rich_input(CLIAgentInputEntrypoint::FooterButton, ctx);
            assert!(view.has_active_cli_agent_input_session(ctx));

            view.submit_cli_agent_rich_input("hello".to_owned(), ctx);

            // Rich input stays open because auto_dismiss is off.
            assert!(view.has_active_cli_agent_input_session(ctx));
        });

        // Buffer should still be cleared even though rich input is open.
        terminal.read(&app, |view, ctx| {
            let input = view.input.as_ref(ctx);
            assert!(input.editor().as_ref(ctx).buffer_text(ctx).is_empty());
        });
    })
}

#[test]
fn submit_with_plugin_and_auto_toggle_keeps_rich_input_open() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _agent_view = FeatureFlag::AgentView.override_enabled(true);
        let _cli_rich = FeatureFlag::CLIAgentRichInput.override_enabled(true);
        // auto_toggle_rich_input defaults to true.
        // Turn on auto_dismiss too — it should be overridden by auto_toggle.
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            let _ = settings
                .auto_dismiss_rich_input_after_submit
                .set_value(true, ctx);
        });

        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |view, ctx| {
            // Create a session with a plugin listener and should_auto_toggle_input.
            let listener = ctx.add_model(|ctx| {
                CLIAgentSessionListener::new(
                    view.view_id,
                    CLIAgent::Claude,
                    &view.model_events_handle,
                    ctx,
                )
            });
            CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
                sessions.set_session(
                    view.view_id,
                    CLIAgentSession {
                        agent: CLIAgent::Claude,
                        status: CLIAgentSessionStatus::InProgress,
                        session_context: CLIAgentSessionContext::default(),
                        input_state: CLIAgentInputState::Closed,
                        should_auto_toggle_input: true,
                        listener: Some(listener),
                        remote_host: None,
                        plugin_version: Some("1.0.0".to_owned()),
                        draft_text: None,
                        custom_command_prefix: None,
                        received_rich_notification: true,
                    },
                    ctx,
                );
            });

            view.open_cli_agent_rich_input(CLIAgentInputEntrypoint::FooterButton, ctx);
            assert!(view.has_active_cli_agent_input_session(ctx));

            view.submit_cli_agent_rich_input("hello".to_owned(), ctx);

            // Rich input stays open because auto_toggle + plugin takes precedence.
            assert!(view.has_active_cli_agent_input_session(ctx));
        });
    })
}

#[test]
fn submit_with_plugin_but_auto_toggle_off_respects_auto_dismiss() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _agent_view = FeatureFlag::AgentView.override_enabled(true);
        let _cli_rich = FeatureFlag::CLIAgentRichInput.override_enabled(true);
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            let _ = settings.auto_toggle_rich_input.set_value(false, ctx);
            let _ = settings
                .auto_dismiss_rich_input_after_submit
                .set_value(true, ctx);
        });

        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |view, ctx| {
            let listener = ctx.add_model(|ctx| {
                CLIAgentSessionListener::new(
                    view.view_id,
                    CLIAgent::Claude,
                    &view.model_events_handle,
                    ctx,
                )
            });
            CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
                sessions.set_session(
                    view.view_id,
                    CLIAgentSession {
                        agent: CLIAgent::Claude,
                        status: CLIAgentSessionStatus::InProgress,
                        session_context: CLIAgentSessionContext::default(),
                        input_state: CLIAgentInputState::Closed,
                        should_auto_toggle_input: true,
                        listener: Some(listener),
                        remote_host: None,
                        plugin_version: Some("1.0.0".to_owned()),
                        draft_text: None,
                        custom_command_prefix: None,
                        received_rich_notification: false,
                    },
                    ctx,
                );
            });

            view.open_cli_agent_rich_input(CLIAgentInputEntrypoint::FooterButton, ctx);
            assert!(view.has_active_cli_agent_input_session(ctx));

            view.submit_cli_agent_rich_input("hello".to_owned(), ctx);
        });

        // auto_toggle is off, so auto_dismiss closes rich input.
        // Claude uses DelayedEnter, so the close happens after a timer.
        assert_eventually!(
            100 => terminal.read(&app, |view, ctx| !view
                .has_active_cli_agent_input_session(ctx)),
            "Rich input should be closed after submit with auto_dismiss"
        );
    })
}

#[test]
fn status_blocked_auto_closes_rich_input() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _agent_view = FeatureFlag::AgentView.override_enabled(true);
        let _cli_rich = FeatureFlag::CLIAgentRichInput.override_enabled(true);
        // auto_toggle_rich_input defaults to true.

        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |view, ctx| {
            let listener = ctx.add_model(|ctx| {
                CLIAgentSessionListener::new(
                    view.view_id,
                    CLIAgent::Claude,
                    &view.model_events_handle,
                    ctx,
                )
            });
            CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
                sessions.set_session(
                    view.view_id,
                    CLIAgentSession {
                        agent: CLIAgent::Claude,
                        status: CLIAgentSessionStatus::InProgress,
                        session_context: CLIAgentSessionContext::default(),
                        input_state: CLIAgentInputState::Closed,
                        should_auto_toggle_input: true,
                        listener: Some(listener),
                        remote_host: None,
                        plugin_version: Some("1.0.0".to_owned()),
                        draft_text: None,
                        custom_command_prefix: None,
                        received_rich_notification: false,
                    },
                    ctx,
                );
            });

            view.open_cli_agent_rich_input(CLIAgentInputEntrypoint::FooterButton, ctx);
            assert!(view.has_active_cli_agent_input_session(ctx));

            // Simulate a PermissionRequest event → status transitions to Blocked.
            CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
                sessions.update_from_event(
                    view.view_id,
                    &CLIAgentEvent {
                        source: CLIAgentEventSource::RichPlugin,
                        v: 1,
                        agent: CLIAgent::Claude,
                        event: CLIAgentEventType::PermissionRequest,
                        session_id: None,
                        cwd: None,
                        project: None,
                        payload: CLIAgentEventPayload {
                            summary: Some("Approve?".to_owned()),
                            ..Default::default()
                        },
                    },
                    ctx,
                );
            });
        });

        // The StatusChanged event is delivered to the terminal view, which
        // auto-closes rich input because the agent is blocked.
        terminal.read(&app, |view, ctx| {
            assert!(!view.has_active_cli_agent_input_session(ctx));
        });

        // should_auto_toggle_input is preserved so auto-open can fire later.
        terminal.read(&app, |_view, ctx| {
            let session = CLIAgentSessionsModel::as_ref(ctx).session(_view.view_id);
            assert!(session.unwrap().should_auto_toggle_input);
        });
    })
}

#[test]
fn status_in_progress_auto_opens_rich_input_after_blocked() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _agent_view = FeatureFlag::AgentView.override_enabled(true);
        let _cli_rich = FeatureFlag::CLIAgentRichInput.override_enabled(true);

        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |view, ctx| {
            let listener = ctx.add_model(|ctx| {
                CLIAgentSessionListener::new(
                    view.view_id,
                    CLIAgent::Claude,
                    &view.model_events_handle,
                    ctx,
                )
            });
            CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
                sessions.set_session(
                    view.view_id,
                    CLIAgentSession {
                        agent: CLIAgent::Claude,
                        status: CLIAgentSessionStatus::InProgress,
                        session_context: CLIAgentSessionContext::default(),
                        input_state: CLIAgentInputState::Closed,
                        should_auto_toggle_input: true,
                        listener: Some(listener),
                        remote_host: None,
                        plugin_version: Some("1.0.0".to_owned()),
                        draft_text: None,
                        custom_command_prefix: None,
                        received_rich_notification: false,
                    },
                    ctx,
                );
            });

            // Open rich input, then simulate blocked → closed automatically.
            view.open_cli_agent_rich_input(CLIAgentInputEntrypoint::FooterButton, ctx);
            CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
                sessions.update_from_event(
                    view.view_id,
                    &CLIAgentEvent {
                        source: CLIAgentEventSource::RichPlugin,
                        v: 1,
                        agent: CLIAgent::Claude,
                        event: CLIAgentEventType::PermissionRequest,
                        session_id: None,
                        cwd: None,
                        project: None,
                        payload: CLIAgentEventPayload {
                            summary: Some("Approve?".to_owned()),
                            ..Default::default()
                        },
                    },
                    ctx,
                );
            });
        });

        // Rich input should be auto-closed from the blocked status.
        terminal.read(&app, |view, ctx| {
            assert!(!view.has_active_cli_agent_input_session(ctx));
        });

        // Simulate permission replied → status transitions back to InProgress.
        terminal.update(&mut app, |view, ctx| {
            CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
                sessions.update_from_event(
                    view.view_id,
                    &CLIAgentEvent {
                        source: CLIAgentEventSource::RichPlugin,
                        v: 1,
                        agent: CLIAgent::Claude,
                        event: CLIAgentEventType::PermissionReplied,
                        session_id: None,
                        cwd: None,
                        project: None,
                        payload: CLIAgentEventPayload::default(),
                    },
                    ctx,
                );
            });
        });

        // Rich input should auto-open because should_auto_toggle_input was preserved.
        terminal.read(&app, |view, ctx| {
            assert!(view.has_active_cli_agent_input_session(ctx));
        });
    })
}

// Regression test.
// Codex's listener doesn't emit Blocked-state events (it only forwards opaque
// OSC 9 notifications as Stop), so auto-toggling rich input would trap arrow
// keys when Codex shows interactive option menus. Auto-toggle must not fire
// for agents whose handlers report `supports_rich_status() == false`.
#[test]
fn codex_status_change_does_not_auto_open_rich_input() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _agent_view = FeatureFlag::AgentView.override_enabled(true);
        let _cli_rich = FeatureFlag::CLIAgentRichInput.override_enabled(true);
        // auto_toggle_rich_input defaults to true.

        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |view, ctx| {
            let listener = ctx.add_model(|ctx| {
                CLIAgentSessionListener::new(
                    view.view_id,
                    CLIAgent::Codex,
                    &view.model_events_handle,
                    ctx,
                )
            });
            CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
                sessions.set_session(
                    view.view_id,
                    CLIAgentSession {
                        agent: CLIAgent::Codex,
                        status: CLIAgentSessionStatus::InProgress,
                        session_context: CLIAgentSessionContext::default(),
                        input_state: CLIAgentInputState::Closed,
                        should_auto_toggle_input: true,
                        listener: Some(listener),
                        remote_host: None,
                        plugin_version: None,
                        draft_text: None,
                        custom_command_prefix: None,
                        received_rich_notification: false,
                    },
                    ctx,
                );
            });

            // Rich input starts closed. Simulating a Stop event (the only
            // status Codex's handler ever emits) must not re-open it,
            // because the user may be navigating Codex's option menus.
            assert!(!view.has_active_cli_agent_input_session(ctx));
            CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
                sessions.update_from_event(
                    view.view_id,
                    &CLIAgentEvent {
                        source: CLIAgentEventSource::CodexOsc9Fallback,
                        v: 1,
                        agent: CLIAgent::Codex,
                        event: CLIAgentEventType::Stop,
                        session_id: None,
                        cwd: None,
                        project: None,
                        payload: CLIAgentEventPayload {
                            query: Some("Agent turn complete".to_owned()),
                            ..Default::default()
                        },
                    },
                    ctx,
                );
            });
        });

        terminal.read(&app, |view, ctx| {
            assert!(!view.has_active_cli_agent_input_session(ctx));
        });
    })
}

#[test]
fn manual_dismiss_disables_auto_toggle_for_session() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _agent_view = FeatureFlag::AgentView.override_enabled(true);
        let _cli_rich = FeatureFlag::CLIAgentRichInput.override_enabled(true);

        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |view, ctx| {
            let listener = ctx.add_model(|ctx| {
                CLIAgentSessionListener::new(
                    view.view_id,
                    CLIAgent::Claude,
                    &view.model_events_handle,
                    ctx,
                )
            });
            CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
                sessions.set_session(
                    view.view_id,
                    CLIAgentSession {
                        agent: CLIAgent::Claude,
                        status: CLIAgentSessionStatus::InProgress,
                        session_context: CLIAgentSessionContext::default(),
                        input_state: CLIAgentInputState::Closed,
                        should_auto_toggle_input: true,
                        listener: Some(listener),
                        remote_host: None,
                        plugin_version: Some("1.0.0".to_owned()),
                        draft_text: None,
                        custom_command_prefix: None,
                        received_rich_notification: false,
                    },
                    ctx,
                );
            });

            view.open_cli_agent_rich_input(CLIAgentInputEntrypoint::FooterButton, ctx);
            assert!(view.has_active_cli_agent_input_session(ctx));

            // Manual dismiss via the "disable auto-toggle" path (Escape / Ctrl-G / footer).
            view.close_cli_agent_rich_input_and_disable_auto_toggle(ctx);
            assert!(!view.has_active_cli_agent_input_session(ctx));
        });

        // should_auto_toggle_input should now be false.
        terminal.read(&app, |view, ctx| {
            let session = CLIAgentSessionsModel::as_ref(ctx).session(view.view_id);
            assert!(!session.unwrap().should_auto_toggle_input);
        });

        // A status change to InProgress should NOT auto-open rich input.
        terminal.update(&mut app, |view, ctx| {
            // First move to Blocked so we can transition back.
            CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
                sessions.update_from_event(
                    view.view_id,
                    &CLIAgentEvent {
                        source: CLIAgentEventSource::RichPlugin,
                        v: 1,
                        agent: CLIAgent::Claude,
                        event: CLIAgentEventType::PermissionRequest,
                        session_id: None,
                        cwd: None,
                        project: None,
                        payload: CLIAgentEventPayload {
                            summary: Some("Approve?".to_owned()),
                            ..Default::default()
                        },
                    },
                    ctx,
                );
            });
            CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
                sessions.update_from_event(
                    view.view_id,
                    &CLIAgentEvent {
                        source: CLIAgentEventSource::RichPlugin,
                        v: 1,
                        agent: CLIAgent::Claude,
                        event: CLIAgentEventType::PermissionReplied,
                        session_id: None,
                        cwd: None,
                        project: None,
                        payload: CLIAgentEventPayload::default(),
                    },
                    ctx,
                );
            });
        });

        // Rich input should remain closed.
        terminal.read(&app, |view, ctx| {
            assert!(!view.has_active_cli_agent_input_session(ctx));
        });
    })
}

#[test]
fn close_cli_agent_rich_input_saves_draft_and_reopen_restores_it() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _agent_view = FeatureFlag::AgentView.override_enabled(true);
        let _cli_rich = FeatureFlag::CLIAgentRichInput.override_enabled(true);

        let terminal = open_cli_agent_rich_input_for_agent(&mut app, CLIAgent::Claude);

        // Type some text into the composer.
        terminal.update(&mut app, |view, ctx| {
            view.input.update(ctx, |input, ctx| {
                input.replace_buffer_content("work in progress", ctx);
            });
        });

        // Close the composer — the buffer text should be saved as a draft.
        terminal.update(&mut app, |view, ctx| {
            view.close_cli_agent_rich_input(CLIAgentRichInputCloseReason::Manual, ctx);
            assert!(!view.has_active_cli_agent_input_session(ctx));
        });

        terminal.read(&app, |view, ctx| {
            let session = CLIAgentSessionsModel::as_ref(ctx)
                .session(view.view_id)
                .expect("session should exist");
            assert_eq!(
                session.draft_text.as_deref(),
                Some("work in progress"),
                "draft should be saved on close"
            );
        });

        // Reopen — draft should be restored into the buffer and consumed.
        terminal.update(&mut app, |view, ctx| {
            view.open_cli_agent_rich_input(CLIAgentInputEntrypoint::FooterButton, ctx);
            assert!(view.has_active_cli_agent_input_session(ctx));
        });

        terminal.read(&app, |view, ctx| {
            assert_eq!(
                view.input.as_ref(ctx).buffer_text(ctx),
                "work in progress",
                "draft should be restored on reopen"
            );
            let session = CLIAgentSessionsModel::as_ref(ctx)
                .session(view.view_id)
                .expect("session should exist");
            assert_eq!(
                session.draft_text, None,
                "draft should be consumed after restore"
            );
        });
    })
}

#[test]
fn submit_cli_agent_rich_input_clears_draft() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _agent_view = FeatureFlag::AgentView.override_enabled(true);
        let _cli_rich = FeatureFlag::CLIAgentRichInput.override_enabled(true);
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            // Keep the input open after submit so we can inspect the buffer.
            let _ = settings
                .auto_dismiss_rich_input_after_submit
                .set_value(false, ctx);
        });

        let terminal = open_cli_agent_rich_input_for_agent(&mut app, CLIAgent::Claude);

        terminal.update(&mut app, |view, ctx| {
            view.submit_cli_agent_rich_input("hello agent".to_owned(), ctx);
            // Input stays open because auto-dismiss is off.
            assert!(view.has_active_cli_agent_input_session(ctx));
        });

        terminal.read(&app, |view, ctx| {
            let session = CLIAgentSessionsModel::as_ref(ctx)
                .session(view.view_id)
                .expect("session should exist");
            assert_eq!(
                session.draft_text, None,
                "draft should be cleared after submit"
            );
            assert!(
                view.input.as_ref(ctx).buffer_text(ctx).is_empty(),
                "buffer should be empty after submit"
            );
        });
    })
}

#[test]
fn close_cli_agent_rich_input_with_empty_buffer_stores_no_draft() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _agent_view = FeatureFlag::AgentView.override_enabled(true);
        let _cli_rich = FeatureFlag::CLIAgentRichInput.override_enabled(true);

        let terminal = open_cli_agent_rich_input_for_agent(&mut app, CLIAgent::Claude);

        // Close immediately without typing anything.
        terminal.update(&mut app, |view, ctx| {
            view.close_cli_agent_rich_input(CLIAgentRichInputCloseReason::Manual, ctx);
            assert!(!view.has_active_cli_agent_input_session(ctx));
        });

        terminal.read(&app, |view, ctx| {
            let session = CLIAgentSessionsModel::as_ref(ctx)
                .session(view.view_id)
                .expect("session should exist");
            assert_eq!(
                session.draft_text, None,
                "no draft should be stored for empty buffer"
            );
        });
    })
}

/// Regression test for the async-find branch of #11212.
///
/// Closing the find bar must clear stale AI block highlights without dropping
/// the saved query options on the async-find path. `open_find_bar` reads
/// `active_find_options` to restore the previous query; if `close_find_bar`
/// routes through `clear_matches → AsyncFindController::clear_results`, that
/// helper resets `current_find_options` and reopening the find bar starts
/// from a blank query instead of the previous one.
#[test]
fn close_find_bar_preserves_options_on_async_find_path() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _async_find = FeatureFlag::AsyncFind.override_enabled(true);
        let terminal = add_window_with_terminal(&mut app, None);

        let needle_options = || FindOptions {
            query: Some("needle".to_owned().into()),
            ..Default::default()
        };

        terminal.update(&mut app, |view, ctx| {
            view.show_find_bar(ctx);
            view.run_find(needle_options(), ctx);
        });

        // The async controller should have saved the active query.
        assert_eq!(
            terminal.read(&app, |view, ctx| view
                .find_model
                .as_ref(ctx)
                .active_find_options()
                .map(|o| o.query.clone())),
            Some(needle_options().query),
            "running find on the async path must save the active query"
        );

        // Closing the find bar must NOT drop the saved query — otherwise
        // the next `open_find_bar` would start blank instead of restoring
        // the previous search.
        terminal.update(&mut app, |view, ctx| {
            view.close_find_bar(ctx);
        });

        assert_eq!(
            terminal.read(&app, |view, ctx| view
                .find_model
                .as_ref(ctx)
                .active_find_options()
                .map(|o| o.query.clone())),
            Some(needle_options().query),
            "closing the find bar must preserve the saved query on the async path"
        );
    })
}

#[test]
#[cfg(target_os = "linux")]
fn copy_forwards_etx_to_pty_on_linux_alt_screen_without_warp_selection() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        let pty_writes: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
        let writes = pty_writes.clone();
        app.update(|ctx| {
            ctx.subscribe_to_view(&terminal, move |_, event, _| {
                if let Event::WriteBytesToPty { bytes } = event {
                    writes.borrow_mut().push(bytes.to_vec());
                }
            });
        });

        terminal.update(&mut app, |view, ctx| {
            // Enter the alt screen (a fullscreen TUI is in control) and add no
            // Torch-visible selection of any kind.
            {
                let mut model = view.model.lock();
                model.set_mode(ansi::Mode::SwapScreen {
                    save_cursor_and_clear_screen: true,
                });
                assert!(model.is_alt_screen_active());
            }
            assert_eq!("", &read_from_clipboard(ctx));

            // No CLI-subagent / error-screen / grid / input-editor / block
            // selection exists, so `copy()` reaches the new fallback. The clipboard
            // is written synchronously, but `WriteBytesToPty` events are dispatched
            // after the update closure returns, so the PTY-write assertion is made
            // outside the closure (mirroring `ctrl_c_after_stop_takeover_cancels_conversation`).
            view.handle_action(&TerminalAction::Copy, ctx);

            assert_eq!(
                read_from_clipboard(ctx),
                "",
                "Copy must not write anything to the clipboard when Torch has no selection"
            );
        });

        assert_eq!(
            *pty_writes.borrow(),
            vec![vec![C0::ETX]],
            "Copy on Linux alt screen with no Torch selection must forward exactly one ETX byte to the PTY"
        );
    })
}

#[test]
fn copy_does_not_forward_when_alt_screen_has_warp_selection() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        let pty_writes: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
        let writes = pty_writes.clone();
        app.update(|ctx| {
            ctx.subscribe_to_view(&terminal, move |_, event, _| {
                if let Event::WriteBytesToPty { bytes } = event {
                    writes.borrow_mut().push(bytes.to_vec());
                }
            });
        });

        terminal.update(&mut app, |view, ctx| {
            {
                // Enter the alt screen and add text the user will select in Torch.
                let mut model = view.model.lock();
                model.set_mode(ansi::Mode::SwapScreen {
                    save_cursor_and_clear_screen: true,
                });
                assert!(model.is_alt_screen_active());

                model.alt_screen_mut().input('h');
            }

            // Make a Torch-owned alt-screen selection (the path that copies today).
            view.begin_alt_selection(Point::new(0, 0), Side::Left, SelectionType::Simple, ctx);
            view.update_alt_selection(Point::new(0, 2), Side::Left, &Lines::zero(), ctx);
            view.end_alt_selection(ctx);
            // `end_alt_selection` copies via copy-on-select, so the clipboard now
            // holds the selected text. Reset the PTY-write recorder so the only
            // writes observed below come from the explicit Copy dispatch.
            pty_writes.borrow_mut().clear();
            assert_eq!("h", &read_from_clipboard(ctx));

            view.handle_action(&TerminalAction::Copy, ctx);

            assert_eq!(
                read_from_clipboard(ctx),
                "h",
                "Copy must still copy the Torch alt-screen selection to the clipboard"
            );
        });

        assert!(
            pty_writes.borrow().is_empty(),
            "Copy must not forward ETX to the PTY when a Torch alt-screen selection was copied"
        );
    })
}

#[test]
fn copy_does_not_forward_on_normal_screen() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let terminal = add_window_with_terminal(&mut app, None);
        let pty_writes: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
        let writes = pty_writes.clone();
        app.update(|ctx| {
            ctx.subscribe_to_view(&terminal, move |_, event, _| {
                if let Event::WriteBytesToPty { bytes } = event {
                    writes.borrow_mut().push(bytes.to_vec());
                }
            });
        });

        terminal.update(&mut app, |view, ctx| {
            // Normal screen: no alt screen, no selection of any kind.
            assert!(!view.model.lock().is_alt_screen_active());
            assert_eq!("", &read_from_clipboard(ctx));

            view.handle_action(&TerminalAction::Copy, ctx);

            assert_eq!(
                read_from_clipboard(ctx),
                "",
                "Copy must not write anything to the clipboard on the normal screen with no selection"
            );
        });

        assert!(
            pty_writes.borrow().is_empty(),
            "Copy must not write anything to the PTY on the normal screen with no selection"
        );
    })
}

fn set_warp_tui_session(view: &mut TerminalView, ctx: &mut ViewContext<TerminalView>) {
    view.model.lock().simulate_long_running_block("warp", "");
    assert_eq!(
        CLIAgent::detect("warp", None, None, ctx),
        Some(CLIAgent::WarpTui)
    );

    CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, ctx| {
        sessions.set_session(
            view.view_id,
            CLIAgentSession {
                agent: CLIAgent::WarpTui,
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
}

#[test]
fn warp_tui_listener_does_not_auto_open_rich_input() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            let _ = settings
                .auto_open_rich_input_on_cli_agent_start
                .set_value(true, ctx);
        });

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            view.handle_cli_agent_notification(
                Some(CLI_AGENT_NOTIFICATION_SENTINEL),
                r#"{"v":1,"agent":"warp-tui","event":"session_start"}"#,
                ctx,
            );
        });

        terminal.read(&app, |view, ctx| {
            let session = CLIAgentSessionsModel::as_ref(ctx)
                .session(view.view_id)
                .expect("Torch TUI session should be registered");
            assert!(session.listener.is_some());
            assert!(!session.should_auto_toggle_input);
            assert!(!view.has_active_cli_agent_input_session(ctx));
        });
    });
}

/// `active_cli_agent` must return `None` for the Torch TUI when `HoaCodeReview`
/// is disabled, preserving the pre-feature behavior (no review destination).
#[test]
fn active_cli_agent_ignores_warp_tui_when_hoa_code_review_disabled() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _hoa_code_review = FeatureFlag::HoaCodeReview.override_enabled(false);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, ctx| {
            set_warp_tui_session(view, ctx);
        });

        terminal.read(&app, |view, ctx| {
            assert_eq!(
                view.active_cli_agent(ctx),
                None,
                "Torch TUI should not be a review destination when HoaCodeReview is disabled"
            );
        });
    });
}

#[test]
fn active_cli_agent_ignores_non_tui_long_running_command() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let _hoa_code_review = FeatureFlag::HoaCodeReview.override_enabled(true);

        let terminal = add_window_with_terminal(&mut app, None);
        terminal.update(&mut app, |view, _| {
            view.model.lock().simulate_long_running_block("vim", "");
        });

        terminal.read(&app, |view, ctx| {
            assert_eq!(CLIAgent::detect("vim", None, None, ctx), None);
            assert_eq!(
                view.active_cli_agent(ctx),
                None,
                "a non-TUI long-running command must not be a review destination"
            );
        });
    });
}

#[test]
fn visible_bootstrap_block_leaves_focus_on_tab_rename_editor() {
    App::test((), |mut app| async move {
        initialize_workspace_app(&mut app);
        let workspace = mock_workspace(&mut app);
        let (window, terminal) = workspace.update(&mut app, |workspace, ctx| {
            workspace.rename_tab(0, ctx);
            let terminal = workspace
                .active_tab_pane_group()
                .as_ref(ctx)
                .active_session_view(ctx)
                .expect("tab should contain a terminal");
            (ctx.window_id(), terminal)
        });
        assert!(workspace.read(&app, |workspace, ctx| {
            workspace.is_inline_rename_editor_focused(ctx)
        }));
        let focused_before = app.focused_view_id(window);

        terminal.update(&mut app, |view, ctx| {
            view.handle_terminal_event(&ModelEvent::VisibleBootstrapBlock, ctx);
        });

        assert_eq!(app.focused_view_id(window), focused_before);
        assert!(workspace.read(&app, |workspace, ctx| {
            workspace.is_inline_rename_editor_focused(ctx)
        }));
    });
}

#[test]
fn visible_bootstrap_block_leaves_focus_on_tab_group_rename_editor() {
    let _grouped_tabs_guard = FeatureFlag::GroupedTabs.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_workspace_app(&mut app);
        let workspace = mock_workspace(&mut app);
        let (window, terminal, group_id) = workspace.update(&mut app, |workspace, ctx| {
            workspace.handle_action(
                &WorkspaceAction::SelectNewSessionMenuItem(NewSessionMenuItem::CreateNewTabGroup),
                ctx,
            );
            let group_id = workspace.tabs[0]
                .group_id
                .expect("active tab should be assigned to the new group");
            let terminal = workspace
                .active_tab_pane_group()
                .as_ref(ctx)
                .active_session_view(ctx)
                .expect("new tab group should contain a terminal");
            (ctx.window_id(), terminal, group_id)
        });
        workspace.update(&mut app, |workspace, ctx| {
            workspace.rename_tab_group(group_id, ctx);
        });
        assert!(workspace.read(&app, |workspace, ctx| {
            workspace.is_inline_rename_editor_focused(ctx)
        }));
        let focused_before = app.focused_view_id(window);

        terminal.update(&mut app, |view, ctx| {
            view.handle_terminal_event(&ModelEvent::VisibleBootstrapBlock, ctx);
        });

        assert_eq!(app.focused_view_id(window), focused_before);
        assert!(workspace.read(&app, |workspace, ctx| {
            workspace.is_inline_rename_editor_focused(ctx)
        }));
    });
}
