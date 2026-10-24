pub mod default_themes;
pub mod theme;
pub mod theme_chooser;
pub mod theme_creator;
pub mod theme_creator_body;
pub mod theme_creator_modal;
pub mod theme_deletion_body;
pub mod theme_deletion_modal;

use torch_core::ui::theme::WarpTheme;

/// Quiet fills for the terminal composer, restored history and command palette.
/// Keep custom themes driven by their own foreground/background colors.
pub fn command_surface(theme: &WarpTheme) -> torch_core::ui::theme::Fill {
    use torch_core::ui::color::blend::Blend;
    if torch_core::channel::ChannelState::is_local_only() {
        theme
            .background()
            .blend(&theme.foreground().with_opacity(2))
    } else {
        theme.surface_2()
    }
}

pub fn restored_command_overlay(theme: &WarpTheme) -> torch_core::ui::theme::Fill {
    if torch_core::channel::ChannelState::is_local_only() {
        theme.foreground().with_opacity(3)
    } else {
        theme.restored_blocks_overlay()
    }
}
