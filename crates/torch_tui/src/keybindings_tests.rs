use torchui_core::keymap::{Context, Trigger};
use torchui_core::{App, TuiView};

use super::{TUI_BINDING_GROUP, is_tui_owned};
use crate::input::TuiInputView;
use crate::input::view::INLINE_MENU_CAN_CLEAR_SELECTED_FLAG;

#[test]
fn tui_ownership_is_by_name_prefix_or_group() {
    // Editable TUI bindings are owned by name prefix.
    assert!(is_tui_owned("tui:input:submit", None));
    // Fixed TUI bindings have no name; they are owned by group.
    assert!(is_tui_owned("", Some(TUI_BINDING_GROUP)));

    // GUI bindings — named, unnamed, or grouped differently — are not.
    assert!(!is_tui_owned("terminal:cancel_command", None));
    assert!(!is_tui_owned("", None));
    assert!(!is_tui_owned("", Some("workspace")));
    assert!(!is_tui_owned("input:clear_screen", None));
}

/// Registering every TUI binding — including the orchestration card's
/// enter/ctrl-e/escape/ctrl-c and Tab/Left/Right navigation set — must satisfy the debug-time
/// cross-surface validators, which panic on any keystroke binding matching
/// a TUI view's context that is not TUI-owned.
#[test]
fn tui_binding_registration_passes_the_cross_surface_validators() {
    App::test((), |mut app| async move {
        app.update(super::init);
    });
}

#[test]
fn tui_binding_registration_passes_the_app_cross_platform_validator() {
    App::test((), |mut app| async move {
        app.update(|ctx| {
            ctx.set_default_binding_validator(torch::util::bindings::is_binding_cross_platform);
            super::init(ctx);
        });
    });
}

#[test]
fn input_cut_binding_yields_ctrl_x_to_contextual_menu_clear() {
    App::test((), |mut app| async move {
        app.update(super::init);
        app.read(|ctx| {
            let cut_bindings = ctx
                .editable_bindings()
                .filter(|binding| binding.name == "tui:input:cut")
                .collect::<Vec<_>>();
            assert_eq!(cut_bindings.len(), 2);
            let binding_for = |key: &str| {
                cut_bindings.iter().find(|binding| {
                    matches!(
                        binding.trigger,
                        Trigger::Keystrokes(keys)
                            if keys.first().is_some_and(|keystroke| keystroke.normalized() == key)
                    )
                })
            };
            let ctrl_x = binding_for("ctrl-x").expect("ctrl-x cut binding must be registered");
            let cmd_x = binding_for("cmd-x").expect("cmd-x cut binding must be registered");

            let mut plain_input = Context::default();
            plain_input.set.insert(TuiInputView::ui_name());
            assert!(ctrl_x.in_context(&plain_input));
            assert!(cmd_x.in_context(&plain_input));

            let mut menu_clear = plain_input;
            menu_clear.set.insert(INLINE_MENU_CAN_CLEAR_SELECTED_FLAG);
            assert!(!ctrl_x.in_context(&menu_clear));
            assert!(cmd_x.in_context(&menu_clear));
        });
    });
}
