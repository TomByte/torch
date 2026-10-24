use std::collections::HashSet;

use torch_core::settings::SupportedPlatforms;
use torch_core::settings::macros::define_settings_group;

use crate::banner::BannerState;
use crate::resource_center::Tip;

define_settings_group!(GeneralSettings, settings: [
    show_warning_before_quitting: ShowWarningBeforeQuitting {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::DESKTOP,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "general.show_warning_before_quitting",
        description: "Whether to show a warning dialog before quitting Torch.",
    },
    quit_on_last_window_closed: QuitOnLastWindowClosed {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::MAC,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "general.quit_on_last_window_closed",
        description: "Whether to quit Torch when the last window is closed.",
    },
    restore_session: RestoreSession {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::DESKTOP,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "general.restore_session",
        description: "Whether to restore the previous session when Torch starts up.",
    },
    add_app_as_login_item: LoginItem {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::OR(
            Box::new(SupportedPlatforms::MAC),
            Box::new(SupportedPlatforms::WINDOWS),
        ),
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "general.login_item",
        description: "Whether to launch Torch automatically when you log in.",
    },
    // Records whether the app has been added as a login item.
    // If it has, we don't try to add it again unless the user explicitly
    // retoggles the setting. This is to allow a user to remove the login item
    // directly from their OS's startup UI and not have it re-added when they
    // next start Torch.
    app_added_as_login_item: AppAddedAsLoginItem {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::OR(
            Box::new(SupportedPlatforms::MAC),
            Box::new(SupportedPlatforms::WINDOWS),
        ),
        surface: settings::SettingSurfaces::GUI,
        private: true,
    },
    link_tooltip: LinkTooltip {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "general.link_tooltip",
        description: "Whether to show a tooltip when hovering over links.",
    },
    welcome_tips_features_used: WelcomeTipsFeaturesUsed {
        type: HashSet<Tip>,
        default: HashSet::new(),
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    },
    welcome_tips_skipped_or_completed: WelcomeTipsCompleted {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    },
    telemetry_banner_dismissed: TelemetryBannerDismissed {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    },
    user_default_shell_unsupported_banner_state: UserDefaultShellUnsupportedBannerState {
        type: BannerState,
        default: BannerState::default(),
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    },
    did_non_anonymous_user_log_in: DidNonAnonymousUserLogIn {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    },
    // One-time flag tracking whether the OpenWarp launch modal has already been
    // shown to the user. Not user-visible; modeled as a setting so it's only
    // shown once per user regardless of the number of devices they use.
    did_check_to_trigger_openwarp_launch_modal: DidShowOpenWarpLaunchModal {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    },
    auto_open_code_review_pane_on_first_agent_change: AutoOpenCodeReviewPaneOnFirstAgentChange {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "code.editor.auto_open_code_review_pane_on_first_agent_change",
        description: "Whether to automatically open the code review pane when the agent makes its first change.",
    },
]);
