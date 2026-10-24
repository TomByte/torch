use std::sync::Arc;

use torch_core::features::FeatureFlag;
use torch_core::settings::Setting;
use torch_errors::report_if_error;
use torchui::{Entity, ModelContext, SingletonEntity};

use crate::auth::auth_state::AuthState;
use crate::settings::input::InputBoxType;
use crate::settings::{FontSettings, InputSettings, PrivacySettings, ThemeSettings};
use crate::terminal::session_settings::SessionSettings;
use crate::themes::theme::ThemeKind;

pub struct SettingsInitializer;

impl Default for SettingsInitializer {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsInitializer {
    pub fn new() -> Self {
        Self
    }

    /// A hook for changing settings values after a user is fetched from the server.
    ///
    /// Specifically useful for adjusting settings for first-time users when the default value of a
    /// setting as set in define_settings_group! is no longer the desired default value,
    /// but we don't want to change it for existing users (which is what would happen if we changed the
    /// default value in define_settings_group! in code).
    pub fn handle_user_fetched(&self, auth_state: Arc<AuthState>, ctx: &mut ModelContext<Self>) {
        /// We use a font-size of 16px (12pt) on Windows to more closely match the default font size of
        /// Windows terminal.
        const DEFAULT_WINDOWS_MONOSPACE_FONT_SIZE: f32 = 16.;

        if auth_state.is_onboarded() == Some(false) {
            PrivacySettings::handle(ctx).update(ctx, |settings, ctx| {
                // Previously, secret redaction had a built-in default set of regexes that users couldn't change.
                // We want to add that default list to all existing users' lists, so we don't regress their current secret redaction experience.
                // However, for new users, we don't want to add these defaults without their explicit action, so we disable adding them here.
                settings.disable_default_regex_trigger(ctx);
            });

            if FeatureFlag::DefaultAdeberryTheme.is_enabled() {
                log::debug!("Setting default theme to Adeberry for new user");
                ThemeSettings::handle(ctx).update(ctx, |settings, ctx| {
                    if *settings.theme_kind.value() == ThemeKind::Phenomenon {
                        report_if_error!(settings.theme_kind.set_value(ThemeKind::Adeberry, ctx));
                    }
                });
            }

            if cfg!(windows) {
                log::debug!("Setting default font size to 16px (12pt) for a new Windows user");
                FontSettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(
                        settings
                            .monospace_font_size
                            .set_value(DEFAULT_WINDOWS_MONOSPACE_FONT_SIZE, ctx)
                    );
                })
            }

            let did_update_input_type = InputSettings::handle(ctx).update(ctx, |settings, ctx| {
                if !settings.input_box_type.is_value_explicitly_set()
                    && *settings.input_box_type.value() == InputBoxType::Classic
                {
                    log::debug!("Setting default input type to Torch prompt for new user");
                    report_if_error!(
                        settings
                            .input_box_type
                            .set_value(InputBoxType::Universal, ctx)
                    );
                    ctx.notify();
                    return true;
                }
                false
            });
            // Keep honor_ps1 in sync: Universal input requires honor_ps1 = false.
            if did_update_input_type {
                SessionSettings::handle(ctx).update(ctx, |settings, ctx| {
                    if *settings.honor_ps1.value() {
                        report_if_error!(settings.honor_ps1.set_value(false, ctx));
                    }
                });
            }
        }
    }
}

impl Entity for SettingsInitializer {
    type Event = ();
}

/// Mark CloudPreferencesSyncer as global application state.
impl SingletonEntity for SettingsInitializer {}
