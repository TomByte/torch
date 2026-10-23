use torchui::App;

use super::*;
use crate::settings::{SystemThemes, Theme, UseSystemTheme};

#[test]
fn retired_preferences_keep_the_correct_theme_when_opening_the_picker() {
    if !torch_core::channel::ChannelState::is_local_only() {
        return;
    }
    for system in [false, true] {
        App::test((), move |mut app| async move {
            app.add_singleton_model(move |_| ThemeSettings {
                theme_kind: Theme::new(Some(ThemeKind::SolarizedLight)),
                use_system_theme: UseSystemTheme::new(Some(system)),
                selected_system_themes: SystemThemes::new(Some(SelectedSystemThemes {
                    light: ThemeKind::GruvboxLight,
                    dark: ThemeKind::Dracula,
                })),
            });
            app.update(move |ctx| {
                assert_eq!(
                    ThemeChooserMode::SystemAgnostic.into_theme_kind(ctx),
                    ThemeKind::Light
                );
                if system {
                    assert_eq!(
                        ThemeChooserMode::SystemLight.into_theme_kind(ctx),
                        ThemeKind::Light
                    );
                    assert_eq!(
                        ThemeChooserMode::SystemDark.into_theme_kind(ctx),
                        ThemeKind::Dark
                    );
                }
            });
        });
    }
}
