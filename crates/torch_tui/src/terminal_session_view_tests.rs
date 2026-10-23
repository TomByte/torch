use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use chrono::NaiveDate;
use instant::Instant;
use string_offset::CharOffset;
use tempfile::TempDir;
use torch::appearance::Appearance;
#[cfg(feature = "voice_input")]
use torch::settings::TuiVoiceSettings;
use torch::settings::{
    AISettings, SettingsFileError, TuiStatuslineConfig, TuiStatuslineItem, TuiTheme,
    TuiThemeSettings, TuiZeroStateObject,
};
use torch::terminal::model::ansi::{Handler, InputBufferValue, Mode};
use torch::terminal::model::session::SessionInfo;
use torch::terminal::model::session::command_executor::LocalCommandExecutor;
use torch::terminal::shell::ShellType;
use torch::tui_export::{
    BlockPadding, LinkedWorkflowData, PtyIntent, PtyIntentEvent, Session, SizeInfo, SizeUpdate,
    TranscriptScope, TuiUpArrowHistoryItemKind, WarpConfig, WarpConfigUpdateEvent,
    export_conversation_markdown, register_tui_session_view_test_singletons, slash_commands,
};
use torch_core::channel::Channel;
use torch_core::settings::Setting as _;
use torch_editor::model::CoreEditorModel;
use torchui::platform::WindowStyle;
use torchui::{AddWindowOptions, EntityIdMap, ModelHandle, SingletonEntity, ViewHandle};
use torchui_core::r#async::{FutureExt as _, Timer};
use torchui_core::elements::tui::{
    TuiBuffer, TuiBufferExt, TuiConstrainedBox, TuiConstraint, TuiContainer, TuiElement,
    TuiLayoutContext, TuiPaintContext, TuiPaintSurface, TuiRect, TuiScreenPosition, TuiSize,
    TuiStyle, TuiText,
};
#[cfg(feature = "voice_input")]
use torchui_core::event::KeyState;
use torchui_core::keymap::{Keystroke, Trigger};
use torchui_core::presenter::tui::{TuiFrame, TuiPresenter};
use torchui_core::{App, AppContext, TuiView, TypedActionView, WindowInvalidation};

use super::statusline::{
    ContextWindowUsage, FooterSegment, FooterSegments, format_context_window_usage,
    format_statusline_date, format_statusline_time_12_hour, format_statusline_time_24_hour,
    format_todo_progress, render_git_branch_status, render_status_footer_row,
    render_statusline_datetime, should_render_plain_git_branch,
};
use super::{
    AUTO_APPROVE_ENABLED_HINT, BlockingInputSource, CTRL_C_EXIT_HINT, ConversationRestoreState,
    INLINE_MENU_TOP_PADDING_ROWS, LOADING_CONVERSATION_HINT, LOG_BUNDLE_FAILED_HINT,
    RUNNING_COMMAND_DETACH_HINT, SESSION_CAN_ATTACH_AGENT_TO_RUNNING_COMMAND_FLAG, SHELL_MODE_HINT,
    STATUSLINE_RESET_HINT, TuiConversationRestoreOrigin, TuiTerminalSessionAction,
    TuiTerminalSessionEvent, attachment_focus_available, export_file_success_message,
    log_bundle_success_message, raw_prompt_if_not_blank,
};
#[cfg(feature = "voice_input")]
use super::{
    SESSION_COMPOSER_SHORTCUTS_ACTIVE_FLAG, VOICE_INPUT_BINDING_NAME, VOICE_USAGE_HINT,
    voice_argument_is_empty, voice_command_argument,
};
use crate::autoupdate::TuiAutoupdater;
use crate::editor_element::TuiEditorAction;
use crate::inline_menu::MAX_INLINE_MENU_ROWS;
use crate::input::view::TuiInputAction;
use crate::input_suggestions_mode::TuiInputSuggestionsMode;
use crate::keybindings::TUI_BINDING_GROUP;
use crate::orchestration_model::TuiOrchestrationModel;
use crate::read_only_menu::TuiReadOnlyMenuKind;
use crate::root_view::RootTuiView;
use crate::session_registry::{TuiSessionId, TuiSessions};
use crate::statusline_config_view::TuiStatuslineConfigEvent;
use crate::telemetry::TuiConversationRestoreTelemetryTarget;
use crate::terminal_block::{block_content_rows, should_render_terminal_block};
use crate::terminal_use::TuiInputTarget;
use crate::test_fixtures::{
    add_test_semantic_selection, add_test_terminal_session,
    add_test_terminal_session_with_settings_file_error,
};
use crate::transcript_view::TRANSCRIPT_BLOCK_SPACING;
use crate::transient_hint::TransientHintTone;
use crate::tui_builder::TuiUiBuilder;
#[cfg(feature = "voice_input")]
use crate::voice_input::{TuiVoiceInputState, requires_modifier_key_reporting};
use crate::zero_state_animation::{
    ZeroStateAnimationConfig, ZeroStateAnimationConfigEvent, ZeroStateAnimationLoadFailure,
};

struct FocusTestFixture {
    window_id: torchui_core::WindowId,
    sessions: ModelHandle<TuiSessions>,
}

#[test]
fn figma_statusline_metadata_formats_are_stable() {
    let now = NaiveDate::from_ymd_opt(2026, 7, 20)
        .unwrap()
        .and_hms_opt(13, 8, 0)
        .unwrap();
    assert_eq!(format_statusline_date(now), "July 20, 2026");
    assert_eq!(format_statusline_time_12_hour(now), "1:08pm");
    assert_eq!(format_statusline_time_24_hour(now), "13:08");
    assert_eq!(format_todo_progress(1, 10, false), "❒ 1/10");
    assert_eq!(format_todo_progress(10, 10, true), "✓ 10/10");
    assert_eq!(
        format_context_window_usage(0.0),
        ContextWindowUsage {
            bar: "████".to_owned(),
            percentage_remaining: 100,
            warning: false,
        }
    );
    assert_eq!(
        format_context_window_usage(0.25),
        ContextWindowUsage {
            bar: "███░".to_owned(),
            percentage_remaining: 75,
            warning: false,
        }
    );
    assert_eq!(
        format_context_window_usage(0.5),
        ContextWindowUsage {
            bar: "██░░".to_owned(),
            percentage_remaining: 50,
            warning: false,
        }
    );
    assert_eq!(
        format_context_window_usage(0.75),
        ContextWindowUsage {
            bar: "█░░░".to_owned(),
            percentage_remaining: 25,
            warning: true,
        }
    );
}

#[test]
fn statusline_datetime_requests_a_periodic_repaint() {
    App::test((), |app| async move {
        app.read(|ctx| {
            let datetime =
                render_statusline_datetime(format_statusline_time_24_hour, TuiStyle::default());
            let mut presenter = TuiPresenter::new();
            let frame = presenter.present_element(datetime, TuiRect::new(0, 0, 5, 1), ctx);
            assert!(
                frame.repaint_at.is_some(),
                "visible date/time items must repaint so their value cannot freeze"
            );
        });
    });
}

#[test]
fn footer_supports_arbitrary_order_and_figma_group_dividers() {
    App::test((), |mut app| async move {
        app.update(|ctx| {
            ctx.add_singleton_model(|_| Appearance::mock());
            let builder = TuiUiBuilder::from_app(ctx);
            let row = render_status_footer_row(
                FooterSegments {
                    ordered: vec![
                        FooterSegment::ContextWindowUsage(format_context_window_usage(0.426)),
                        FooterSegment::GitBranch("feature/statusline".to_owned()),
                        FooterSegment::AutoApproveIndicator(TuiText::new("▶▶").finish()),
                        FooterSegment::WorkingDirectory("/tmp/warp".to_owned()),
                        FooterSegment::DateTime(TuiText::new("July 20, 2026").finish()),
                    ],
                },
                &builder,
            )
            .finish();
            assert_eq!(
                render_element(row, ctx, 120).to_lines(),
                vec![
                    "██░░ 57% context remaining | ⊢ feature/statusline | ▶▶ | /tmp/warp | July 20, 2026"
                        .to_owned()
                ],
            );

            let branch_only = render_status_footer_row(
                FooterSegments {
                    ordered: vec![FooterSegment::GitBranch("main".to_owned())],
                },
                &builder,
            )
            .finish();
            assert_eq!(
                render_element(branch_only, ctx, 80).to_lines(),
                vec!["⊢ main".to_owned()],
            );
        });
    });
}

#[test]
fn git_diff_status_matches_figma_file_count_content_and_styles() {
    App::test((), |mut app| async move {
        app.update(|ctx| {
            ctx.add_singleton_model(|_| Appearance::mock());
            let builder = TuiUiBuilder::from_app(ctx);
            let row = render_status_footer_row(
                FooterSegments {
                    ordered: vec![FooterSegment::GitDiff {
                        files_changed: 6,
                        additions: 31,
                        deletions: 12,
                    }],
                },
                &builder,
            )
            .finish();
            let buffer = render_element(row, ctx, 80);
            assert_eq!(buffer.to_lines(), vec!["☰ 6 • +31 -12".to_owned()],);
            assert_eq!(
                buffer[(0, 0)].fg,
                builder
                    .muted_text_style()
                    .fg
                    .expect("file glyph should use the muted foreground"),
            );
            assert_eq!(
                buffer[(
                    (0..buffer.area().width)
                        .find(|column| buffer[(*column, 0)].symbol() == "+")
                        .expect("addition glyph should render"),
                    0,
                )]
                    .fg,
                builder
                    .diff_added_style()
                    .fg
                    .expect("addition count should use the added foreground"),
            );
            assert_eq!(
                buffer[(
                    (0..buffer.area().width)
                        .find(|column| buffer[(*column, 0)].symbol() == "-")
                        .expect("deletion glyph should render"),
                    0,
                )]
                    .fg,
                builder
                    .diff_removed_style()
                    .fg
                    .expect("deletion count should use the removed foreground"),
            );

            let file_only = render_status_footer_row(
                FooterSegments {
                    ordered: vec![FooterSegment::GitDiff {
                        files_changed: 1,
                        additions: 0,
                        deletions: 0,
                    }],
                },
                &builder,
            )
            .finish();
            assert_eq!(
                render_element(file_only, ctx, 80).to_lines(),
                vec!["☰ 1".to_owned()],
                "binary or zero-line changes should remain visible through their file count",
            );
        });
    });
}

#[test]
fn git_branch_status_matches_figma_content_styles_and_tracking_variants() {
    App::test((), |mut app| async move {
        app.update(|ctx| {
            ctx.add_singleton_model(|_| Appearance::mock());
            let builder = TuiUiBuilder::from_app(ctx);
            let status = render_element(
                render_git_branch_status(
                    "main",
                    false,
                    Some("1".to_owned()),
                    Some("2".to_owned()),
                    &builder,
                ),
                ctx,
                80,
            );
            assert_eq!(status.to_lines(), vec!["⊢ main • ↑1 ↓2".to_owned()]);
            let muted = builder
                .muted_text_style()
                .fg
                .expect("muted status text has a foreground");
            let accent = builder
                .accent_text_style()
                .fg
                .expect("branch status arrows have a foreground");
            assert_eq!(status[(0, 0)].fg, muted);
            assert_eq!(status[(9, 0)].fg, accent);
            assert_eq!(status[(10, 0)].fg, muted);
            assert_eq!(status[(12, 0)].fg, accent);
            assert_eq!(status[(13, 0)].fg, muted);

            assert_eq!(
                render_element(
                    render_git_branch_status("main", false, Some("1".to_owned()), None, &builder,),
                    ctx,
                    80,
                )
                .to_lines(),
                vec!["⊢ main • ↑1".to_owned()]
            );
            assert_eq!(
                render_element(
                    render_git_branch_status("main", false, None, Some("2".to_owned()), &builder,),
                    ctx,
                    80,
                )
                .to_lines(),
                vec!["⊢ main • ↓2".to_owned()]
            );
            assert_eq!(
                render_element(
                    render_git_branch_status("main", true, None, None, &builder),
                    ctx,
                    80,
                )
                .to_lines(),
                vec!["⊢ main • ⇅".to_owned()]
            );
            assert_eq!(
                render_element(
                    render_git_branch_status("main", false, None, None, &builder),
                    ctx,
                    80,
                )
                .to_lines(),
                vec!["⊢ main".to_owned()]
            );
        });
    });
}

#[test]
fn composite_git_branch_status_suppresses_the_plain_branch_item() {
    let branch_only = TuiStatuslineConfig {
        order: TuiStatuslineItem::ALL.to_vec(),
        enabled: vec![TuiStatuslineItem::GitBranch],
    };
    assert!(should_render_plain_git_branch(&branch_only));

    let branch_and_status = TuiStatuslineConfig {
        order: TuiStatuslineItem::ALL.to_vec(),
        enabled: vec![
            TuiStatuslineItem::GitBranch,
            TuiStatuslineItem::GitBranchStatus,
        ],
    };
    assert!(!should_render_plain_git_branch(&branch_and_status));
}

#[test]
fn empty_configurable_footer_has_zero_height() {
    App::test((), |mut app| async move {
        app.update(|ctx| {
            ctx.add_singleton_model(|_| Appearance::mock());
            let builder = TuiUiBuilder::from_app(ctx);
            let row = render_status_footer_row(
                FooterSegments {
                    ordered: Vec::new(),
                },
                &builder,
            )
            .finish();
            assert!(render_element(row, ctx, 80).to_lines().is_empty());
        });
    });
}

#[test]
fn shell_mode_reserves_tab_even_when_attachments_render() {
    assert!(attachment_focus_available(false, true));
    assert!(!attachment_focus_available(true, true));
    assert!(!attachment_focus_available(false, false));
}

#[test]
fn shell_completion_source_warmup_loads_path_executables() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        let command_dir = TempDir::new().unwrap();
        #[cfg(unix)]
        let shell_path = std::path::PathBuf::from("/bin/bash");
        #[cfg(windows)]
        let shell_path = std::path::PathBuf::from(
            std::env::var_os("ProgramFiles").expect("Windows must have ProgramFiles"),
        )
        .join(r"Git\usr\bin\bash.exe");
        #[cfg(unix)]
        let executable_name = "warp-completion-fixture";
        #[cfg(windows)]
        let executable_name = "warp-completion-fixture.exe";

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            let executable = command_dir.path().join(executable_name);
            std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(windows)]
        std::fs::copy(
            std::path::PathBuf::from(
                std::env::var_os("SystemRoot").expect("Windows must have a SystemRoot"),
            )
            .join(r"System32\where.exe"),
            command_dir.path().join(executable_name),
        )
        .unwrap();
        let session_info = SessionInfo::new_for_test()
            .with_shell_type(ShellType::Bash)
            .with_path(Some(command_dir.path().to_str().unwrap().to_owned()));
        let executor = Arc::new(LocalCommandExecutor::new(Some(shell_path), ShellType::Bash));
        let session = Arc::new(Session::new(session_info, executor));

        view.update(&mut app, |view, ctx| {
            view.warm_shell_completion_sources(session.clone(), ctx);
        });

        async {
            while !session.has_attempted_to_load_external_commands() {
                Timer::after(Duration::from_millis(10)).await;
            }
        }
        .with_timeout(Duration::from_secs(5))
        .await
        .expect("Completion warmup must start external command discovery");

        session
            .load_external_commands()
            .with_timeout(Duration::from_secs(5))
            .await
            .expect("External command discovery must finish");
        assert!(session.has_loaded_external_commands());
        assert!(
            session
                .executable_names()
                .any(|command| command == executable_name)
        );
    });
}

#[test]
fn log_bundle_success_message_includes_the_absolute_path() {
    let path = std::path::Path::new("/tmp/warp-20260718-132640.zip");
    assert_eq!(
        log_bundle_success_message(path),
        "Log bundle saved to /tmp/warp-20260718-132640.zip"
    );
}

#[test]
fn tui_cli_shell_command_uses_channel_entry_points() {
    assert_eq!(
        super::tui_cli_shell_command(Channel::Local, "--version"),
        "./script/run-tui -- --version"
    );
    assert_eq!(
        super::tui_cli_shell_command(Channel::Stable, "--version"),
        "warp --version"
    );
    assert_eq!(
        super::tui_cli_shell_command(Channel::Dev, "--version"),
        "warp-dev --version"
    );
    assert_eq!(
        super::tui_cli_shell_command(Channel::Preview, "--version"),
        "warp-preview --version"
    );
    assert_eq!(
        super::tui_cli_shell_command(Channel::Oss, "--version"),
        "torch-tui --version"
    );
    assert_eq!(
        super::tui_cli_shell_command(Channel::Integration, "--version"),
        "warp-integration --version"
    );
}

#[test]
fn log_bundle_failure_hint_does_not_hardcode_a_frontend_path() {
    assert!(!LOG_BUNDLE_FAILED_HINT.contains("warp.log"));
    assert!(!LOG_BUNDLE_FAILED_HINT.contains("/oz/"));
    assert!(!LOG_BUNDLE_FAILED_HINT.contains("/tui/"));
    assert!(!LOG_BUNDLE_FAILED_HINT.contains("/warp-cli/"));
}

#[test]
fn inline_menu_padding_preserves_result_capacity() {
    App::test((), |app| async move {
        app.read(|ctx| {
            let menu_rows = (0..MAX_INLINE_MENU_ROWS)
                .map(|row| format!("menu {row}"))
                .collect::<Vec<_>>();
            let menu = TuiConstrainedBox::new(
                TuiContainer::new(TuiText::new(menu_rows.join("\n")).finish())
                    .with_padding_top(INLINE_MENU_TOP_PADDING_ROWS)
                    .finish(),
            )
            .with_max_rows(MAX_INLINE_MENU_ROWS + INLINE_MENU_TOP_PADDING_ROWS)
            .finish();
            let lines = render_element_with_size(
                menu,
                ctx,
                20,
                MAX_INLINE_MENU_ROWS + INLINE_MENU_TOP_PADDING_ROWS,
            )
            .to_lines();

            assert_eq!(lines.len(), usize::from(MAX_INLINE_MENU_ROWS + 1));
            assert!(lines[0].trim().is_empty());
            assert_eq!(&lines[1..], menu_rows);
        });
    });
}

#[test]
fn zero_state_reload_failure_renders_as_an_error_footer_hint() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);

        app.update(|ctx| {
            ZeroStateAnimationConfig::handle(ctx).update(ctx, |_, ctx| {
                ctx.emit(ZeroStateAnimationConfigEvent::LoadFailed(
                    ZeroStateAnimationLoadFailure::Reload,
                ));
            });
        });

        assert_eq!(
            view.read(&app, |view, _| {
                view.transient_hint
                    .current()
                    .map(|(text, tone)| (text.to_owned(), tone))
            }),
            Some((
                super::ZERO_STATE_ASCII_RELOAD_FAILED_HINT.to_owned(),
                TransientHintTone::Error
            ))
        );

        app.read(|ctx| {
            let footer = view.as_ref(ctx).render_footer(ctx).finish();
            let buffer = render_element(footer, ctx, 120);
            assert_eq!(
                buffer.to_lines(),
                vec![super::ZERO_STATE_ASCII_RELOAD_FAILED_HINT.to_owned()]
            );
            assert_eq!(
                buffer[(0, 0)].fg,
                TuiUiBuilder::from_app(ctx)
                    .error_text_style()
                    .fg
                    .expect("error text style should have a foreground")
            );
        });
    });
}

#[test]
fn settings_reload_failure_renders_as_an_error_footer_hint() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);

        app.update(|ctx| {
            WarpConfig::handle(ctx).update(ctx, |_, ctx| {
                ctx.emit(WarpConfigUpdateEvent::SettingsErrors(
                    SettingsFileError::InvalidSettings(vec!["Theme".to_owned()]),
                ));
            });
        });

        assert_eq!(
            view.read(&app, |view, _| {
                view.transient_hint
                    .current()
                    .map(|(text, tone)| (text.to_owned(), tone))
            }),
            Some((
                super::SETTINGS_INVALID_VALUES_HINT.to_owned(),
                TransientHintTone::Error
            ))
        );

        app.read(|ctx| {
            let footer = view.as_ref(ctx).render_footer(ctx).finish();
            assert_eq!(
                render_element(footer, ctx, 120).to_lines(),
                vec![super::SETTINGS_INVALID_VALUES_HINT.to_owned()]
            );
        });
    });
}

#[test]
fn theme_slash_command_accepts_direct_selection_and_rejects_invalid_values() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        let light = "light".to_owned();

        view.update(&mut app, |view, ctx| {
            view.execute_tui_slash_command(&slash_commands::THEME, Some(&light), ctx);
        });
        app.read(|ctx| {
            assert_eq!(
                TuiTheme::from(Appearance::as_ref(ctx).theme()),
                TuiTheme::Light
            );
            assert_eq!(
                TuiThemeSettings::as_ref(ctx).selected_theme(),
                TuiTheme::Light
            );
        });
        let dark = "dark".to_owned();
        view.update(&mut app, |view, ctx| {
            view.execute_tui_slash_command(&slash_commands::THEME, Some(&dark), ctx);
        });
        app.read(|ctx| {
            assert_eq!(
                TuiTheme::from(Appearance::as_ref(ctx).theme()),
                TuiTheme::Dark
            );
            assert_eq!(
                TuiThemeSettings::as_ref(ctx).selected_theme(),
                TuiTheme::Dark
            );
        });

        let auto = "auto".to_owned();
        view.update(&mut app, |view, ctx| {
            view.execute_tui_slash_command(&slash_commands::THEME, Some(&auto), ctx);
        });
        app.read(|ctx| {
            assert_eq!(
                TuiTheme::from(Appearance::as_ref(ctx).theme()),
                TuiTheme::Dark
            );
            assert_eq!(
                TuiThemeSettings::as_ref(ctx).selected_theme(),
                TuiTheme::Auto
            );
        });

        let invalid = "sepia".to_owned();
        view.update(&mut app, |view, ctx| {
            view.execute_tui_slash_command(&slash_commands::THEME, Some(&invalid), ctx);
        });
        app.read(|ctx| {
            assert_eq!(
                TuiThemeSettings::as_ref(ctx).selected_theme(),
                TuiTheme::Auto
            );
        });
        assert_eq!(
            view.read(&app, |view, _| {
                view.transient_hint
                    .current()
                    .map(|(text, _)| text.to_owned())
            }),
            Some(super::THEME_INVALID_ARGUMENT_HINT.to_owned())
        );
    });
}

#[test]
fn zero_state_initial_load_failure_shows_an_error_footer_hint() {
    App::test((), |mut app| async move {
        let temp_dir = TempDir::new().unwrap();
        let config = ZeroStateAnimationConfig::load(
            &TuiZeroStateObject::AsciiFile {
                path: "missing.txt".into(),
            },
            5.0,
            0.18,
            temp_dir.path(),
        );
        app.add_singleton_model(move |_| config);
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);

        assert_eq!(
            view.read(&app, |view, _| {
                view.transient_hint
                    .current()
                    .map(|(text, tone)| (text.to_owned(), tone))
            }),
            Some((
                super::ZERO_STATE_ASCII_INITIAL_LOAD_FAILED_HINT.to_owned(),
                TransientHintTone::Error
            ))
        );
    });
}

#[test]
fn startup_settings_parse_failure_renders_as_an_error_footer_hint() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let view = add_focus_test_session_with_settings_file_error(
            &mut app,
            &fixture,
            SettingsFileError::FileParseFailed("expected a value".to_owned()),
        );

        assert_eq!(
            view.read(&app, |view, _| {
                view.transient_hint
                    .current()
                    .map(|(text, tone)| (text.to_owned(), tone))
            }),
            Some((
                super::SETTINGS_PARSE_FAILED_HINT.to_owned(),
                TransientHintTone::Error
            ))
        );

        app.read(|ctx| {
            let footer = view.as_ref(ctx).render_footer(ctx).finish();
            assert_eq!(
                render_element(footer, ctx, 120).to_lines(),
                vec![super::SETTINGS_PARSE_FAILED_HINT.to_owned()]
            );
        });
    });
}

fn render_footer_lines(
    app: &mut App,
    view: &ViewHandle<super::TuiTerminalSessionView>,
    width: u16,
) -> Vec<String> {
    render_footer(app, view, width).to_lines()
}

#[test]
fn input_area_renders_all_six_editor_rows() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        view.update(&mut app, |view, ctx| {
            view.input_view.update(ctx, |input, ctx| {
                input.set_text("input-0\ninput-1\ninput-2\ninput-3\ninput-4\ninput-5", ctx);
            });
        });

        let rendered = render_session(&mut app, &view, 80, 24).join("\n");
        for row in 0..6 {
            assert!(
                rendered.contains(&format!("input-{row}")),
                "input row {row} should be visible:\n{rendered}"
            );
        }
    });
}

#[test]
fn theme_slash_command_rejects_a_missing_argument() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);

        app.read(|ctx| {
            assert_eq!(
                TuiTheme::from(Appearance::as_ref(ctx).theme()),
                TuiTheme::Dark
            );
            assert_eq!(
                TuiThemeSettings::as_ref(ctx).selected_theme(),
                TuiTheme::Auto
            );
        });

        view.update(&mut app, |view, ctx| {
            view.execute_tui_slash_command(&slash_commands::THEME, None, ctx);
        });
        app.read(|ctx| {
            assert_eq!(
                TuiTheme::from(Appearance::as_ref(ctx).theme()),
                TuiTheme::Dark
            );
            assert_eq!(
                TuiThemeSettings::as_ref(ctx).selected_theme(),
                TuiTheme::Auto
            );
        });
        assert_eq!(
            view.read(&app, |view, _| {
                view.transient_hint
                    .current()
                    .map(|(text, _)| text.to_owned())
            }),
            Some(super::THEME_INVALID_ARGUMENT_HINT.to_owned())
        );
    });
}

#[test]
fn statusline_slash_command_preserves_nested_focus_and_restores_the_input_cursor() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        view.update(&mut app, |view, ctx| {
            view.input_view.update(ctx, |input, ctx| {
                input.set_text("/statusline", ctx);
            });
            view.execute_tui_slash_command(&slash_commands::STATUSLINE, None, ctx);
        });

        let (picker_id, picker_focus_id) = view.read(&app, |view, ctx| {
            let picker = view
                .statusline_config_view
                .as_ref()
                .expect("statusline picker should be open");
            assert_eq!(
                view.input_view
                    .as_ref(ctx)
                    .model()
                    .as_ref(ctx)
                    .content()
                    .as_ref(ctx)
                    .text()
                    .into_string(),
                ""
            );
            assert!(ctx.check_view_or_child_focused(fixture.window_id, &picker.id()));
            (
                picker.id(),
                ctx.focused_view_id(fixture.window_id)
                    .expect("the statusline picker should delegate focus to a child"),
            )
        });

        assert!(view.update(&mut app, |view, ctx| { view.handle_terminal_wakeup(ctx) }));
        app.read(|ctx| {
            assert_eq!(
                ctx.focused_view_id(fixture.window_id),
                Some(picker_focus_id),
                "a redraw must preserve the interaction surface's delegated child focus"
            );
        });
        view.update(&mut app, |view, ctx| {
            view.execute_tui_slash_command(&slash_commands::STATUSLINE, None, ctx);
        });
        assert_eq!(
            view.read(&app, |view, _| {
                view.statusline_config_view.as_ref().map(ViewHandle::id)
            }),
            Some(picker_id),
        );

        view.update(&mut app, |view, ctx| {
            view.handle_statusline_config_event(&TuiStatuslineConfigEvent::Cancelled, ctx);
        });
        view.read(&app, |view, ctx| {
            assert!(view.statusline_config_view.is_none());
            assert!(ctx.check_view_or_child_focused(fixture.window_id, &view.input_view.id()));
        });
        assert!(
            render_session_frame(&mut app, &view, 80, 24)
                .cursor
                .is_some(),
            "dismissing the interaction surface should restore the input cursor"
        );
    });
}

#[test]
fn saving_statusline_configuration_persists_and_restores_input_focus() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        let config = TuiStatuslineConfig {
            order: vec![
                TuiStatuslineItem::ContextWindowUsage,
                TuiStatuslineItem::CreditUsage,
            ],
            enabled: Vec::new(),
        }
        .normalized();

        view.update(&mut app, |view, ctx| {
            view.execute_tui_slash_command(&slash_commands::STATUSLINE, None, ctx);
            view.handle_statusline_config_event(
                &TuiStatuslineConfigEvent::Saved(config.clone()),
                ctx,
            );
        });

        assert_eq!(
            app.read(|ctx| AISettings::as_ref(ctx).tui_statusline.normalized()),
            config,
        );
        view.read(&app, |view, ctx| {
            assert!(view.statusline_config_view.is_none());
            assert!(ctx.check_view_or_child_focused(fixture.window_id, &view.input_view.id()));
            assert_eq!(
                view.transient_hint.current().map(|(text, _)| text),
                Some(super::STATUSLINE_SAVED_HINT),
            );
        });
    });
}

#[test]
fn reset_statusline_command_restores_default_items_and_ordering() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        let custom = TuiStatuslineConfig {
            order: vec![TuiStatuslineItem::CreditUsage, TuiStatuslineItem::Model],
            enabled: vec![TuiStatuslineItem::CreditUsage],
        }
        .normalized();
        assert_ne!(custom, TuiStatuslineConfig::default());

        view.update(&mut app, |view, ctx| {
            AISettings::handle(ctx).update(ctx, |settings, ctx| {
                settings
                    .tui_statusline
                    .set_value(custom, ctx)
                    .expect("custom statusline should persist");
            });
            view.input_view.update(ctx, |input, ctx| {
                input.set_text("/reset-statusline", ctx);
            });
            view.execute_tui_slash_command(&slash_commands::RESET_STATUSLINE, None, ctx);
        });

        assert_eq!(
            app.read(|ctx| AISettings::as_ref(ctx).tui_statusline.clone()),
            TuiStatuslineConfig::default(),
        );
        view.read(&app, |view, ctx| {
            assert!(view.statusline_config_view.is_none());
            assert_eq!(
                view.input_view
                    .as_ref(ctx)
                    .model()
                    .as_ref(ctx)
                    .content()
                    .as_ref(ctx)
                    .text()
                    .into_string(),
                ""
            );
            assert_eq!(
                view.transient_hint.current().map(|(text, _)| text),
                Some(STATUSLINE_RESET_HINT),
            );
        });
    });
}

fn focus_test_fixture(app: &mut App) -> FocusTestFixture {
    register_tui_session_view_test_singletons(app);
    add_test_semantic_selection(app);
    app.update(TuiAutoupdater::register);
    let (window_id, _) = app.update(|ctx| {
        ctx.add_tui_window(
            AddWindowOptions {
                window_style: WindowStyle::NotStealFocus,
                ..Default::default()
            },
            RootTuiView::new,
        )
    });
    let sessions = app.add_singleton_model(|_| TuiSessions::new_for_test());
    let orchestration = app.update(TuiOrchestrationModel::register);
    app.update(|ctx| TuiSessions::wire_orchestration(&sessions, &orchestration, ctx));
    FocusTestFixture {
        window_id,
        sessions,
    }
}

fn add_focus_test_session(
    app: &mut App,
    fixture: &FocusTestFixture,
    focus: bool,
) -> (ViewHandle<super::TuiTerminalSessionView>, TuiSessionId) {
    let (view, manager) = add_test_terminal_session(app, fixture.window_id);
    let session_id = app.update(|ctx| {
        TuiSessions::register_session(&fixture.sessions, view.clone(), manager, focus, ctx)
    });
    (view, session_id)
}

fn add_focus_test_session_with_settings_file_error(
    app: &mut App,
    fixture: &FocusTestFixture,
    error: SettingsFileError,
) -> ViewHandle<super::TuiTerminalSessionView> {
    let (view, manager) =
        add_test_terminal_session_with_settings_file_error(app, fixture.window_id, Some(error));
    app.update(|ctx| {
        TuiSessions::register_session(&fixture.sessions, view.clone(), manager, true, ctx);
    });
    view
}

fn render_element(element: Box<dyn TuiElement>, ctx: &AppContext, width: u16) -> TuiBuffer {
    render_element_with_size(element, ctx, width, 1)
}

fn render_element_with_size(
    mut element: Box<dyn TuiElement>,
    ctx: &AppContext,
    width: u16,
    height: u16,
) -> TuiBuffer {
    let mut rendered_views = EntityIdMap::default();
    let mut layout_ctx = TuiLayoutContext {
        rendered_views: &mut rendered_views,
    };
    let size = element.layout(
        TuiConstraint::loose(TuiSize::new(width, height)),
        &mut layout_ctx,
        ctx,
    );
    let area = TuiRect::new(0, 0, size.width, size.height);
    let mut buffer = TuiBuffer::empty(area);
    let mut paint_ctx = TuiPaintContext::new(&mut rendered_views);
    {
        let mut surface = TuiPaintSurface::new(&mut buffer);
        element.render(
            TuiScreenPosition::new(i32::from(area.x), i32::from(area.y)),
            &mut surface,
            &mut paint_ctx,
        );
    }
    buffer
}

fn render_session(
    app: &mut App,
    view: &ViewHandle<super::TuiTerminalSessionView>,
    width: u16,
    height: u16,
) -> Vec<String> {
    render_session_buffer(app, view, width, height).to_lines()
}

fn render_session_buffer(
    app: &mut App,
    view: &ViewHandle<super::TuiTerminalSessionView>,
    width: u16,
    height: u16,
) -> TuiBuffer {
    render_session_frame(app, view, width, height).buffer
}

fn render_session_frame(
    app: &mut App,
    view: &ViewHandle<super::TuiTerminalSessionView>,
    width: u16,
    height: u16,
) -> TuiFrame {
    let mut presenter = TuiPresenter::new();
    app.update(|ctx| {
        let mut invalidation = WindowInvalidation::default();
        invalidation.updated.insert(view.id());
        invalidation
            .updated
            .extend(view.as_ref(ctx).child_view_ids(ctx));
        presenter.invalidate(&invalidation, ctx, view.window_id(ctx));
        presenter.present(ctx, view, TuiRect::new(0, 0, width, height))
    })
}

fn first_visible_column(line: &str) -> usize {
    line.chars()
        .position(|character| !character.is_whitespace())
        .unwrap_or_else(|| panic!("line must contain visible content: {line:?}"))
}

#[test]
fn input_adjacent_surfaces_follow_figma_outer_edge_alignment() {
    App::test((), |mut app| async move {
        app.update(crate::keybindings::init);
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);

        let lines = render_session(&mut app, &view, 80, 24);
        let input_border_column = lines
            .iter()
            .find(|line| line.contains('▏'))
            .map(|line| first_visible_column(line))
            .unwrap_or_else(|| panic!("input border must render:\n{}", lines.join("\n")));
        let statusline_column = lines
            .iter()
            .find(|line| line.contains("auto (cost-efficient)"))
            .map(|line| first_visible_column(line))
            .unwrap_or_else(|| panic!("statusline must render:\n{}", lines.join("\n")));
        assert_eq!(
            statusline_column,
            input_border_column,
            "statusline must begin at the input border's outer edge:\n{}",
            lines.join("\n")
        );

        view.update(&mut app, |view, ctx| {
            view.input_view.update(ctx, |input, ctx| {
                input.set_text("/", ctx);
            });
        });
        futures_lite::future::yield_now().await;
        let buffer = render_session_buffer(&mut app, &view, 80, 24);
        let lines = buffer.to_lines();
        let (slash_command_row, slash_command_column) = lines
            .iter()
            .enumerate()
            .find(|(_, line)| line.contains("/agent"))
            .map(|(row, line)| (row, first_visible_column(line)))
            .unwrap_or_else(|| panic!("slash-command menu must render:\n{}", lines.join("\n")));
        assert_eq!(
            slash_command_column,
            input_border_column,
            "inline-menu content must begin at the input border's outer edge:\n{}",
            lines.join("\n")
        );
        assert_eq!(
            buffer[(input_border_column as u16, slash_command_row as u16)].bg,
            app.read(|ctx| TuiUiBuilder::from_app(ctx).slash_command_selection_background()),
            "selected inline-menu background must begin at the input border's outer edge"
        );

        view.update(&mut app, |view, ctx| {
            view.suggestions_mode.update(ctx, |mode, ctx| {
                mode.set_mode(
                    TuiInputSuggestionsMode::ReadOnlyMenu(TuiReadOnlyMenuKind::Shortcuts),
                    ctx,
                );
            });
        });
        let buffer = render_session_buffer(&mut app, &view, 80, 24);
        let lines = buffer.to_lines();
        let (shortcuts_row, shortcuts_column) = lines
            .iter()
            .enumerate()
            .find(|(_, line)| line.contains("Shortcuts"))
            .map(|(row, line)| (row, first_visible_column(line)))
            .unwrap_or_else(|| panic!("shortcuts surface must render:\n{}", lines.join("\n")));
        assert_eq!(
            shortcuts_column,
            input_border_column + 1,
            "shortcuts text keeps its designed one-cell internal padding:\n{}",
            lines.join("\n")
        );
        assert_eq!(
            buffer[(input_border_column as u16, shortcuts_row as u16)].bg,
            app.read(|ctx| TuiUiBuilder::from_app(ctx).read_only_menu_background()),
            "shortcuts background must begin at the input border's outer edge"
        );
    });
}

#[test]
fn shortcuts_surface_renders_above_the_input() {
    App::test((), |mut app| async move {
        app.update(crate::keybindings::init);
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        view.update(&mut app, |view, ctx| {
            view.suggestions_mode.update(ctx, |mode, ctx| {
                mode.set_mode(
                    TuiInputSuggestionsMode::ReadOnlyMenu(TuiReadOnlyMenuKind::Shortcuts),
                    ctx,
                );
            });
        });

        let rendered = render_session(&mut app, &view, 80, 24).join("\n");
        assert!(rendered.contains("Shortcuts"), "{rendered}");
        assert!(rendered.contains("? shortcuts"), "{rendered}");
        assert!(rendered.contains("/ commands"), "{rendered}");
        assert!(rendered.contains("! shell mode"), "{rendered}");
        assert!(rendered.contains("← conversations"), "{rendered}");
        assert!(rendered.contains("↑ input history"), "{rendered}");
        assert!(!rendered.contains("toggle auto-queue"), "{rendered}");
        assert!(rendered.contains("toggle auto-approve"), "{rendered}");
        // The shortcuts panel must NOT include the status section (that
        // lives in the dedicated status menu opened by /status).
        assert!(
            !rendered.contains("Version"),
            "Shortcuts panel must not show Version:\n{rendered}"
        );
        assert!(
            !rendered.contains("Working directory"),
            "Shortcuts panel must not show Working directory:\n{rendered}"
        );

        view.update(&mut app, |view, ctx| {
            view.handle_action(
                &TuiTerminalSessionAction::ToggleAutoApprove {
                    show_feedback: true,
                },
                ctx,
            );
        });
        let rendered = render_session(&mut app, &view, 80, 24).join("\n");
        assert!(rendered.contains("toggle auto-approve"), "{rendered}");
        assert!(rendered.contains(AUTO_APPROVE_ENABLED_HINT), "{rendered}");

        let narrow = render_session(&mut app, &view, 40, 24).join("\n");
        assert!(narrow.contains("Shortcuts"), "{narrow}");
        assert!(narrow.contains("? shortcuts"), "{narrow}");
    });
}

fn input_text(view: &ViewHandle<super::TuiTerminalSessionView>, ctx: &AppContext) -> String {
    view.as_ref(ctx)
        .input_view
        .as_ref(ctx)
        .model()
        .as_ref(ctx)
        .content()
        .as_ref(ctx)
        .text()
        .into_string()
}

#[test]
fn typeahead_event_inserts_and_overwrites_the_tui_input() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);

        view.update(&mut app, |view, ctx| {
            {
                let mut model = view.terminal_model.lock();
                model.simulate_long_running_block("sleep 5", "");
                model.finish_block();
                model.input_buffer(InputBufferValue {
                    buffer: "ec".to_owned(),
                    session_id: None,
                });
            }
            view.handle_typeahead_event(ctx);
        });
        assert_eq!(app.read(|ctx| input_text(&view, ctx)), "ec");

        view.update(&mut app, |view, ctx| {
            view.terminal_model.lock().input_buffer(InputBufferValue {
                buffer: "echo hi".to_owned(),
                session_id: None,
            });
            view.handle_typeahead_event(ctx);
        });
        assert_eq!(app.read(|ctx| input_text(&view, ctx)), "echo hi");
    });
}

#[test]
fn empty_typeahead_event_leaves_the_tui_input_unchanged() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        view.update(&mut app, |view, ctx| {
            view.input_view.update(ctx, |input, ctx| {
                input.set_text("draft", ctx);
            });
            {
                let mut model = view.terminal_model.lock();
                model.simulate_long_running_block("sleep 5", "");
                model.finish_block();
            }
            view.handle_typeahead_event(ctx);
        });

        assert_eq!(app.read(|ctx| input_text(&view, ctx)), "draft");
    });
}

#[test]
fn bootstrap_renders_starting_shell_above_input() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        view.update(&mut app, |view, _| {
            view.terminal_model
                .lock()
                .block_list_mut()
                .reinit_shell(None);
        });

        let lines = render_session(&mut app, &view, 80, 40);
        let status_index = lines
            .iter()
            .position(|line| line.trim() == "Starting shell...")
            .unwrap_or_else(|| panic!("bootstrap status should render:\n{}", lines.join("\n")));
        let input_index = lines
            .iter()
            .enumerate()
            .skip(status_index + 1)
            .find(|(_, line)| line.contains('▏') || line.contains('▁') || line.contains('─'))
            .map(|(index, _)| index)
            .expect("bootstrap input border should render below the status");
        assert!(status_index < input_index);
    });
}

#[test]
fn zero_state_position_stays_stable_across_shell_bootstrap() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);

        let ready_lines = render_session(&mut app, &view, 80, 40);
        assert!(
            ready_lines
                .iter()
                .all(|line| line.trim() != "Starting shell..."),
            "ready state must not render the bootstrap hint:\n{}",
            ready_lines.join("\n")
        );

        view.update(&mut app, |view, _| {
            view.terminal_model
                .lock()
                .block_list_mut()
                .reinit_shell(None);
        });
        let bootstrap_lines = render_session(&mut app, &view, 80, 40);
        assert!(
            bootstrap_lines
                .iter()
                .any(|line| line.trim() == "Starting shell..."),
            "bootstrap state must render the starting-shell hint:\n{}",
            bootstrap_lines.join("\n")
        );

        let title_row = |lines: &[String]| {
            lines
                .iter()
                .position(|line| line.contains("Torch Agent CLI"))
                .unwrap_or_else(|| panic!("zero-state title should render:\n{}", lines.join("\n")))
        };
        assert_eq!(
            title_row(&bootstrap_lines),
            title_row(&ready_lines),
            "zero state must not shift when the bootstrap hint disappears\nbootstrap:\n{}\nready:\n{}",
            bootstrap_lines.join("\n"),
            ready_lines.join("\n")
        );
    });
}

#[test]
fn submit_is_blocked_during_bootstrap_and_allowed_at_prompt() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);

        view.update(&mut app, |view, ctx| {
            view.input_view.update(ctx, |input, ctx| {
                input.set_text("draft", ctx);
            });
            view.terminal_model
                .lock()
                .block_list_mut()
                .reinit_shell(None);
            view.handle_submitted("draft".to_owned(), None, ctx);
        });

        assert_eq!(
            app.read(|ctx| input_text(&view, ctx)),
            "draft",
            "bootstrap submission must leave the draft untouched"
        );
        assert!(!view.read(&app, |view, _| {
            view.input_target().agent_editor_owns_input()
        }));
        assert!(TuiInputTarget::AgentEditor.agent_editor_owns_input());
    });
}

#[test]
fn accepted_command_history_executes_through_the_shell_submission_path() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        let executed = Rc::new(RefCell::new(Vec::new()));
        app.update(|ctx| {
            let executed = executed.clone();
            ctx.subscribe_to_view(&view, move |_, event, _| {
                if let TuiTerminalSessionEvent::ExecuteCommand(event) = event {
                    executed.borrow_mut().push(event.command.clone());
                }
            });
        });

        view.update(&mut app, |view, ctx| {
            view.handle_accepted_prompt_and_command_history(
                "echo from history".to_owned(),
                TuiUpArrowHistoryItemKind::Command {
                    linked_workflow_data: None,
                },
                ctx,
            );
        });

        assert_eq!(executed.borrow().as_slice(), &["echo from history"]);
        assert_eq!(app.read(|ctx| input_text(&view, ctx)), "");
    });
}

#[test]
fn accepted_command_history_preserves_workflow_metadata() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        let executed = Rc::new(RefCell::new(Vec::new()));
        app.update(|ctx| {
            let executed = executed.clone();
            ctx.subscribe_to_view(&view, move |_, event, _| {
                if let TuiTerminalSessionEvent::ExecuteCommand(event) = event {
                    executed.borrow_mut().push((**event).clone());
                }
            });
        });

        view.update(&mut app, |view, ctx| {
            view.handle_accepted_prompt_and_command_history(
                "deploy production".to_owned(),
                TuiUpArrowHistoryItemKind::Command {
                    linked_workflow_data: Some(LinkedWorkflowData::Command(
                        "deploy {{environment}}".to_owned(),
                    )),
                },
                ctx,
            );
        });

        let executed = executed.borrow();
        let event = executed.as_slice().first().expect("command was executed");
        assert_eq!(event.command, "deploy production");
        assert_eq!(event.workflow_id, None);
        assert_eq!(
            event.workflow_command.as_deref(),
            Some("deploy {{environment}}")
        );
    });
}

#[test]
fn long_running_command_keeps_input_hidden() {
    App::test((), |mut app| async move {
        app.update(crate::keybindings::init);
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        view.update(&mut app, |view, ctx| {
            let mut terminal_model = view.terminal_model.lock();
            terminal_model
                .block_list_mut()
                .set_transcript_scope(TranscriptScope::Unfiltered);
            terminal_model.simulate_block("echo ready", "ready\r\n");
            terminal_model.simulate_long_running_block("cat", "");
            drop(terminal_model);
            assert!(matches!(
                view.session_state(ctx)
                    .expect("session state resolves")
                    .blocking_input_source(),
                Some(&BlockingInputSource::LongRunningCommand)
            ));
            assert!(
                !view.transcript.as_ref(ctx).is_empty(),
                "command output should make the transcript visible"
            );
        });

        let lines = render_session(&mut app, &view, 80, 40);
        assert!(
            !lines
                .iter()
                .any(|line| line.trim_end() == "Starting shell..."),
            "LRC must not render bootstrap status:\n{}",
            lines.join("\n")
        );
        assert!(
            !lines
                .iter()
                .any(|line| line.chars().any(|glyph| "┌┐└┘─│▁▏▕▔".contains(glyph))),
            "LRC must keep the input editor hidden:\n{}",
            lines.join("\n")
        );
        // Manual attachment remains advertised while the running command is visible.
        let hint = view.read(&app, |view, ctx| {
            view.running_command_hint(ctx)
                .expect("visible running command should have an attachment hint")
        });
        assert!(
            lines.iter().any(|line| line.trim() == hint),
            "LRC must render the attach hint row:\n{}",
            lines.join("\n")
        );
        assert_eq!(hint, "Ctrl + Shift + ⏎  to use agent");
        assert!(
            lines
                .iter()
                .all(|line| !line.contains(RUNNING_COMMAND_DETACH_HINT)),
            "LRC must not show the detach hint before agent attachment:\n{}",
            lines.join("\n")
        );
    });
}

#[test]
fn user_controlled_alt_screen_keeps_full_session_input_on_the_pty() {
    App::test((), |mut app| async move {
        app.update(crate::keybindings::init);
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        view.update(&mut app, |view, _| {
            let mut terminal_model = view.terminal_model.lock();
            terminal_model.simulate_long_running_block("vim", "");
            terminal_model.set_mode(Mode::SwapScreen {
                save_cursor_and_clear_screen: true,
            });
            for character in "USER ALT SCREEN".chars() {
                terminal_model.alt_screen_mut().input(character);
            }
        });

        assert!(view.read(&app, |view, _| view.input_target().pty_owns_input()));
        let lines = render_session(&mut app, &view, 80, 12);
        let compact_output = lines
            .iter()
            .flat_map(|line| line.chars())
            .filter(|character| !character.is_whitespace())
            .collect::<String>();
        assert!(
            compact_output.contains("USERALTSCREEN"),
            "alternate-screen output should render:\n{}",
            lines.join("\n")
        );
        assert!(
            !lines.iter().any(|line| {
                line.chars().any(|glyph| "┌┐└┘─│▁▏▕▔".contains(glyph))
                    || line.contains("auto (cost-efficient)")
            }),
            "user-controlled alternate screen should not render the agent composer:\n{}",
            lines.join("\n")
        );
        let hint = view.read(&app, |view, ctx| {
            view.running_command_hint(ctx)
                .expect("alternate screen should have a running-command hint")
        });
        assert!(
            lines.iter().any(|line| line.trim() == hint),
            "user-controlled alternate screen should render the attach hint:\n{}",
            lines.join("\n")
        );
    });
}

/// Visible startup-script execution also routes input to the PTY, but it is
/// not a user-controlled command: the running-command hint row must not appear.
#[test]
fn visible_startup_script_shows_no_running_command_hint() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        view.update(&mut app, |view, ctx| {
            {
                let mut terminal_model = view.terminal_model.lock();
                terminal_model.block_list_mut().reinit_shell(None);
                terminal_model
                    .update_blockheight_items(TRANSCRIPT_BLOCK_SPACING.block_padding, 0.0);
                // Advance past WarpInput, then leave an unfinished startup-script
                // block with visible output owning PTY input.
                terminal_model.simulate_block("bootstrap", "");
                terminal_model.simulate_long_running_block("shell init", "startup output\r\n");
            }
            assert!(
                !view
                    .session_state(ctx)
                    .expect("session state resolves")
                    .can_attach_agent_to_running_command()
            );
            assert!(
                !view
                    .keymap_context(ctx)
                    .set
                    .contains(SESSION_CAN_ATTACH_AGENT_TO_RUNNING_COMMAND_FLAG)
            );
            assert!(
                !view.try_attach_agent_to_running_command(ctx),
                "startup-script input is not an attachable user LRC"
            );
            assert!(view.input_target().pty_owns_input());
        });
        assert!(
            view.read(&app, |view, _| view.input_target().pty_owns_input()),
            "fixture should route input to the PTY during the visible startup script"
        );

        let lines = render_session(&mut app, &view, 80, 40);
        assert!(
            !lines.iter().any(|line| line.contains("to use agent")),
            "startup-script execution must not advertise agent attachment:\n{}",
            lines.join("\n")
        );
    });
}

#[test]
fn zero_state_renders_with_only_zero_height_bootstrap_blocks() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        view.update(&mut app, |view, _| {
            let mut terminal_model = view.terminal_model.lock();
            terminal_model.block_list_mut().reinit_shell(None);
            terminal_model.update_blockheight_items(TRANSCRIPT_BLOCK_SPACING.block_padding, 0.0);
            terminal_model.simulate_block("bootstrap", "");
            terminal_model.simulate_long_running_block("shell init", "");
            let bootstrap_block_id = terminal_model.block_list().active_block().id().clone();
            terminal_model.finish_block();
            let bootstrap_block = terminal_model
                .block_list_mut()
                .mut_block_from_id(&bootstrap_block_id)
                .expect("bootstrap block should remain in the block list");
            bootstrap_block.set_should_hide_command_grid(true);
            terminal_model.update_blockheight_items(
                BlockPadding {
                    bottom: 1.0,
                    ..TRANSCRIPT_BLOCK_SPACING.block_padding
                },
                0.0,
            );

            let block_list = terminal_model.block_list();
            let bootstrap_block = block_list
                .block_with_id(&bootstrap_block_id)
                .expect("bootstrap block should remain in the block list");
            assert!(
                should_render_terminal_block(bootstrap_block, block_list),
                "fixture should contain an eligible shell bootstrap block"
            );
            assert!(
                block_content_rows(bootstrap_block).is_empty(),
                "fixture bootstrap block should have zero displayed height"
            );
        });
        view.read(&app, |view, ctx| {
            assert!(
                view.transcript.as_ref(ctx).is_empty(),
                "zero-height terminal blocks should leave the transcript empty"
            );
        });

        let mut presenter = TuiPresenter::new();
        let frame = app.update(|ctx| {
            let mut invalidation = WindowInvalidation::default();
            invalidation.updated.insert(view.id());
            invalidation
                .updated
                .extend(view.as_ref(ctx).child_view_ids(ctx));
            presenter.invalidate(&invalidation, ctx, fixture.window_id);
            presenter.present(ctx, &view, TuiRect::new(0, 0, 200, 40))
        });
        let lines = frame.buffer.to_lines();
        let title_row = lines
            .iter()
            .position(|line| line.contains("Torch Agent CLI"))
            .expect("zero state should render the Torch Agent CLI title");
        assert!(
            title_row < 28,
            "zero-state title should render in the transcript area:\n{}",
            lines.join("\n")
        );
        // The 32-column animation panel is centered in the 152 columns left
        // after the 48-column copy region: 48 + (152 - 32) / 2 = 108.
        let animation_start = 108;
        let animation_end = 140;
        assert!(
            lines.iter().take(28).any(|line| line
                .chars()
                .skip(animation_start)
                .take(animation_end - animation_start)
                .any(|character| character != ' ')),
            "animation content should render in the centered remaining-space panel:\n{}",
            lines.join("\n")
        );
        assert!(
            lines.iter().take(28).any(|line| line
                .chars()
                .skip(animation_end)
                .any(|character| character != ' ')),
            "starfield content should extend beyond the centered logo panel:\n{}",
            lines.join("\n")
        );
    });
}

#[test]
fn zero_state_transitions_through_bootstrap_lifecycle() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);

        // Phase 1: an unfinished ScriptExecution block with visible output suppresses the zero
        // state. The `|| !block.finished()` lifecycle guard covers this case: PTY input is still
        // routed to the block, so the zero state must stay hidden while the block runs.
        view.update(&mut app, |view, _| {
            let mut terminal_model = view.terminal_model.lock();
            terminal_model.block_list_mut().reinit_shell(None);
            terminal_model.update_blockheight_items(TRANSCRIPT_BLOCK_SPACING.block_padding, 0.0);
            // Advance past WarpInput to ScriptExecution.
            terminal_model.simulate_block("bootstrap", "");
            // Create an unfinished ScriptExecution block with visible output rows.
            terminal_model.simulate_long_running_block("shell init", "startup output\r\n");
        });
        view.read(&app, |view, ctx| {
            assert!(
                !view.transcript.as_ref(ctx).is_empty(),
                "unfinished startup block with visible content should suppress the zero state"
            );
        });

        // Phase 2: once the startup block finishes it no longer satisfies the lifecycle guard
        // (it is finished, not restored, and not PostBootstrapPrecmd), so the zero state returns.
        view.update(&mut app, |view, _| {
            let mut terminal_model = view.terminal_model.lock();
            // Advance bootstrap stage so finish_block() promotes the list to PostBootstrapPrecmd.
            terminal_model.block_list_mut().set_bootstrapped();
            terminal_model.finish_block();
        });
        view.read(&app, |view, ctx| {
            assert!(
                view.transcript.as_ref(ctx).is_empty(),
                "finished ScriptExecution block should no longer suppress the zero state"
            );
        });

        // Phase 3: the first normal post-bootstrap command dismisses the zero state.
        view.update(&mut app, |view, _| {
            view.terminal_model
                .lock()
                .simulate_block("echo hello", "hello\r\n");
        });
        view.read(&app, |view, ctx| {
            assert!(
                !view.transcript.as_ref(ctx).is_empty(),
                "post-bootstrap command with visible output should dismiss the zero state"
            );
        });
    });
}

fn render_footer(
    app: &mut App,
    view: &ViewHandle<super::TuiTerminalSessionView>,
    width: u16,
) -> TuiBuffer {
    app.update(|ctx| {
        let footer = view.as_ref(ctx).render_footer(ctx).finish();
        render_element(footer, ctx, width)
    })
}

/// A replacing hint occupies the whole status row, so no section separators,
/// branch arrows, or usage text should appear alongside it.
fn assert_footer_segments_absent(lines: &[String]) {
    let row = lines.join("\n");
    assert!(
        !row.contains('│'),
        "a replacing hint should occupy the whole row with no sections: {row}"
    );
    assert!(
        !row.contains(" | "),
        "a replacing hint should contain no statusline group dividers: {row}"
    );
    assert!(
        !row.contains(" ⊢ "),
        "the cwd/branch section is absent: {row}"
    );
    assert!(
        !row.contains("credits"),
        "the usage section is absent: {row}"
    );
}

#[test]
fn new_slash_command_clears_shell_commands_from_transcript() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        view.update(&mut app, |view, _| {
            let mut terminal_model = view.terminal_model.lock();
            terminal_model.block_list_mut().set_bootstrapped();
            terminal_model.simulate_block("echo before-new", "before-new\r\n");
        });

        view.read(&app, |view, ctx| {
            assert!(!view.transcript.as_ref(ctx).is_empty());
            assert!(
                view.terminal_model
                    .lock()
                    .block_list()
                    .blocks()
                    .iter()
                    .any(|block| block.command_to_string() == "echo before-new")
            );
        });

        view.update(&mut app, |view, ctx| {
            view.execute_tui_slash_command(&slash_commands::NEW, None, ctx);
        });

        view.read(&app, |view, ctx| {
            assert!(
                view.transcript.as_ref(ctx).is_empty(),
                "/new should clear both agent and shell transcript blocks"
            );
            assert_eq!(
                view.terminal_model.lock().block_list().blocks().len(),
                1,
                "/new should leave only the active prompt block"
            );
        });
    });
}

#[test]
fn clear_slash_command_clears_shell_commands_from_transcript() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        view.update(&mut app, |view, _| {
            let mut terminal_model = view.terminal_model.lock();
            terminal_model.block_list_mut().set_bootstrapped();
            terminal_model.simulate_block("echo before-clear", "before-clear\r\n");
        });

        view.read(&app, |view, ctx| {
            assert!(!view.transcript.as_ref(ctx).is_empty());
            assert!(
                view.terminal_model
                    .lock()
                    .block_list()
                    .blocks()
                    .iter()
                    .any(|block| block.command_to_string() == "echo before-clear")
            );
        });

        view.update(&mut app, |view, ctx| {
            view.execute_tui_slash_command(&slash_commands::CLEAR, None, ctx);
        });

        view.read(&app, |view, ctx| {
            assert!(
                view.transcript.as_ref(ctx).is_empty(),
                "/clear should clear both agent and shell transcript blocks, identical to /new"
            );
            assert_eq!(
                view.terminal_model.lock().block_list().blocks().len(),
                1,
                "/clear should leave only the active prompt block"
            );
        });
    });
}

#[test]
fn footer_renders_shell_mode_sections_without_model_or_usage() {
    App::test((), |mut app| async move {
        app.update(|ctx| {
            ctx.add_singleton_model(|_| Appearance::mock());
            let builder = TuiUiBuilder::from_app(ctx);
            let row = render_status_footer_row(
                FooterSegments {
                    ordered: vec![
                        FooterSegment::ShellMode,
                        FooterSegment::WorkingDirectory("/home/user/warp".to_owned()),
                        FooterSegment::GitBranch("main".to_owned()),
                        FooterSegment::GitDiff {
                            files_changed: 2,
                            additions: 3,
                            deletions: 1,
                        },
                    ],
                },
                &builder,
            )
            .finish();
            let buffer = render_element(row, ctx, 120);
            assert_eq!(
                buffer[(0, 0)].fg,
                builder
                    .shell_command_accent_style()
                    .fg
                    .expect("shell command accent has a foreground")
            );
            let lines = buffer.to_lines();
            let line = lines.join("\n");

            assert_eq!(
                lines,
                vec![format!(
                    "{SHELL_MODE_HINT} /home/user/warp ⊢ main | ☰ 2 • +3 -1"
                )],
                "shell footer leads with the shell-mode indicator and hides model/usage"
            );
            assert!(
                line.starts_with(SHELL_MODE_HINT),
                "shell mode is the first segment"
            );
            assert!(
                !line.contains("TestModel"),
                "model segment is hidden in shell mode"
            );
            assert!(
                !line.contains("2.5 credits"),
                "usage segment is hidden in shell mode"
            );
        });
    });
}

#[test]
fn footer_transient_state_replaces_all_sections() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);

        // ctrl-c exit confirmation replaces the whole row.
        view.update(&mut app, |view, _| {
            view.exit_confirmation.arm(Instant::now());
        });
        let lines = render_footer_lines(&mut app, &view, 80);
        assert_eq!(lines, vec![CTRL_C_EXIT_HINT]);
        assert_footer_segments_absent(&lines);

        // Loading-conversation hint replaces the whole row.
        view.update(&mut app, |view, _| {
            view.exit_confirmation.disarm();
            view.conversation_restore_state = ConversationRestoreState::Loading {
                origin: TuiConversationRestoreOrigin::ConversationList,
                target: TuiConversationRestoreTelemetryTarget::Local,
                request_id: 0,
                future: None,
            };
        });
        let lines = render_footer_lines(&mut app, &view, 80);
        assert_eq!(lines, vec![LOADING_CONVERSATION_HINT]);
        assert_footer_segments_absent(&lines);

        // A transient notice replaces the whole row.
        view.update(&mut app, |view, ctx| {
            view.conversation_restore_state = ConversationRestoreState::Idle;
            view.show_transient_hint("transient notice".to_owned(), ctx);
        });
        let lines = render_footer_lines(&mut app, &view, 80);
        assert_eq!(lines, vec!["transient notice"]);
        assert_footer_segments_absent(&lines);

        // Priority: when ctrl-c, loading, and a transient notice all overlap,
        // ctrl-c wins (the existing ctrl-c → loading → transient order).
        view.update(&mut app, |view, ctx| {
            view.exit_confirmation.arm(Instant::now());
            view.conversation_restore_state = ConversationRestoreState::Loading {
                origin: TuiConversationRestoreOrigin::ConversationList,
                target: TuiConversationRestoreTelemetryTarget::Local,
                request_id: 1,
                future: None,
            };
            view.show_transient_hint("transient notice".to_owned(), ctx);
        });
        let lines = render_footer_lines(&mut app, &view, 80);
        assert_eq!(lines, vec![CTRL_C_EXIT_HINT]);
    });
}

#[test]
fn interrupt_event_projects_to_high_level_pty_intent() {
    let event = TuiTerminalSessionEvent::InterruptPty;
    assert!(matches!(event.pty_intent(), Some(PtyIntent::Interrupt)));
}

#[test]
fn user_input_event_projects_to_raw_user_bytes() {
    let event = TuiTerminalSessionEvent::WriteUserInput(b"hello\r".to_vec().into());
    let Some(PtyIntent::WriteBytes(bytes)) = event.pty_intent() else {
        panic!("user input event should map to raw PTY bytes");
    };
    assert_eq!(&*bytes, b"hello\r");
}

#[test]
fn ctrl_d_is_owned_by_the_session_surface_not_input_delete_forward() {
    App::test((), |mut app| async move {
        app.update(crate::keybindings::init);
        app.read(|ctx| {
            let ctrl_d = Trigger::Keystrokes(vec![Keystroke::parse("ctrl-d").unwrap()]);

            // The prompt input no longer binds ctrl-d to delete-forward (the
            // session surface owns it); only the `delete` key deletes forward.
            let input_delete_forward_binds_ctrl_d = ctx
                .editable_bindings()
                .any(|b| b.name == "tui:input:delete_forward" && *b.trigger == ctrl_d);
            assert!(
                !input_delete_forward_binds_ctrl_d,
                "input delete-forward must not bind ctrl-d"
            );

            // The generic editor keeps ctrl-d as delete-forward.
            let editor_delete_forward_binds_ctrl_d = ctx
                .editable_bindings()
                .any(|b| b.name == "tui:editor:delete_forward" && *b.trigger == ctrl_d);
            assert!(
                editor_delete_forward_binds_ctrl_d,
                "editor delete-forward should still bind ctrl-d"
            );

            // The session handles ctrl-d only while the prompt is focused.
            // When a process owns focus, ctrl-d falls through to the terminal
            // element's standard PTY key encoding.
            let session_binds_ctrl_d = ctx.get_key_bindings().any(|b| {
                *b.trigger == ctrl_d && b.name.is_empty() && b.group == Some(TUI_BINDING_GROUP)
            });
            assert!(
                session_binds_ctrl_d,
                "the session should bind ctrl-d for prompt exit / deletion"
            );
        });
    });
}

#[test]
fn non_command_prompt_preserves_leading_whitespace() {
    assert_eq!(raw_prompt_if_not_blank("  /compact"), Some("  /compact"));
}

#[test]
fn whitespace_only_prompt_is_ignored() {
    assert_eq!(raw_prompt_if_not_blank(" \t\n"), None);
}

#[test]
fn file_export_success_message_includes_destination_path() {
    let directory = tempfile::tempdir().expect("temp directory");
    let export = export_conversation_markdown(
        Some(directory.path().to_str().expect("UTF-8 temp path")),
        Some("conversation.md"),
        None,
        "# Conversation",
    )
    .expect("conversation export");

    assert_eq!(
        export_file_success_message(&export),
        format!("Conversation exported to {}", export.path().display())
    );
}

#[test]
fn resize_event_maps_to_pty_resize_intent() {
    let last_size = SizeInfo::new_without_font_metrics(24, 120);
    let size_update = SizeUpdate::from_cell_dimensions(last_size, 8, 42);
    let event = TuiTerminalSessionEvent::Resize(size_update);

    let Some(PtyIntent::Resize(actual_update)) = event.pty_intent() else {
        panic!("resize event should map to a PTY resize intent");
    };
    assert_eq!(actual_update.new_size().rows(), 8);
    assert_eq!(actual_update.new_size().columns(), 42);
}

#[test]
fn terminal_wakeup_redraws_only_the_focused_session() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (foreground, _) = add_focus_test_session(&mut app, &fixture, true);
        let (background, _) = add_focus_test_session(&mut app, &fixture, false);

        assert!(foreground.update(&mut app, |view, ctx| { view.handle_terminal_wakeup(ctx) }));
        assert!(!background.update(&mut app, |view, ctx| { view.handle_terminal_wakeup(ctx) }));
    });
}

#[test]
fn terminal_wakeup_focuses_a_new_long_running_command() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        let input_id = view.read(&app, |view, _| view.input_view.id());

        view.update(&mut app, |view, ctx| {
            view.terminal_model
                .lock()
                .simulate_long_running_block("cat", "");
            assert!(view.input_target().pty_owns_input());
            assert_eq!(
                ctx.focused_view_id(fixture.window_id),
                Some(input_id),
                "the composer remains focused until the delayed terminal wakeup"
            );

            assert!(view.handle_terminal_wakeup(ctx));
        });

        app.read(|ctx| {
            assert_eq!(
                ctx.focused_view_id(fixture.window_id),
                Some(view.id()),
                "the PTY-owning session must receive input after the wakeup"
            );
        });
    });
}

#[test]
fn background_focus_reconciliation_does_not_steal_foreground_focus() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (foreground, _) = add_focus_test_session(&mut app, &fixture, true);
        let (background, _) = add_focus_test_session(&mut app, &fixture, false);
        let foreground_input_id = foreground.read(&app, |view, _| view.input_view.id());

        background.update(&mut app, |view, ctx| {
            view.reconcile_focus(ctx);
        });

        app.read(|ctx| {
            assert_eq!(
                ctx.focused_view_id(fixture.window_id),
                Some(foreground_input_id),
                "background ownership transitions must not change framework focus"
            );
        });
    });
}

#[test]
fn empty_background_attachment_update_does_not_steal_foreground_focus() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (foreground, _) = add_focus_test_session(&mut app, &fixture, true);
        let (background, _) = add_focus_test_session(&mut app, &fixture, false);
        let foreground_input_id = foreground.read(&app, |view, _| view.input_view.id());

        background.update(&mut app, |view, ctx| {
            assert!(!view.attachment_bar.as_ref(ctx).should_render(ctx));
            view.attachment_bar
                .update(ctx, |bar, ctx| bar.emit_model_updated_for_test(ctx));
        });

        app.read(|ctx| {
            assert_eq!(
                ctx.focused_view_id(fixture.window_id),
                Some(foreground_input_id),
                "an empty background context update must not focus its hidden input"
            );
        });
    });
}

#[test]
fn foreground_input_pointer_action_recovers_focus_from_background_session() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (foreground, _) = add_focus_test_session(&mut app, &fixture, true);
        let (background, _) = add_focus_test_session(&mut app, &fixture, false);
        let foreground_input = foreground.read(&app, |view, _| view.input_view.clone());
        let background_input = background.read(&app, |view, _| view.input_view.clone());

        background_input.update(&mut app, |_, ctx| ctx.focus_self());
        foreground_input.update(&mut app, |input, ctx| {
            input.handle_action(
                &TuiInputAction::Editor(TuiEditorAction::SelectionStartAt {
                    offset: CharOffset::from(1),
                }),
                ctx,
            );
        });

        assert!(app.read(|ctx| foreground_input.is_focused(ctx)));
    });
}

#[test]
fn stale_foreground_input_pointer_action_preserves_new_pty_owner() {
    App::test((), |mut app| async move {
        let fixture = focus_test_fixture(&mut app);
        let (foreground, _) = add_focus_test_session(&mut app, &fixture, true);
        let (background, _) = add_focus_test_session(&mut app, &fixture, false);
        let foreground_input = foreground.read(&app, |view, _| view.input_view.clone());
        let background_input = background.read(&app, |view, _| view.input_view.clone());

        foreground.update(&mut app, |view, _| {
            view.terminal_model
                .lock()
                .simulate_long_running_block("cat", "");
            assert!(view.input_target().pty_owns_input());
        });
        background_input.update(&mut app, |_, ctx| ctx.focus_self());
        foreground_input.update(&mut app, |input, ctx| {
            input.handle_action(
                &TuiInputAction::Editor(TuiEditorAction::SelectionStartAt {
                    offset: CharOffset::from(1),
                }),
                ctx,
            );
        });

        assert!(app.read(|ctx| foreground.is_focused(ctx)));
    });
}

// ── Vim mode tests ───────────────────────────────────────────────────────────

/// The `/vim-mode` slash command static definition must be correctly
/// populated: a non-empty name, a non-empty description, and a TUI-only
/// supported surface (so the command appears only in the TUI's slash-command
/// menu, not in the GUI).
///
/// The global `COMMAND_REGISTRY` is initialized with `SettingsMode::Gui` in
/// unit-test processes (the mode defaults to Gui when not explicitly set at
/// startup), so TUI-only commands are correctly excluded from that registry.
/// This test validates the static definition directly without relying on the
/// filtered registry.
#[test]
fn vim_mode_slash_command_is_registered_in_command_registry() {
    use torch::tui_export::SlashCommandSurfaces;

    let cmd = &slash_commands::VIM_MODE;
    assert_eq!(cmd.name, "/vim-mode");
    assert!(
        !cmd.description.is_empty(),
        "/vim-mode must have a non-empty description"
    );
    // The command must be TUI-only so it appears in the TUI's slash-command
    // menu but is excluded from the GUI surface.
    assert_eq!(
        cmd.supported_surfaces,
        SlashCommandSurfaces::TuiOnly,
        "/vim-mode must be registered as TUI-only"
    );
}

/// Executing the `/vim-mode` slash command must toggle and persist the
/// `AppEditorSettings::vim_mode` setting on each invocation.
#[test]
fn vim_mode_slash_command_persists_toggle() {
    App::test((), |mut app| async move {
        use torch::settings::AppEditorSettings;
        use torchui::SingletonEntity as _;

        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        // AppEditorSettings is not included in the standard TUI test fixture;
        // register it explicitly (mirrors the `enable_vim_mode` helper in
        // view_tests.rs).
        app.update(AppEditorSettings::register);

        assert!(
            !app.read(|ctx| AppEditorSettings::as_ref(ctx).vim_mode_enabled()),
            "vim mode should start disabled"
        );

        // First toggle: off → on.
        view.update(&mut app, |view, ctx| {
            view.execute_tui_slash_command(&slash_commands::VIM_MODE, None, ctx);
        });
        assert!(
            app.read(|ctx| AppEditorSettings::as_ref(ctx).vim_mode_enabled()),
            "/vim-mode should enable vim mode on the first toggle"
        );
        assert_eq!(
            view.read(&app, |view, _| {
                view.transient_hint
                    .current()
                    .map(|(text, _)| text.to_owned())
            }),
            Some(super::VIM_MODE_ENABLED_HINT.to_owned()),
            "should surface an enabled hint after enabling vim mode"
        );

        // Second toggle: on → off.
        view.update(&mut app, |view, ctx| {
            view.execute_tui_slash_command(&slash_commands::VIM_MODE, None, ctx);
        });
        assert!(
            !app.read(|ctx| AppEditorSettings::as_ref(ctx).vim_mode_enabled()),
            "/vim-mode should disable vim mode on the second toggle"
        );
        assert_eq!(
            view.read(&app, |view, _| {
                view.transient_hint
                    .current()
                    .map(|(text, _)| text.to_owned())
            }),
            Some(super::VIM_MODE_DISABLED_HINT.to_owned()),
            "should surface a disabled hint after disabling vim mode"
        );
    });
}

/// The Vim mode indicator (INS/NOR/VIS/V-L/REP) must appear in the footer only
/// while Vim mode is enabled.
///
/// This test validates the accessor and full render path. Notification
/// delivery is covered directly by the input-view mode-change event test.
#[test]
fn vim_mode_indicator_shown_only_when_vim_mode_is_enabled() {
    App::test((), |mut app| async move {
        use torch::settings::AppEditorSettings;
        use torchui::SingletonEntity as _;

        let fixture = focus_test_fixture(&mut app);
        let (view, _) = add_focus_test_session(&mut app, &fixture, true);
        app.update(AppEditorSettings::register);

        // Vim mode off: vim_mode_indicator returns None regardless of mode.
        app.read(|ctx| {
            let indicator = view.as_ref(ctx).vim_mode_indicator(ctx);
            assert!(
                indicator.is_none(),
                "indicator must be None when vim mode is disabled, got {indicator:?}"
            );
        });

        // Enable vim mode. The FSA starts in Insert mode, so the indicator
        // shows "INS", matching the GUI Vim status indicator.
        app.update(|ctx| {
            AppEditorSettings::handle(ctx).update(ctx, |settings, ctx| {
                settings
                    .vim_mode
                    .set_value(true, ctx)
                    .expect("failed to enable vim mode");
            });
        });
        app.read(|ctx| {
            let indicator = view.as_ref(ctx).vim_mode_indicator(ctx);
            assert_eq!(
                indicator,
                Some("INS"),
                "indicator must be INS in Insert mode when vim mode is enabled, got {indicator:?}"
            );
        });

        // Drive the input to Normal mode (Escape from Insert): indicator → Some("NOR").
        view.update(&mut app, |view, ctx| {
            view.input_view.update(ctx, |input, ctx| {
                // Process Escape to move Insert → Normal.
                input.handle_action(&crate::input::view::TuiInputAction::HandleEscape, ctx);
            });
        });
        // Verify via accessor that the mode state is correct.
        app.read(|ctx| {
            let indicator = view.as_ref(ctx).vim_mode_indicator(ctx);
            assert_eq!(
                indicator,
                Some("NOR"),
                "indicator must be NOR in Normal mode when vim mode is enabled"
            );
        });
        // Verify via the full render path: the footer must contain NOR.
        let rendered = render_session(&mut app, &view, 80, 24).join("\n");
        assert!(
            rendered.contains("NOR"),
            "rendered footer must contain 'NOR' after Insert\u{2192}Normal transition, got:\n{rendered}"
        );
        // Uppercase R enters continuous Replace mode and the footer reflects it.
        view.update(&mut app, |view, ctx| {
            view.input_view.update(ctx, |input, ctx| {
                input.handle_action(
                    &crate::input::view::TuiInputAction::Editor(
                        crate::editor_element::TuiEditorAction::InsertChar('R'),
                    ),
                    ctx,
                );
            });
        });
        app.read(|ctx| {
            assert_eq!(view.as_ref(ctx).vim_mode_indicator(ctx), Some("REP"));
        });
        let rendered = render_session(&mut app, &view, 80, 24).join("\n");
        assert!(
            rendered.contains("REP"),
            "rendered footer must contain 'REP' in continuous Replace mode, got:\n{rendered}"
        );

        // Disable vim mode: indicator → None again.
        app.update(|ctx| {
            AppEditorSettings::handle(ctx).update(ctx, |settings, ctx| {
                settings
                    .vim_mode
                    .set_value(false, ctx)
                    .expect("failed to disable vim mode");
            });
        });
        app.read(|ctx| {
            let indicator = view.as_ref(ctx).vim_mode_indicator(ctx);
            assert!(
                indicator.is_none(),
                "indicator must be None after vim mode is disabled, got {indicator:?}"
            );
        });
    });
}

#[test]
fn resume_shell_commands_use_shared_tui_launcher() {
    assert_eq!(
        super::tui_resume_shell_command(Channel::Local, "conversation-token"),
        "./script/run-tui -- --resume conversation-token"
    );
    assert_eq!(
        super::tui_resume_shell_command(Channel::Preview, "conversation-token"),
        "warp-preview --resume conversation-token"
    );
}
