use std::sync::Arc;

use torchui::{Action, TypedActionView, View, ViewContext, ViewHandle};

use crate::ui_components::icons::Icon;
use crate::view_components::action_button::{
    ActionButton, AdjoinedSide, ButtonSize, KeystrokeSource, NakedTheme, PrimaryRightBiasedTheme,
    PrimaryTheme,
};
use crate::view_components::compactible_action_button::CompactibleActionButton;

/// A split button composed of a primary CompactibleActionButton and a trailing
/// icon-only menu button (chevron-down). The menu button may be used as an anchor
/// for a dropdown Menu via `with_menu`.
#[derive(Clone)]
#[allow(dead_code)]
pub struct CompactibleSplitActionButton {
    primary_button: CompactibleActionButton,
    menu_button: ViewHandle<ActionButton>,
    save_position_id: Option<String>,
}

impl CompactibleSplitActionButton {
    /// Creates a split button: a primary CompactibleActionButton and an icon-only
    /// chevron menu button that inherits the same theme choice.
    #[allow(dead_code)]
    #[allow(clippy::too_many_arguments)]
    pub fn new<T, A>(
        label: String,
        keybinding: Option<KeystrokeSource>,
        size: ButtonSize,
        action: A,
        menu_action: A,
        compact_icon: Icon,
        use_primary_theme: bool,
        save_position_id: Option<String>,
        ctx: &mut ViewContext<'_, T>,
    ) -> Self
    where
        T: TypedActionView<Action = A> + View,
        A: Action + Clone + 'static,
    {
        let mut primary_button = CompactibleActionButton::new(
            label,
            keybinding,
            size,
            action,
            compact_icon,
            if use_primary_theme {
                Arc::new(PrimaryTheme)
            } else {
                Arc::new(NakedTheme)
            },
            ctx,
        );

        primary_button.set_adjoined_side(AdjoinedSide::Right, ctx);

        // The down-caret icon-only menu button.
        let menu_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new_with_boxed_theme(
                "",
                if use_primary_theme {
                    Arc::new(PrimaryRightBiasedTheme)
                } else {
                    Arc::new(NakedTheme)
                },
            )
            .with_size(size)
            .with_icon(Icon::ChevronDown)
            .with_adjoined_side(AdjoinedSide::Left)
            .on_click(move |ctx| ctx.dispatch_typed_action(menu_action.clone()))
        });

        Self {
            primary_button,
            menu_button,
            save_position_id,
        }
    }
}
