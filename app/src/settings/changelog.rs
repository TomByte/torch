use settings::macros::define_settings_group;
use settings::{SupportedPlatforms};

define_settings_group!(ChangelogSettings, settings: [
   show_changelog_after_update: ShowChangelogAfterUpdate {
       type: bool,
       default: true,
       supported_platforms: SupportedPlatforms::ALL,
       surface: settings::SettingSurfaces::GUI,
       private: false,
       toml_path: "general.show_changelog_after_update",
       description: "Whether the changelog is shown after an update.",
   },
]);
