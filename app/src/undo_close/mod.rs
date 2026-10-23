pub mod settings;
mod stack;

use torchui::AppContext;
use torchui::keymap::EditableBinding;

pub use self::settings::UndoCloseSettings;
pub use self::stack::{UndoCloseStack, UndoCloseStackEvent};
use crate::util::bindings::CustomAction;
use crate::workspace::WorkspaceAction;

/// Register keybindings for undo close functionality.
pub fn init(ctx: &mut AppContext) {
    use torchui::keymap::macros::*;

    ctx.register_editable_bindings([EditableBinding::new(
        "app:reopen_closed_session",
        "Reopen closed session",
        // Trigger ReopenClosedSession on the active workspace when
        // the action is taken from the command palette.
        WorkspaceAction::ReopenClosedSession,
    )
    .with_custom_action(CustomAction::ReopenClosedSession)
    // Scope to the `Workspace` context (mirrors the sibling `workspace:*` bindings).
    .with_context_predicate(id!("Workspace"))]);
}
