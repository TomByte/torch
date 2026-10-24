use settings::macros::define_settings_group;
use settings::{RespectUserSyncSetting, SupportedPlatforms, SyncToCloud};

use super::DriveSortOrder;

pub const HAS_AUTO_OPENED_WELCOME_FOLDER: &str = "HasAutoOpenedWelcomeFolder";

define_settings_group!(WarpDriveSettings, settings: [
    sorting_choice: WarpDriveSortingChoice {
        type: DriveSortOrder,
        default: DriveSortOrder::ByObjectType,
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::Yes),
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "warp_drive.sorting_choice",
        description: "The sort order for items in Torch Drive.",
    },
    // Controls whether Torch Drive appears in the tools panel, command palette, and command search.
    enable_warp_drive: EnableWarpDrive {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::Yes),
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "warp_drive.enabled",
        description: "Whether Torch Drive is enabled.",
    },
]);

impl WarpDriveSettings {
    /// Returns whether Torch Drive is available for the current auth state.
    ///
    /// This is intentionally separate from the stored `enable_warp_drive`
    /// preference. Logged-out and anonymous users can retain their onboarding
    /// preference so Torch Drive appears automatically after signup, while the
    /// feature remains unavailable until then.
    pub fn is_warp_drive_available(app: &torchui::AppContext) -> bool {
        if torch_core::channel::ChannelState::is_local_only() {
            return false;
        }

        use torchui::SingletonEntity as _;
        !crate::auth::AuthStateProvider::as_ref(app)
            .get()
            .is_anonymous_or_logged_out()
    }
    /// Returns whether Torch Drive should be considered enabled.
    /// Returns `false` when the user is anonymous or fully logged out,
    /// regardless of the user setting.
    pub fn is_warp_drive_enabled(app: &torchui::AppContext) -> bool {
        use torchui::SingletonEntity as _;
        *Self::as_ref(app).enable_warp_drive && Self::is_warp_drive_available(app)
    }
}
