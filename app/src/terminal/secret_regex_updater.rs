use torchui::{Entity, ModelContext, SingletonEntity};

use crate::server::telemetry::secret_redaction::update_telemetry_secrets_regex;
use crate::settings::{CustomSecretRegex, PrivacySettings, PrivacySettingsChangedEvent};
use crate::terminal::model::set_user_and_enterprise_secret_regexes;

/// Dummy singleton model that is used to update the current set of custom regexes within the
/// terminal model. We do this via a singleton model since we only want to do this once any time
/// the custom secret regex list changes, which must be done independent of any view.
pub struct CustomSecretRegexUpdater;

impl CustomSecretRegexUpdater {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let updater = CustomSecretRegexUpdater;
        // Initialize with current custom regexes (will be empty until safe mode is enabled)
        updater.update_custom_secret_regex_list(ctx);

        let privacy_settings = PrivacySettings::handle(ctx);
        ctx.subscribe_to_model(&privacy_settings, |me, _, evt, ctx| {
            if let PrivacySettingsChangedEvent::CustomSecretRegexList { .. } = evt {
                me.update_custom_secret_regex_list(ctx);
            }
        });
        updater
    }

    fn update_custom_secret_regex_list(&self, ctx: &mut ModelContext<Self>) {
        let privacy_settings = PrivacySettings::as_ref(ctx);

        let user_secrets = privacy_settings
            .user_secret_regex_list
            .iter()
            .map(CustomSecretRegex::pattern);

        set_user_and_enterprise_secret_regexes(user_secrets, std::iter::empty());

        // Also update the telemetry-side secret regex, which is independent of
        // the user's safe-mode setting and always includes the default patterns.
        let user_secrets = privacy_settings
            .user_secret_regex_list
            .iter()
            .map(CustomSecretRegex::pattern);

        update_telemetry_secrets_regex(user_secrets, std::iter::empty());
    }
}

impl Entity for CustomSecretRegexUpdater {
    type Event = ();
}

impl SingletonEntity for CustomSecretRegexUpdater {}
