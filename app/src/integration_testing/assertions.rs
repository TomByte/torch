use torchui::integration::{AssertionCallback, TestStep};
use torchui::{async_assert, async_assert_eq};

use crate::integration_testing::view_getters::workspace_view;
use crate::util::bindings::keybinding_name_to_display_string;

pub fn assert_binding_display_string(
    binding: &'static str,
    display_string: Option<&'static str>,
) -> TestStep {
    TestStep::new("Assert a binding's display string").add_named_assertion(
        format!("Binding {binding} should have display string {display_string:?}"),
        move |app, _| {
            app.update(|ctx| {
                async_assert_eq!(
                    keybinding_name_to_display_string(binding, ctx).as_deref(),
                    display_string
                )
            })
        },
    )
}

pub fn assert_is_left_panel_open() -> AssertionCallback {
    Box::new(move |app, window_id| {
        let workspace = workspace_view(app, window_id);

        workspace.read(app, |workspace, ctx| {
            async_assert!(
                workspace.is_left_panel_open(ctx),
                "Expected left panel to be open, but it was closed"
            )
        })
    })
}
