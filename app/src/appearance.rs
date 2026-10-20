use settings::Setting as _;
use warpui::fonts::FamilyId;
use warpui::{AddSingletonModel, AppContext, AssetProvider, Entity, ModelContext, SingletonEntity};

#[cfg(target_os = "macos")]
mod macos_app_icon {
    pub use crate::settings::app_icon::{AppIcon, AppIconSettings, AppIconSettingsChangedEvent};
}
use anyhow::anyhow;
#[cfg(target_os = "macos")]
use macos_app_icon::*;
pub use warp_core::ui::appearance::{Appearance, AppearanceEvent};

use crate::ASSETS;
use crate::settings::{
    FontSettings, FontSettingsChangedEvent, MonospaceFontSize, Settings, ThemeSettings,
    active_theme_kind,
};
use crate::themes::theme::{ThemeKind, WarpTheme};

/// Manages the state of the app-wide Appearance settings, it is responsible
/// for 1) listening to settings changes and update the underlying Appearance
/// accordingly 2) hold transient theme states that are used when user is switching
/// between temporary theme overrides in the theme picker.
pub struct AppearanceManager {
    // The transient theme is a theme that is set by the user but not saved
    // as a setting. It is used when the user is actively choosing a theme.
    transient_theme: Option<WarpTheme>,

    #[cfg(target_os = "macos")]
    app_icon_at_startup: AppIcon,
}

impl AppearanceManager {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        ctx.subscribe_to_model(&ThemeSettings::handle(ctx), move |me, _, _event, ctx| {
            me.refresh_theme_state(ctx);
        });

        #[cfg(target_os = "macos")]
        {
            ctx.subscribe_to_model(&AppIconSettings::handle(ctx), move |me, _, event, ctx| {
                match event {
                    AppIconSettingsChangedEvent::AppIconState { .. } => {}
                    AppIconSettingsChangedEvent::ShowDockIconState { .. } => {
                        me.apply_dock_icon_visibility(ctx);
                    }
                }
            });
        }

        ctx.subscribe_to_model(
            &FontSettings::handle(ctx),
            move |_, _, event, ctx| match event {
                FontSettingsChangedEvent::MonospaceFontName { .. } => {
                    let (font_name, match_fonts) = {
                        let settings = FontSettings::as_ref(ctx);
                        let font_name = settings.monospace_font_name.value().clone();
                        let match_fonts = settings.match_ai_font_to_terminal_font.value();
                        (font_name, *match_fonts)
                    };

                    if let Some(new_family) = get_or_load_font_family(&font_name, ctx) {
                        Appearance::handle(ctx).update(ctx, |appearance, ctx| {
                            appearance.set_monospace_font_family(new_family, ctx);
                            if match_fonts {
                                appearance.set_ai_font_family(new_family, ctx);
                            }
                        });
                    }
                }
                FontSettingsChangedEvent::MonospaceFontSize { .. } => {
                    let new_font_size = *FontSettings::as_ref(ctx).monospace_font_size.value();
                    Appearance::handle(ctx).update(ctx, |appearance, ctx| {
                        appearance.set_monospace_font_size(new_font_size, ctx)
                    });
                }

                FontSettingsChangedEvent::MonospaceFontWeight { .. } => {
                    let new_font_weight = *FontSettings::as_ref(ctx).monospace_font_weight.value();
                    Appearance::handle(ctx).update(ctx, |appearance, ctx| {
                        appearance.set_monospace_font_weight(new_font_weight, ctx)
                    });
                }
                FontSettingsChangedEvent::LineHeightRatio { .. } => {
                    let new_line_height_ratio =
                        *FontSettings::as_ref(ctx).line_height_ratio.value();

                    Appearance::handle(ctx).update(ctx, |appearance, ctx| {
                        appearance.set_line_height_ratio(new_line_height_ratio, ctx);
                    });
                }
                FontSettingsChangedEvent::AIFontName { .. } => {
                    let font_name = FontSettings::as_ref(ctx).ai_font_name.value().clone();
                    if let Some(new_family) = get_or_load_font_family(&font_name, ctx) {
                        Appearance::handle(ctx).update(ctx, |appearance, ctx| {
                            appearance.set_ai_font_family(new_family, ctx)
                        });
                    }
                }
                FontSettingsChangedEvent::MatchAIFontToTerminalFont { .. } => {
                    let settings = FontSettings::as_ref(ctx);
                    let match_ai_font_to_terminal_font =
                        *settings.match_ai_font_to_terminal_font.value();
                    if match_ai_font_to_terminal_font {
                        let font_name = settings.monospace_font_name.value().clone();

                        if let Some(new_family) = get_or_load_font_family(&font_name, ctx) {
                            Appearance::handle(ctx).update(ctx, |appearance, ctx| {
                                appearance.set_ai_font_family(new_family, ctx)
                            });
                        }
                    }
                }
                _ => {}
            },
        );

        Self {
            transient_theme: None,
            #[cfg(target_os = "macos")]
            app_icon_at_startup: *AppIconSettings::handle(ctx).as_ref(ctx).app_icon.value(),
        }
    }

    pub fn refresh_theme_state(&mut self, ctx: &mut ModelContext<Self>) {
        let new_theme = if let Some(transient_theme) = self.transient_theme.as_ref() {
            transient_theme.clone()
        } else {
            let theme_kind = active_theme_kind(ThemeSettings::as_ref(ctx), ctx);
            Settings::theme_for_theme_kind(&theme_kind, ctx)
        };

        #[cfg(target_family = "wasm")]
        emit_theme_background_event(&new_theme);

        Appearance::handle(ctx).update(ctx, |appearance, ctx| {
            appearance.set_theme(new_theme, ctx);
        })
    }

    pub fn set_transient_theme(&mut self, theme: ThemeKind, ctx: &mut ModelContext<Self>) {
        self.transient_theme = Some(Settings::theme_for_theme_kind(&theme, ctx));
        self.refresh_theme_state(ctx);
    }

    #[cfg(target_os = "macos")]
    pub fn app_icon_at_startup(&self) -> AppIcon {
        self.app_icon_at_startup
    }

    #[cfg(target_os = "macos")]
    pub fn apply_dock_icon_visibility(&self, app: &AppContext) {
        app.set_dock_icon_visible(*AppIconSettings::as_ref(app).show_dock_icon.value());
    }

    pub fn clear_transient_theme(&mut self, ctx: &mut ModelContext<Self>) {
        self.transient_theme = None;
        self.refresh_theme_state(ctx);
    }
}

impl Entity for AppearanceManager {
    type Event = ();
}

impl SingletonEntity for AppearanceManager {}

fn load_default_monospace_font_family(ctx: &mut AppContext) -> anyhow::Result<FamilyId> {
    warpui::fonts::Cache::handle(ctx).update(ctx, |font_cache, _| {
        let default_monospace_font_family = font_cache.load_family_from_bytes(
            "Hack",
            vec![
                ASSETS.get("bundled/fonts/hack/Hack-Italic.ttf")?.to_vec(),
                ASSETS.get("bundled/fonts/hack/Hack-Bold.ttf")?.to_vec(),
                ASSETS.get("bundled/fonts/hack/Hack-Regular.ttf")?.to_vec(),
                ASSETS
                    .get("bundled/fonts/hack/Hack-BoldItalic.ttf")?
                    .to_vec(),
            ],
        )?;
        let default_monospace_font =
            font_cache.select_font(default_monospace_font_family, Default::default());
        font_cache.glyph_typographic_bounds(
            default_monospace_font,
            MonospaceFontSize::default_value(),
            font_cache
                .glyph_for_char(default_monospace_font, 'm', false)
                .ok_or_else(|| anyhow!("monospace font has no 'm' glyph"))?
                .0,
        )?;
        Ok(default_monospace_font_family)
    })
}

fn load_default_ui_font_family(ctx: &mut AppContext) -> anyhow::Result<FamilyId> {
    warpui::fonts::Cache::handle(ctx).update(ctx, |font_cache, _| {
        let roboto = font_cache.load_family_from_bytes(
            "Roboto",
            vec![
                ASSETS
                    .get("bundled/fonts/roboto/Roboto-Italic.ttf")?
                    .to_vec(),
                ASSETS.get("bundled/fonts/roboto/Roboto-Bold.ttf")?.to_vec(),
                ASSETS
                    .get("bundled/fonts/roboto/Roboto-Regular.ttf")?
                    .to_vec(),
                ASSETS
                    .get("bundled/fonts/roboto/Roboto-Medium.ttf")?
                    .to_vec(),
                ASSETS
                    .get("bundled/fonts/roboto/RobotoFlex-Semibold.ttf")?
                    .to_vec(),
                ASSETS
                    .get("bundled/fonts/roboto/Roboto-BoldItalic.ttf")?
                    .to_vec(),
            ],
        );

        // On Windows, default to use Segoe UI as the UI font. This font is recommended by
        // Windows when rendering any UI text: https://learn.microsoft.com/en-us/windows/win32/uxguide/vis-fonts.
        // This font should be bundled with any modern version of Windows, if we can't load it for
        // any reason we fallback to using our normal bundled font.
        #[cfg(windows)]
        if let Ok(font_family_id) = font_cache.load_system_font("Segoe UI") {
            return Ok(font_family_id);
        }

        roboto
    })
}

fn load_password_font_family(ctx: &mut AppContext) -> anyhow::Result<FamilyId> {
    warpui::fonts::Cache::handle(ctx).update(ctx, |font_cache, _| {
        font_cache.load_family_from_bytes(
            "PasswordCircle",
            vec![ASSETS.get("bundled/fonts/password.ttf")?.to_vec()],
        )
    })
}

#[cfg(target_family = "wasm")]
/// On wasm we don't support loading fonts, so we just use the default.
fn get_or_load_font_family(_font_name: &str, _ctx: &mut AppContext) -> Option<FamilyId> {
    None
}

#[cfg(not(target_family = "wasm"))]
/// If we're running on a native platform (where we support font loading),
/// make sure we load the user's selected monospace font. We first check
/// the font cache in case we are using a pre-bundled font like Hack.
/// Then we fall back to loading a system font.
fn get_or_load_font_family(font_name: &str, ctx: &mut AppContext) -> Option<FamilyId> {
    warpui::fonts::Cache::handle(ctx).update(ctx, |font_cache, _| {
        match font_cache.get_or_load_system_font(font_name) {
            Ok(family) => {
                let font_id = font_cache.select_font(family, warpui::fonts::Properties::default());

                // Validate that the font contains the `m` glyph since this is assumed in
                // various parts of the code. We already do this when surfacing fonts in the font
                // selector dropdown, but a user could have edited their settings manually (or
                // there could have been a bug in the font selection logic) and we don't want to
                // crash in that case.
                let glyph_id = font_cache
                    .glyph_for_char(font_id, 'm', false /* include_fallback_fonts */);
                if glyph_id.is_none() {
                    log::warn!(
                        "Failed to load font: {font_name} because it didn't contain the character m"
                    );
                    return None;
                }
                Some(family)
            }
            Err(err) => {
                log::warn!("Failed to load font: {font_name} due to error {err:?}");
                // Just return the default if we can't load the font so we don't crash.
                None
            }
        }
    })
}

fn build_appearance(ctx: &mut AppContext) -> Appearance {
    let default_monospace_font_family = load_default_monospace_font_family(ctx)
        .expect("unable to load default monospace font family");
    let monospace_font_name = FontSettings::as_ref(ctx)
        .monospace_font_name
        .value()
        .clone();
    let am_font_name = FontSettings::as_ref(ctx).ai_font_name.value().clone();

    let monospace_font_family_from_settings = get_or_load_font_family(&monospace_font_name, ctx);

    let ui_font_family =
        load_default_ui_font_family(ctx).expect("unable to load default ui font family");

    let am_font_family_from_settings = get_or_load_font_family(&am_font_name, ctx);

    let password_font_family =
        load_password_font_family(ctx).expect("unable to load password font family");

    let monospace_font_size = *FontSettings::as_ref(ctx).monospace_font_size.value();

    let monospace_font_weight = *FontSettings::as_ref(ctx).monospace_font_weight.value();

    let line_height_ratio = *FontSettings::as_ref(ctx).line_height_ratio.value();

    let theme_kind = active_theme_kind(ThemeSettings::as_ref(ctx), ctx);
    let theme = Settings::theme_for_theme_kind(&theme_kind, ctx);
    #[cfg(target_family = "wasm")]
    emit_theme_background_event(&theme);

    Appearance::new(
        theme,
        monospace_font_family_from_settings.unwrap_or(default_monospace_font_family),
        monospace_font_size,
        monospace_font_weight,
        ui_font_family,
        line_height_ratio,
        am_font_family_from_settings.unwrap_or(default_monospace_font_family),
        password_font_family,
    )
}

#[cfg(target_family = "wasm")]
fn emit_theme_background_event(theme: &WarpTheme) {
    let bg = theme.background().into_solid();
    let color = format!("#{:02x}{:02x}{:02x}", bg.r, bg.g, bg.b);
    crate::platform::wasm::emit_event(crate::platform::wasm::WarpEvent::ThemeBackgroundChanged {
        color,
    });
}

pub fn register(app: &mut impl AddSingletonModel) {
    app.add_singleton_model(|ctx| build_appearance(ctx));
    app.add_singleton_model(AppearanceManager::new);
}
