pub mod default_themes;
pub mod theme;
pub mod theme_chooser;
pub mod theme_creator;
pub mod theme_creator_body;
pub mod theme_creator_modal;
pub mod theme_deletion_body;
pub mod theme_deletion_modal;

use warp_core::ui::theme::WarpTheme;

pub fn onboarding_theme_picker_themes() -> [WarpTheme; 4] {
    if warp_core::channel::ChannelState::is_local_only() {
        return [
            default_themes::light_theme(),
            default_themes::dark_theme(),
            default_themes::kanagawa(),
            default_themes::ayu(),
        ];
    }
    [
        default_themes::phenomenon(),
        default_themes::dark_theme(),
        default_themes::light_theme(),
        default_themes::adeberry(),
    ]
}

/// Quiet fills for the terminal composer, restored history and command palette.
/// Keep custom themes driven by their own foreground/background colors.
pub fn command_surface(theme: &WarpTheme) -> warp_core::ui::theme::Fill {
    use warp_core::ui::color::blend::Blend;
    if warp_core::channel::ChannelState::is_local_only() {
        theme
            .background()
            .blend(&theme.foreground().with_opacity(2))
    } else {
        theme.surface_2()
    }
}

pub fn restored_command_overlay(theme: &WarpTheme) -> warp_core::ui::theme::Fill {
    if warp_core::channel::ChannelState::is_local_only() {
        theme.foreground().with_opacity(3)
    } else {
        theme.restored_blocks_overlay()
    }
}
