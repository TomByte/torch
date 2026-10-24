#![allow(clippy::doc_lazy_continuation)]

mod agent_status;
//mod ai;
mod alloc;
mod antivirus;
#[cfg(target_os = "macos")]
mod app_menus;
mod app_services;
mod app_state;
mod auth;
mod autoupdate;
mod banner;
mod changelog_model;
mod chip_configurator;
mod cloud_object;
mod code;
mod code_review;
mod coding_panel_enablement_state;
mod command_palette;
mod completer;
#[allow(dead_code)]
mod context_chips;
#[cfg(enable_crash_recovery)]
mod crash_recovery;
mod debug_dump;
mod default_terminal;
mod drive;
#[cfg(windows)]
mod dynamic_libraries;
mod env_vars;
mod experiments;
mod external_secrets;
#[cfg(target_family = "wasm")]
mod font_fallback;
mod global_resource_handles;
mod gpu_state;
mod interval_timer;
#[cfg(feature = "local_fs")]
mod local_control;
pub mod local_hooks;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod login_item;
mod menu;
mod modal;
mod network;
mod notebooks;
mod notification;
mod palette;
mod persisted_workspace;
mod persistence;
mod platform;
mod prefix;
#[cfg(target_os = "macos")]
mod preview_config_migration;
mod profiling;
mod projects;
mod prompt;
mod quit_warning;
#[allow(dead_code)]
mod remote_server;
mod resource_limits;
mod safe_triangle;
mod search_bar;
mod server;
mod session_management;
mod shell_indicator;
mod suggestions;
mod system;
mod tab;
#[cfg(test)]
mod test_util;
mod throttle;
mod tips;
mod ui_components;
mod undo_close;
mod uri;
mod user_config;
pub mod util;
mod view_components;
mod vim_registers;
mod voltron;
mod warp_managed_paths_watcher;
#[cfg(target_family = "wasm")]
mod wasm_nux_dialog;
mod window_settings;
mod workspaces;

// PLEASE DO NOT ADD MORE PUBLIC MODULES!
//
// Any modules which we make public outside of the `warp` crate lose dead code
// checking support, as the compiler cannot make any assumptions about whether
// or not the function/type is used by another crate that pulls in this one as
// a dependency.
//
// If you feel the need to export a module so that a type or function within it
// can be used by an integration test, you should define a new assertion function
// in the torch::integration_testing::assertions module (or a sub-module).  These
// functions will allow us to keep types internal to this crate and expose a
// simpler API for integration tests to consume.
pub mod appearance;
pub mod channel;
pub mod editor;
pub mod features;
pub mod input_suggestions;
#[cfg(feature = "integration_tests")]
pub mod integration_testing;
pub mod keyboard;
pub mod launch_configs;
pub mod pane_group;
pub mod resource_center;
pub mod root_view;
pub mod search;
pub mod settings;
pub mod settings_view;
pub mod tab_configs;
pub mod terminal;
pub mod themes;
use agent_status::AgentNotificationsModel;
use auth::auth_manager::AuthManager;
use auth::auth_state::{AuthState, AuthStateProvider};
use code::editor_management::CodeManager;
use code::opened_files::OpenedFilesModel;
use code_review::GlobalCodeReviewModel;
use code_review::git_repo_model::GitRepoModels;
use quit_warning::UnsavedStateSummary;
#[cfg(feature = "local_fs")]
use repo_metadata::{
    RepoMetadataModel, repositories::DetectedRepositories, watcher::DirectoryWatcher,
};
use server::network_log_pane_manager::NetworkLogPaneManager;
use server::telemetry::context_provider::AppTelemetryContextProvider;
#[cfg(feature = "local_fs")]
use settings::import::model::ImportedConfigModel;
use settings_view::pane_manager::SettingsPaneManager;
use terminal::general_settings::GeneralSettings;
use terminal::keys_settings::KeysSettings;
#[cfg(all(not(target_family = "wasm"), feature = "local_tty"))]
use terminal::local_shell::LocalShellState;
pub use util::bindings::cmd_or_ctrl_shift;
#[cfg(feature = "local_fs")]
use watcher::HomeDirectoryWatcher;

use crate::persisted_workspace::PersistedWorkspace;
use crate::uri::web_intent_parser::maybe_rewrite_web_url_to_intent;
use crate::view_components::DismissibleToast;
pub mod workflows;
pub mod workspace;

use std::borrow::Cow;
use std::ops::Deref;
use std::sync::Arc;

use ::settings::{Setting, ToggleableSetting};
#[cfg(feature = "local_tty")]
use anyhow::Context;
use anyhow::{Result, anyhow};
use appearance::{Appearance, AppearanceManager};
use channel::ChannelState;
use interval_timer::IntervalTimer;
use itertools::Itertools;
#[cfg(feature = "integration_tests")]
pub use persistence::testing as sqlite_testing;
use server::server_api::ServerApiProvider;
use settings::{ExtraMetaKeys, PrivacySettings};
use terminal::input;
use terminal::session_settings::SessionSettings;
// Re-export the debounce function to simplify imports.
pub use torch_core::r#async::debounce;
use torch_core::execution_mode::{AppExecutionMode, ExecutionMode};
// Re-export the send_telemetry_from_ctx macro at the crate root level
pub use torch_core::send_telemetry_from_app_ctx;
pub use torch_core::send_telemetry_from_ctx;
// Re-export the safe logging macros at the crate root level for backwards compatibility
pub use torch_core::{safe_debug, safe_error, safe_info, safe_warn};
use torch_errors::{report_error, report_if_error};
#[cfg(feature = "local_fs")]
use torch_files::FileModel;
use torch_logging::{LogDestination, LogFrontend};
use torch_server_client::iap::{IapManager, IapManagerEvent, IapState};
use torch_server_client::network_logging::NetworkLogModel;
use torchui::integration::TestDriver;
use torchui::platform::TerminationMode;
use torchui::platform::app::{ApproveTerminateResult, TerminationRequestSource};
use torchui::windowing::state::ApplicationStage;
use torchui::{AppContext, Event, SingletonEntity, WindowId};
use url::Url;
use window_settings::WindowSettings;
use workflows::manager::WorkflowManager;
use workspace::sync_inputs::SyncedInputState;

use self::features::FeatureFlag;
#[cfg(not(target_family = "wasm"))]
use crate::antivirus::AntivirusInfo;
use crate::app_state::AppState;
use crate::autoupdate::{AutoupdateState, RelaunchModel};
use crate::changelog_model::ChangelogModel;
use crate::cloud_object::model::actions::{ObjectAction, ObjectActions};
use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::model::view::CloudViewModel;
use crate::code::global_buffer_model::GlobalBufferModel;
#[cfg(feature = "local_fs")]
use crate::code::language_server_shutdown_manager::LanguageServerShutdownManager;
use crate::context_chips::prompt::Prompt;
use crate::default_terminal::DefaultTerminal;
use crate::drive::CloudObjectTypeAndId;
use crate::env_vars::manager::EnvVarCollectionManager;
use crate::experiments::ImprovedPaletteSearch;
pub use crate::global_resource_handles::{GlobalResourceHandles, GlobalResourceHandlesProvider};
use crate::gpu_state::GPUState;
use crate::network::NetworkStatus;
use crate::notebooks::editor::keys::NotebookKeybindings;
use crate::notebooks::manager::NotebookManager;
use crate::notification::NotificationContext;
use crate::palette::PaletteMode;
use crate::persistence::PersistenceWriter;
use crate::projects::ProjectManagementModel;
use crate::root_view::{
    OpenFromRestoredArg, OpenPath, quake_mode_window_id, quake_mode_window_is_open,
};
use crate::server::cloud_objects::listener::Listener;
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::server::experiments::ServerExperiments;
#[cfg(not(target_family = "wasm"))]
use crate::server::server_api::managed_secrets::AppManagedSecretManager as ManagedSecretManager;
use crate::server::sync_queue::{QueueItem, SyncQueue};
pub use crate::server::telemetry::{
    AgentModeEntrypoint, AgentModeEntrypointSelectionType, TelemetryEvent,
};
use crate::server::telemetry::{AppStartupInfo, CloseTarget, PaletteSource};
use crate::session_management::{RunningSessionSummary, SessionNavigationData};
use crate::settings::cloud_preferences_syncer::{
    CloudPreferencesSyncerEvent, initialize_cloud_preferences_syncer,
};
use crate::settings::manager::SettingsManager;
use crate::settings::{AccessibilitySettings, ScrollSettings, SelectionSettings};
use crate::settings_view::DisplayCount;
use crate::settings_view::keybindings::KeybindingChangedNotifier;
use crate::suggestions::ignored_suggestions_model::IgnoredSuggestionsModel;
use crate::system::SystemStats;
use crate::tab::TabShortcutModifierState;
use crate::terminal::cli_agent_sessions::CLIAgentSessionsModel;
use crate::terminal::resizable_data::ResizableData;
use crate::terminal::{AudibleBell, CustomSecretRegexUpdater, History};
use crate::undo_close::UndoCloseStack;
use crate::user_config::WarpConfig;
use crate::util::bindings::is_binding_cross_platform;
use crate::vim_registers::VimRegisters;
use crate::warp_managed_paths_watcher::{WarpManagedPathsWatcher, ensure_warp_watch_roots_exist};
use crate::workflows::aliases::WorkflowAliases;
use crate::workflows::local_workflows::LocalWorkflows;
use crate::workspace::{ActiveSession, PaneViewLocator, ToastStack, Workspace, WorkspaceAction};
use crate::workspaces::team_tester::TeamTesterStatus;
use crate::workspaces::update_manager::TeamUpdateManager;
use crate::workspaces::user_profiles::UserProfiles;
use crate::workspaces::user_workspaces::UserWorkspaces;

/// Our embedded application assets.
pub static ASSETS: torch_assets::Assets = torch_assets::Assets;

/// Launch mode for how to start up Torch.
#[allow(clippy::large_enum_variant)]
pub(crate) enum LaunchMode {
    /// Run the regular GUI application.
    App { args: torch_cli::AppArgs },

    /// Run a test - this may be an integration test or an eval.
    Test {
        driver: Box<Option<TestDriver>>,
        is_integration_test: bool,
    },

    /// Remote server proxy — bridges SSH stdio to the daemon's Unix socket.
    /// This is a short-lived process that runs for the lifetime of an SSH session.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    RemoteServerProxy,

    /// Remote server daemon — long-lived headless process serving remote
    /// connections via a Unix domain socket.
    #[cfg_attr(not(unix), allow(dead_code))]
    RemoteServerDaemon {
        /// Stable identity key used to partition the daemon's socket/PID
        /// directory on the remote host.
        identity_key: String,
    },
}

impl LaunchMode {
    fn args(&self) -> Cow<'_, torch_cli::AppArgs> {
        match self {
            LaunchMode::App { args, .. } => Cow::Borrowed(args),
            LaunchMode::Test { .. }
            | LaunchMode::RemoteServerProxy
            | LaunchMode::RemoteServerDaemon { .. } => Cow::Owned(torch_cli::AppArgs::default()),
        }
    }

    /// Returns `true` if this process is running an integration test.
    fn is_integration_test(&self) -> bool {
        match self {
            LaunchMode::Test {
                is_integration_test,
                ..
            } => *is_integration_test,
            LaunchMode::App { .. }
            | LaunchMode::RemoteServerProxy
            | LaunchMode::RemoteServerDaemon { .. } => false,
        }
    }

    fn take_test_driver(&mut self) -> Option<TestDriver> {
        match self {
            LaunchMode::Test { driver, .. } => driver.take(),
            LaunchMode::App { .. }
            | LaunchMode::RemoteServerProxy
            | LaunchMode::RemoteServerDaemon { .. } => None,
        }
    }

    /// Add an URL to open. Only supported for [`LaunchMode::App`]
    #[allow(dead_code)]
    fn add_url(&mut self, url: Url) {
        if let LaunchMode::App { args, .. } = self {
            args.urls.push(url);
        }
    }

    fn execution_mode(&self) -> ExecutionMode {
        match self {
            LaunchMode::App { .. } => ExecutionMode::App,
            LaunchMode::Test { .. } => ExecutionMode::App,
            // RemoteServerProxy is a thin byte bridge; Sdk is the closest match.
            LaunchMode::RemoteServerProxy => ExecutionMode::Sdk,
            // RemoteServerDaemon gets its own mode for distinct Sentry tagging.
            LaunchMode::RemoteServerDaemon { .. } => ExecutionMode::RemoteServerDaemon,
        }
    }

    /// Returns `true` if Torch renders to native GUI windows on the platform app backend.
    fn is_gui(&self) -> bool {
        match self {
            LaunchMode::App { .. } | LaunchMode::Test { .. } => true,
            LaunchMode::RemoteServerProxy | LaunchMode::RemoteServerDaemon { .. } => false,
        }
    }

    /// Returns `true` if Torch runs with no user interface at all.
    fn is_headless(&self) -> bool {
        match self {
            LaunchMode::RemoteServerProxy | LaunchMode::RemoteServerDaemon { .. } => true,
            LaunchMode::App { .. } | LaunchMode::Test { .. } => false,
        }
    }

    /// Whether this launch mode should start the local loopback HTTP server
    /// (`crates/http_server`), which serves app-installation detection and profiling on a
    /// fixed port. Only GUI instances start it, since co-located windowless processes (daemon,
    /// CLI, proxy) would otherwise contend for the fixed port.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    fn should_start_local_http_server(&self) -> bool {
        self.is_gui()
    }

    /// Whether or not to start a crash recovery process (on platforms that support it).
    #[cfg(enable_crash_recovery)]
    pub(crate) fn crash_recovery_enabled(&self) -> bool {
        match self {
            LaunchMode::App { .. } => true,
            LaunchMode::Test { .. }
            | LaunchMode::RemoteServerProxy
            | LaunchMode::RemoteServerDaemon { .. } => false,
        }
    }

    /// Whether profiling and tracing should be initialized.
    pub(crate) fn needs_profiling(&self) -> bool {
        match self {
            LaunchMode::App { .. }
            | LaunchMode::Test { .. }
            | LaunchMode::RemoteServerDaemon { .. }
            | LaunchMode::RemoteServerProxy => true,
        }
    }

    /// Log destination for this mode.
    fn log_destination(&self) -> Option<LogDestination> {
        match self {
            // Proxy must log to stderr because stdout is the protocol channel.
            LaunchMode::RemoteServerProxy => Some(LogDestination::Stderr),
            LaunchMode::RemoteServerDaemon { .. } => Some(LogDestination::File),
            LaunchMode::App { .. } | LaunchMode::Test { .. } => None,
        }
    }

    fn log_frontend(&self) -> LogFrontend {
        match self {
            LaunchMode::App { .. } | LaunchMode::Test { .. } => LogFrontend::Gui,
            LaunchMode::RemoteServerProxy | LaunchMode::RemoteServerDaemon { .. } => {
                LogFrontend::Cli
            }
        }
    }

    fn as_str_for_tracing(&self) -> &'static str {
        match self {
            LaunchMode::App { .. } => "app",
            LaunchMode::Test { .. } => "test",
            LaunchMode::RemoteServerDaemon { .. } => "remote_server_daemon",
            LaunchMode::RemoteServerProxy => "remote_server_proxy",
        }
    }
}

/// If the given event is a key down event containing alt modifiers, and those
/// alt modifiers should be treated as meta keys, then remove the alts and
/// prefix the keys with an escape. See WAR-472.
fn apply_extra_meta_keys(event: &mut Event, extra_metas: ExtraMetaKeys) {
    if let Event::KeyDown {
        keystroke, details, ..
    } = event
    {
        let left_as_meta = extra_metas.left_alt && details.left_alt;
        let right_as_meta = extra_metas.right_alt && details.right_alt;
        if left_as_meta || right_as_meta {
            let side = match (left_as_meta, right_as_meta) {
                (true, true) => "left+right alt",
                (true, false) => "left alt",
                (false, true) => "right alt",
                (false, false) => unreachable!(),
            };
            log::info!("Treating {side} as meta");
            keystroke.alt = false;
            keystroke.meta = true;
        }
    }
}

fn apply_scroll_multiplier(event: &mut Event, app: &AppContext) {
    if let Event::ScrollWheel { delta, precise, .. } = event
        && !*precise
    {
        let scroll_multiplier = *ScrollSettings::as_ref(app).mouse_scroll_multiplier.value();
        *delta *= scroll_multiplier;
    }
}

/// Runs the shared Torch executable as the app or as one of its command-line modes.
///
/// The bundled Torch Control wrapper injects `--warpctrl`, which is dispatched
/// before the normal Torch parser.
#[::tracing::instrument(skip_all, fields(tags.cloud_agent = true))]
pub fn run() -> Result<()> {
    // Perform any necessary platform-specific initialization.
    platform::init();

    // Ensure feature flags are initialized before parsing command-line arguments.
    features::init_feature_flags();
    if let Some(args) = torch_cli::local_control::ControlArgs::from_control_mode_env() {
        #[cfg(windows)]
        torch_util::windows::attach_to_parent_console();
        torch_cli::local_control::run_and_exit(args);
    }

    // Parse command-line arguments.
    let args = torch_cli::Args::from_env();

    // Server URL overrides are only honored on internal dev channels. Release channels silently
    // ignore `--server-root-url` / `--ws-server-url` (and their
    // `TORCH_*` env-var equivalents) so shipped builds can't be redirected away from their
    // baked-in server URLs. See `Channel::allows_server_url_overrides`.
    if ChannelState::channel().allows_server_url_overrides() {
        if let Some(url) = args.server_root_url()
            && let Err(e) = ChannelState::override_server_root_url(url.to_owned())
        {
            eprintln!("Error: Invalid server root URL: {e:#}");
        }

        if let Some(url) = args.ws_server_url()
            && let Err(e) = ChannelState::override_ws_server_url(url.to_owned())
        {
            eprintln!("Error: Invalid websocket server URL: {e:#}");
        }
    }

    if let Some(command) = args.command() {
        #[cfg(windows)]
        if command.prints_to_stdout() {
            // We attach a console to ensure that all standard output gets printed correctly.
            torch_util::windows::attach_to_parent_console();
        }
        match command {
            torch_cli::Command::Worker(worker) => return run_worker_command(worker),
            torch_cli::Command::Completions { shell } => {
                return torch_cli::completions::generate_to_stdout(*shell);
            }
            torch_cli::Command::DumpDebugInfo => {
                return debug_dump::run();
            }
            #[cfg(not(target_family = "wasm"))]
            torch_cli::Command::DumpSettingsSchema { output_path } => {
                return settings::schema_generation::dump_settings_schema(output_path.as_deref());
            }
            #[cfg(not(target_family = "wasm"))]
            torch_cli::Command::PrintTelemetryEvents => {
                return TelemetryEvent::print_telemetry_events_json();
            }
        }
    }

    run_internal(LaunchMode::App {
        args: args.into_app_args(),
    })
}

/// Runs a parsed Torch worker command.
fn run_worker_command(worker: &torch_cli::WorkerCommand) -> Result<()> {
    match worker {
        #[cfg(all(feature = "local_tty", unix))]
        torch_cli::WorkerCommand::TerminalServer(args) => {
            crate::terminal::local_tty::run_terminal_server(args);
            Ok(())
        }
        #[cfg(not(target_family = "wasm"))]
        torch_cli::WorkerCommand::RemoteServerProxy(args) => {
            // Proxy is a thin byte bridge (stdin/stdout ↔ Unix socket).
            // It only needs logging to stderr since stdout is the protocol
            // channel. No crash reporting, no initialize_app.
            let launch_mode = LaunchMode::RemoteServerProxy;
            torch_logging::init(torch_logging::LogConfig {
                frontend: launch_mode.log_frontend(),
                log_destination: launch_mode.log_destination(),
                ..Default::default()
            })?;
            crate::remote_server::run_proxy(args.identity_key.clone())
        }
        #[cfg(not(target_family = "wasm"))]
        torch_cli::WorkerCommand::RemoteServerDaemon(args) => {
            // Daemon handles its own full initialization (including
            // initialize_app and crash reporting) inside run_daemon_app.
            crate::remote_server::run_daemon(args.identity_key.clone())
        }
        #[cfg(not(target_family = "wasm"))]
        torch_cli::WorkerCommand::RipgrepSearch {
            parent,
            ignore_case,
            multiline,
            pattern,
            paths,
        } => {
            torch_ripgrep::search::run_search_subprocess(
                std::slice::from_ref(pattern),
                paths.clone(),
                *ignore_case,
                *multiline,
                parent.pid,
            )
            .map_err(|err| anyhow!(err.to_string()))?;
            Ok(())
        }
        #[cfg(all(target_family = "wasm", not(feature = "local_tty")))]
        worker => {
            // On wasm, specifically, we should fail spectacularly if we get here.
            #[cfg(target_family = "wasm")]
            panic!("Worker process not supported on WASM: {worker:?}")
        }
    }
}

/// Runs an integration test using the provided test driver.
pub fn run_integration_test(driver: TestDriver) -> Result<()> {
    let is_integration_test = std::env::var("TORCH_INTEGRATION").is_ok();
    let launch = LaunchMode::Test {
        driver: Box::new(Some(driver)),
        is_integration_test,
    };
    run_internal(launch)
}

/// Runs the app (or CLI / daemon).
fn run_internal(mut launch_mode: LaunchMode) -> Result<()> {
    let mut timer = IntervalTimer::new();

    // ── Early initialization (pre-AppBuilder) ──────────────────────
    // These steps run before the platform event loop is started.
    // They must not depend on AppContext.

    #[cfg(windows)]
    dynamic_libraries::configure_library_loading();

    if launch_mode.needs_profiling() {
        profiling::init();
    }

    // The `run` function already initializes feature flags, but ensure they're initialized here
    // for other entrypoints.
    features::init_feature_flags();

    let span = tracing::info_span!(
        "run_internal",
        launch_mode = launch_mode.as_str_for_tracing()
    );
    let _enter = span.enter();

    let log_destination = launch_mode.log_destination();

    cfg_if::cfg_if! {
        if #[cfg(enable_crash_recovery)] {
            if crash_recovery::is_crash_recovery_process(launch_mode.args().as_ref()) {
                torch_logging::init_for_crash_recovery_process()?;
            } else {
                torch_logging::init(torch_logging::LogConfig {
                    frontend: launch_mode.log_frontend(),
                    log_destination,
                    ..Default::default()
                })?;
            }
        } else {
            torch_logging::init(torch_logging::LogConfig {
                frontend: launch_mode.log_frontend(),
                log_destination,
                ..Default::default()
            })?;
        }
    }

    timer.mark_interval_end("LOG_FILE_SETUP_COMPLETE");

    // Claim a background-only process type before anything else can reach
    // AppKit, so a windowless launch never acquires a Dock tile. See APP-2946.
    #[cfg(target_os = "macos")]
    if !launch_mode.is_gui()
        && let Err(e) = platform::mac::mark_process_as_background_only()
    {
        log::warn!("Failed to mark process as background-only: {e:#}");
    }

    #[cfg(windows)]
    platform::windows::check_redirection_guard();

    // Adjust resource limits early, before doing other work, to ensure that
    // any children we spawn (like the terminal server) inherit our adjusted
    // rlimits.
    resource_limits::adjust_resource_limits();

    // For wasm builds we have this special case to parse out the intent
    // from the url that is used to visite the app on web.
    #[cfg(target_family = "wasm")]
    {
        use uri::web_intent_parser;
        if let Some(intent) = web_intent_parser::parse_web_intent_from_current_url() {
            launch_mode.add_url(intent);
        }
        web_intent_parser::set_context_flags_from_current_url();
    }

    #[cfg(all(
        feature = "release_bundle",
        any(target_os = "linux", target_os = "freebsd")
    ))]
    if let LaunchMode::App { .. } = launch_mode {
        match app_services::linux::pass_startup_args_to_existing_instance(
            launch_mode.args().as_ref(),
        ) {
            // If we were able to contact an existing application instance, quit -
            // we only want to run a single instance of Torch at a time.
            Ok(_) => std::process::exit(0),
            // If Torch isn't already running, we're good to go.
            Err(app_services::linux::StartupArgsForwardingError::NoExistingInstance) => {}
            // If we just finished an auto-update, we should continue running.
            Err(app_services::linux::StartupArgsForwardingError::IgnoredAfterAutoUpdate) => {}
            Err(
                app_services::linux::StartupArgsForwardingError::IgnoredForCrashRecoveryProcess,
            ) => {}
            // If we were unable to perform the forwarding for an unknown reason,
            // it's better to run a second instance than potentially end up in a
            // state where Torch refuses to run even a first instance.
            Err(err) => {
                let err = anyhow::Error::from(err).context("Failed to forward startup args");
                report_error!(&err);
            }
        }
    }

    #[cfg(all(feature = "release_bundle", windows))]
    if let LaunchMode::App { .. } = launch_mode {
        match app_services::windows::pass_startup_args_to_existing_instance(
            launch_mode.args().as_ref(),
        ) {
            // If we were able to contact an existing application instance, quit -
            // we only want to run a single instance of Torch at a time.
            Ok(_) => std::process::exit(0),
            // If Torch isn't already running, we're good to go.
            Err(app_services::windows::StartupArgsForwardingError::NoExistingInstance) => {}
            // If we just finished an auto-update, we should continue running.
            Err(app_services::windows::StartupArgsForwardingError::IgnoredAfterAutoUpdate) => {}
            Err(
                app_services::windows::StartupArgsForwardingError::IgnoredForCrashRecoveryProcess,
            ) => {}
            // If we were unable to perform the forwarding for an unknown reason,
            // it's better to run a second instance than potentially end up in a
            // state where Torch refuses to run even a first instance.
            Err(err) => {
                let err = anyhow::Error::from(err).context("Failed to forward startup args");
                report_error!(&err);
            }
        }
    }

    // Sets up a Job Object that we associate with the Torch process to handle
    // shared fate with its child processes. This should be called before we
    // start spawning any child processes.
    #[cfg(windows)]
    command::windows::init();

    let private_preferences = settings::init_private_user_preferences();
    let (public_preferences, startup_toml_parse_error) = settings::init_public_user_preferences();

    // When the SettingsFile feature flag is enabled, public settings live in
    // the TOML-backed store. When disabled, they live in the platform-native
    // store (same backend as private). Use the correct one for pre-app reads.
    #[cfg_attr(
        not(any(
            enable_crash_recovery,
            target_os = "linux",
            target_os = "freebsd",
            target_os = "macos"
        )),
        expect(unused)
    )]
    let prefs_for_public_settings: &dyn torchui_extras::user_preferences::UserPreferences =
        if FeatureFlag::SettingsFile.is_enabled() {
            public_preferences.as_ref()
        } else {
            private_preferences.deref()
        };

    #[cfg(enable_crash_recovery)]
    let crash_recovery =
        crash_recovery::CrashRecovery::new(&launch_mode, prefs_for_public_settings);

    // Set up the pty spawner before doing any meaningful work. We want to
    // ensure that the process is in the cleanest possible state (minimal opened
    // files, modified signal handlers, etc.) to avoid unexpected effects on
    // spawned ptys.
    //
    #[cfg(feature = "local_tty")]
    let pty_spawner =
        terminal::local_tty::spawner::PtySpawner::new().context("Failed to create pty spawner")?;

    let callbacks = app_callbacks(launch_mode.is_integration_test());
    let mut app_builder = if launch_mode.is_gui() {
        torchui::platform::AppBuilder::new(
            callbacks,
            Box::new(ASSETS),
            launch_mode.take_test_driver(),
        )
    } else {
        torchui::platform::AppBuilder::new_windowless(
            callbacks,
            Box::new(ASSETS),
            launch_mode.take_test_driver(),
        )
    };

    // A user is present for any launch with a UI, so it may query microphone authorization.
    if !launch_mode.is_headless() {
        app_builder.enable_windowless_microphone_access_query();
    }

    // A windowless invocation has no Dock presence, so it performs no Dock-visible
    // setup at all (Dock icon, Dock menu, menu bar). See APP-2946.
    #[cfg(target_os = "macos")]
    if launch_mode.is_gui() {
        use torchui::AssetProvider as _;
        use torchui::platform::mac::AppExt;

        let activate_on_launch = !launch_mode.is_integration_test()
            || std::env::var("WARPUI_USE_REAL_DISPLAY_IN_INTEGRATION_TESTS").is_ok();
        app_builder.set_activate_on_launch(activate_on_launch);

        let dev_icon = ASSETS.get("bundled/png/local.png")?;
        app_builder.set_dev_icon(dev_icon);

        let show_dock_icon = crate::settings::app_icon::ShowDockIconState::read_from_preferences(
            prefs_for_public_settings,
        )
        .unwrap_or_else(crate::settings::app_icon::ShowDockIconState::default_value);
        app_builder.set_show_dock_icon_on_launch(show_dock_icon);
        app_builder.set_menu_bar_builder(app_menus::menu_bar);
        app_builder.set_dock_menu_builder(|_| app_menus::dock_menu());
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        use torchui::platform::linux::{self, AppBuilderExt};

        use crate::settings::ForceX11;

        app_builder.set_window_class(ChannelState::app_id().to_string());

        let force_x11 = ForceX11::read_from_preferences(prefs_for_public_settings)
            .unwrap_or(ForceX11::default_value());
        // Force use of wayland if the user has passed the `TORCH_ENABLE_WAYLAND` env var.
        let allow_wayland = linux::is_wayland_env_var_set() || !force_x11;
        app_builder.force_x11(!allow_wayland);
    }

    #[cfg(target_os = "windows")]
    {
        use torchui::platform::windows::AppBuilderExt;
        app_builder.set_app_user_model_id(ChannelState::app_id().to_string());

        // Only use DXC for DirectX shader compilation if we're not running in a Parallels VM
        // Parallels VMs can have issues with DXC shader compilation
        let is_parallels_vm = crate::util::vm_detection::is_running_in_windows_parallels_vm();
        if !is_parallels_vm {
            log::info!("Using DXC for DirectX shader compilation");
            use torchui::platform::windows::DXCPath;

            app_builder.use_dxc_for_directx_shader_compilation(DXCPath {
                dxc_path: "dxcompiler.dll".to_string(),
                dxil_path: "dxil.dll".to_string(),
            });
        } else {
            log::info!("Skipping DXC for DirectX shader compilation; running in a Parallels VM");
        }
    }

    // Override any bindings that have a `Custom` trigger to a `Keystroke`-based trigger. In theory,
    // this should be a noop on Mac (since the keystrokes registered via the  Mac menus first
    // intercept the binding), but just to be safe we only enable this in cases where we don't
    // include mac menus.
    #[cfg(not(target_os = "macos"))]
    app_builder.convert_custom_triggers_to_keystroke_triggers(
        crate::util::bindings::custom_tag_to_keystroke,
    );

    #[cfg(target_os = "macos")]
    app_builder.register_default_keystroke_triggers_for_custom_actions(
        crate::util::bindings::custom_tag_to_keystroke,
    );

    app_builder.run(move |ctx| {
        #[cfg(not(target_family = "wasm"))]
        // Rotate the log files in the background.
        ctx.background_executor()
            .spawn(torch_logging::rotate_log_files())
            .detach();

        ctx.add_singleton_model(|ctx| {
            AppExecutionMode::new(launch_mode.execution_mode(), false, ctx)
        });

        // Add the terminal server singleton to the application.
        #[cfg(feature = "local_tty")]
        ctx.add_singleton_model(move |_ctx| pty_spawner);

        // Register user preferences.  This must be done before initializing
        // feature flags or experiments, both of which check user preferences for
        // overrides.
        ctx.add_singleton_model(move |_ctx| ::settings::PublicPreferences::new(public_preferences));
        ctx.add_singleton_model(move |_ctx| private_preferences);
        let startup_toml_parse_error = startup_toml_parse_error;

        #[cfg(enable_crash_recovery)]
        ctx.add_singleton_model(move |_ctx| crash_recovery);

        let app_state = initialize_app(&launch_mode, timer, startup_toml_parse_error, ctx);

        if ImprovedPaletteSearch::improved_search_enabled(ctx) {
            FeatureFlag::UseTantivySearch.set_enabled(true);
        }

        launch(ctx, app_state, launch_mode)
    })
}

pub struct UpdateQuakeModeEventArg {
    active_window_id: Option<WindowId>,
}

#[derive(Clone)]
enum StartupUserAuthentication {
    RefreshUser,
}

impl StartupUserAuthentication {
    fn start(self, ctx: &mut AppContext) {
        AuthManager::handle(ctx).update(ctx, |auth_manager, ctx| match self {
            Self::RefreshUser => auth_manager.refresh_user(ctx),
        });
    }
}

fn authenticate_user_after_iap_access(
    authentication: StartupUserAuthentication,
    ctx: &mut AppContext,
) {
    let iap_manager = IapManager::handle(ctx);
    if !iap_manager.as_ref(ctx).is_enabled() || iap_manager.as_ref(ctx).has_valid_token() {
        authentication.start(ctx);
        return;
    }

    let mut pending_authentication = Some(authentication);
    ctx.subscribe_to_model(&iap_manager, move |iap_manager, event, ctx| match event {
        IapManagerEvent::StateChanged => {
            if !iap_manager.as_ref(ctx).has_valid_token() {
                return;
            }
            if let Some(authentication) = pending_authentication.take() {
                authentication.start(ctx);
            }
        }
        IapManagerEvent::AccessUnavailable => {
            report_error!("Staging IAP access unavailable before startup user authentication");
        }
        IapManagerEvent::RefreshFailed {
            message: _,
            is_first_failure_of_streak: _,
        } => {}
    });
    iap_manager.update(ctx, |manager, ctx| manager.ensure_access(ctx));
}

#[::tracing::instrument(skip_all, fields(tags.cloud_agent = true))]
pub(crate) fn initialize_app(
    launch_mode: &LaunchMode,
    mut timer: IntervalTimer,
    startup_toml_parse_error: Option<torchui_extras::user_preferences::Error>,
    ctx: &mut torchui::AppContext,
) -> Option<AppState> {
    let secure_storage_service_name = ChannelState::data_domain();

    // Daemon auth arrives through the client handshake, so avoid platform keychains that may
    // require an interactive unlock prompt. Other headless modes still use secure storage for
    // persisted login and BYO provider credentials.
    if matches!(launch_mode, LaunchMode::RemoteServerDaemon { .. }) {
        torchui_extras::secure_storage::register_unavailable(ctx);
    } else {
        // Register an implementation of the secure storage service.
        cfg_if::cfg_if! {
            if #[cfg(feature = "integration_tests")] {
                torchui_extras::secure_storage::register_noop(&secure_storage_service_name, ctx);
            } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
                torchui_extras::secure_storage::register_with_fallback(&secure_storage_service_name, torch_core::paths::state_dir(), ctx)
            } else if #[cfg(target_os = "windows")] {
                torchui_extras::secure_storage::register_with_dir(&secure_storage_service_name, torch_core::paths::state_dir(), ctx)
            } else {
                torchui_extras::secure_storage::register(&secure_storage_service_name, ctx);
            }
        }
    }

    // One-time migration: give Preview its own config directory by
    // symlinking contents from the shared ~/.warp location. Must run
    // before ensure_warp_watch_roots_exist() creates the new directory.
    #[cfg(target_os = "macos")]
    preview_config_migration::migrate_preview_config_dir_if_needed();

    ensure_warp_watch_roots_exist();
    ctx.add_singleton_model(WarpManagedPathsWatcher::new);

    ctx.add_singleton_model(WarpConfig::new);
    ctx.add_singleton_model(|_ctx| SettingsManager::default());

    let user_defaults_on_startup = settings::init(startup_toml_parse_error, ctx);
    timer.mark_interval_end("READ_USER_DEFAULTS_AND_INITIALIZE_SETTINGS");

    if FeatureFlag::UIZoom.is_enabled() {
        ctx.set_zoom_factor(WindowSettings::as_ref(ctx).zoom_level.as_zoom_factor());
    }

    let auth_state = AuthState::initialize(ctx);
    let auth_state = Arc::new(auth_state);
    timer.mark_interval_end("AUTH_MANAGER_SET_USER");

    // NetworkLogModel must be registered before ServerApiProvider so that
    // `NetworkLogModel::install_on_clients` can reach it when forwarding items
    // captured by the HTTP client hooks.
    ctx.add_singleton_model(|_ctx| NetworkLogModel::default());

    // Create a shared IAP state for staging builds. The same `Arc<IapState>`
    // is handed to both `ServerApi` (for sync reads on the request path) and
    // `IapManager` (which owns refresh logic on the main thread).
    #[cfg(not(target_family = "wasm"))]
    let iap_state = ChannelState::iap_config().map(|cfg| Arc::new(IapState::new(&cfg)));
    #[cfg(target_family = "wasm")]
    let iap_state: Option<Arc<IapState>> = None;

    let server_api_provider = ctx.add_singleton_model({
        let auth_state = auth_state.clone();
        let iap_state = iap_state.clone();
        move |ctx| ServerApiProvider::new(auth_state, iap_state, ctx)
    });

    let server_api = server_api_provider.as_ref(ctx).get();
    ctx.add_singleton_model(|_ctx| AuthStateProvider::new(auth_state.clone()));

    ctx.add_singleton_model(AppTelemetryContextProvider::new_context_provider);

    ctx.add_singleton_model(|ctx| {
        AuthManager::new(
            server_api.clone(),
            server_api_provider.as_ref(ctx).get_auth_client(),
            ctx,
        )
    });

    ctx.add_singleton_model(|_ctx| GPUState::new());

    PrivacySettings::register_singleton(ctx);

    // If any part of sqlite initialization fails, we just don't do session restoration (i.e.
    // feature degradation).
    let persistence_scope = match launch_mode {
        LaunchMode::RemoteServerDaemon { identity_key } => {
            persistence::PersistenceScope::RemoteServerDaemon {
                identity_key: identity_key.clone(),
            }
        }
        LaunchMode::App { .. } | LaunchMode::RemoteServerProxy | LaunchMode::Test { .. } => {
            persistence::PersistenceScope::App
        }
    };
    // Only read the subsets of persisted data this launch mode actually
    // consumes; loading everything is expensive on large databases.
    let persisted_data_scope = match launch_mode {
        LaunchMode::RemoteServerDaemon { .. } => {
            persistence::PersistedDataScope::CodebaseIndicesOnly
        }
        LaunchMode::App { .. } | LaunchMode::RemoteServerProxy | LaunchMode::Test { .. } => {
            persistence::PersistedDataScope::Full
        }
    };
    let (sqlite_data, writer_handles) =
        persistence::initialize(ctx, persistence_scope, persisted_data_scope);
    timer.mark_interval_end("SQLITE_INITIALIZED");

    let persistence_writer = PersistenceWriter::new(writer_handles);

    let model_event_sender = persistence_writer.sender();

    let tips_handle = ctx.add_model(|_| user_defaults_on_startup.tips_data);
    let user_default_shell_unsupported_banner_model_handle =
        ctx.add_model(|_| user_defaults_on_startup.user_default_shell_unsupported_banner_state);
    // Extract the full-file parse error (if any) before the settings_file_error
    // value is moved below. Only FileParseFailed gates the broken-file guard
    // in `initialize_cloud_preferences_syncer`; InvalidSettings means TOML
    // parsed but individual values were wrong, which doesn't mean local
    // state is unusable.
    let startup_toml_parse_error_for_syncer = user_defaults_on_startup
        .settings_file_error
        .as_ref()
        .and_then(|err| match err {
            settings::SettingsFileError::FileParseFailed(msg) => Some(msg.clone()),
            settings::SettingsFileError::InvalidSettings(_) => None,
        });
    let settings_file_error = user_defaults_on_startup.settings_file_error;
    ctx.add_singleton_model(move |_ctx| {
        GlobalResourceHandlesProvider::new(GlobalResourceHandles {
            model_event_sender,
            tips_completed: tips_handle,
            user_default_shell_unsupported_banner_model_handle,
            settings_file_error,
        })
    });

    let (
        cloud_objects,
        cached_workspaces,
        current_workspace_uid,
        app_state,
        command_history,
        restored_user_profiles,
        time_of_next_force_object_refresh,
        object_actions,
        experiments,
        persisted_workspaces,
        workspace_language_servers,
        persisted_projects,
        persisted_ignored_suggestions,
    ) = sqlite_data
        .map(|sqlite_data| {
            (
                sqlite_data.cloud_objects,
                sqlite_data.workspaces,
                sqlite_data.current_workspace_uid,
                sqlite_data.app_state,
                sqlite_data.command_history,
                sqlite_data.user_profiles,
                sqlite_data.time_of_next_force_object_refresh,
                sqlite_data.object_actions,
                sqlite_data.experiments,
                sqlite_data.codebase_indices,
                sqlite_data.workspace_language_servers,
                sqlite_data.projects,
                sqlite_data.ignored_suggestions,
            )
        })
        .unwrap_or_else(|| {
            (
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
            )
        });

    // The daemon's `PersistedDataScope::CodebaseIndicesOnly` read already
    // skips everything except codebase index metadata.
    if matches!(launch_mode, LaunchMode::RemoteServerDaemon { .. }) {
        let codebase_index_count = persisted_workspaces.len();
        log::debug!(
            "[Remote codebase indexing] Restored daemon codebase index metadata: metadata_count={codebase_index_count}"
        );
    }

    // Initialize a global model to track server-side experiment state.
    // This depends on the [`GlobalResourceHandlesProvider`] and so it must
    // be initialized after it.
    ctx.add_singleton_model(|ctx| ServerExperiments::new_from_cache(experiments, ctx));

    ctx.add_singleton_model(|ctx| {
        UserWorkspaces::new(
            server_api_provider.as_ref(ctx).get_team_client(),
            server_api_provider.as_ref(ctx).get_workspace_client(),
            cached_workspaces,
            current_workspace_uid,
            ctx,
        )
    });

    ctx.add_singleton_model(AntivirusInfo::new);

    if let LaunchMode::App { .. } = launch_mode {
        autoupdate::check_and_report_update_errors(ctx);
    }

    ctx.set_fallback_font_source_provider(|url| ::asset_cache::url_source(url));

    ctx.set_default_binding_validator(is_binding_cross_platform);

    // Attempt to clean up any old executable, whether or not we were explicitly
    // launched as part of the auto-update process. We may have failed to remove
    // the executable on a previous launch of the app and should try again.
    //
    // On macOS this deletes `Contents/MacOS/old` from inside the installed app
    // bundle, so it runs behind the same `can_autoupdate` guard as the rest of
    // the autoupdate machinery: an execution mode that never autoupdates must
    // not mutate that bundle. The bundled CLI runs the GUI executable from
    // inside `Torch.app`, so without this it would rewrite a bundle it does not
    // own. See APP-2946.
    if FeatureFlag::Autoupdate.is_enabled()
        && AppExecutionMode::as_ref(ctx).can_autoupdate()
        && let Err(e) = autoupdate::remove_old_executable()
    {
        report_error!(e.context("Failed to remove old executable"));
    }

    experiments::init(ctx);

    ctx.add_singleton_model(|_| SettingsPaneManager::new());
    ctx.add_singleton_model(|_| NetworkLogPaneManager::default());
    ctx.add_singleton_model(|ctx| {
        // Not using the *Provider types isn't ideal, but it's worth it for the ability to move managed secrets to a separate crate.
        ManagedSecretManager::new(
            server_api_provider.as_ref(ctx).get_managed_secrets_client(),
            auth_state.clone(),
        )
    });

    #[cfg(feature = "local_tty")]
    terminal::available_shells::register(ctx);

    // Add truly global actions that don't depend on the existence of any view here
    ctx.add_global_action("app:toggle_user_ps1", move |_args: &(), ctx| {
        // The input type and PS1 setting must change together; see `InputSettings::input_type`.
        let use_shell_prompt = crate::settings::InputSettings::as_ref(ctx).input_type(ctx)
            == crate::settings::InputBoxType::Universal;
        crate::settings::InputSettings::handle(ctx).update(ctx, |input_settings, ctx| {
            report_if_error!(input_settings.input_box_type.set_value(
                if use_shell_prompt {
                    crate::settings::InputBoxType::Classic
                } else {
                    crate::settings::InputBoxType::Universal
                },
                ctx
            ));
        });
        SessionSettings::handle(ctx).update(ctx, |session_settings, ctx| {
            report_if_error!(session_settings.honor_ps1.set_value(use_shell_prompt, ctx));
        });
    });
    ctx.add_global_action("app:toggle_copy_on_select", move |_args: &(), ctx| {
        SelectionSettings::handle(ctx).update(ctx, |selection_settings, ctx| {
            report_if_error!(selection_settings.copy_on_select.toggle_and_save_value(ctx));
        });
    });

    ctx.add_singleton_model(|_ctx| SyncedInputState::new());

    ctx.add_singleton_model(remote_server::manager::RemoteServerManager::new);
    #[cfg(not(target_family = "wasm"))]
    #[cfg(not(target_family = "wasm"))]
    remote_server::wire_auth_token_rotation(ctx);

    log::info!(
        "Starting torch with channel state {} and version {:?}",
        ChannelState::debug_str(),
        ChannelState::app_version()
    );

    // Teach our app that sometimes option means meta.
    ctx.set_event_munger(move |event, ctx| {
        let extra_meta_keys = *KeysSettings::as_ref(ctx).extra_meta_keys;
        apply_extra_meta_keys(event, extra_meta_keys);
        apply_scroll_multiplier(event, ctx);
    });

    // Rewrite recognized Torch web URLs (sessions, Drive, settings, home) into local
    // intent URLs when possible so they open directly in the desktop app.
    ctx.set_before_open_url(|url_str, _ctx| {
        if let Ok(url) = Url::parse(url_str)
            && let Some(intent) = maybe_rewrite_web_url_to_intent(&url)
        {
            return intent.to_string();
        }
        url_str.to_owned()
    });

    ctx.set_a11y_verbosity(*AccessibilitySettings::as_ref(ctx).a11y_verbosity);

    #[cfg(enable_crash_recovery)]
    ctx.on_draw_frame_error(|ctx, window_id| {
        crash_recovery::CrashRecovery::handle(ctx).update(ctx, |crash_recovery, _ctx| {
            crash_recovery.on_draw_frame_error(window_id);
        });
    });

    let user_is_logged_in = auth_state.is_logged_in();

    if user_is_logged_in {
        // Set the first frame callback to record the app's startup time.
        // This is only sent for logged-in users so that new users don't skew performance metrics.
        let is_screen_reader_enabled = ctx.is_screen_reader_enabled();
        let from_relaunch = launch_mode.args().finish_update;
        ctx.on_first_frame_drawn(move |ctx| {
            let timing_data = IntervalTimer::handle(ctx).update(ctx, |timer, _| {
                timer.mark_interval_end("FIRST_FRAME_DRAWN");
                timer.compute_stats()
            });
            let event = TelemetryEvent::AppStartup(AppStartupInfo {
                is_session_restoration_on: user_defaults_on_startup.should_restore_session,
                is_screen_reader_enabled,
                from_relaunch,
                is_crash_reporting_enabled: false,
                timing_data,
            });

            GPUState::handle(ctx).update(ctx, |gpu_state, ctx| {
                gpu_state
                    .set_has_lower_power_gpu(torchui::rendering::is_low_power_gpu_available(), ctx);
            });

            for window_id in ctx.window_ids().collect_vec() {
                SettingsPaneManager::handle(ctx)
                    .read(ctx, |model, _| model.settings_view(window_id))
                    .update(ctx, |settings, ctx| {
                        settings.refresh_preferred_graphics_backend_dropdown(ctx);
                    })
            }

            send_telemetry_from_app_ctx!(event, ctx);
        });

        #[cfg(enable_crash_recovery)]
        ctx.on_frame_drawn(|ctx, window_id| {
            crash_recovery::CrashRecovery::handle(ctx).update(ctx, |crash_recovery, ctx| {
                crash_recovery.on_frame_drawn(window_id, ctx);
            });
        })
    }

    #[cfg(not(target_family = "wasm"))]
    {
        ctx.add_singleton_model(DirectoryWatcher::new);
        // Register the skill provider directories as force-included paths so
        // the gitignore-pruning watch descend filter still watches gitignored
        // skill directories (e.g. `.agents/skills`) for `Repository`
        // subscribers (LSP, MCP). Registered before any repository begins
        // watching so it gates descent on the very first registration.
        DirectoryWatcher::handle(ctx).update(ctx, |watcher, _| {
            watcher.register_force_included_paths(
                ::ai::skills::SKILL_PROVIDER_DEFINITIONS
                    .iter()
                    .map(|provider| provider.skills_path.clone()),
            );
        });
        ctx.add_singleton_model(|_| DetectedRepositories::default());
        // Nothing in local-only builds consumes home directory events, and watching `$HOME`
        // means filtering every filesystem event beneath it (agent CLIs write there constantly).
        if ChannelState::is_local_only() {
            ctx.add_singleton_model(HomeDirectoryWatcher::new_inert);
        } else if let Some(home_dir) = dirs::home_dir() {
            ctx.add_singleton_model(|ctx| HomeDirectoryWatcher::new(home_dir, ctx));
        } else {
            log::info!("Home directory not found; skipping HomeDirectoryWatcher registration");
        }
    }

    #[cfg(feature = "local_fs")]
    {
        let imported_config_model = ctx.add_singleton_model(ImportedConfigModel::new);

        // Local-only builds defer the search (which enumerates every system font and parses other
        // terminals' configs) until the user opens the import flow.
        if ChannelState::channel() != torch_core::channel::Channel::Integration
            && !ChannelState::is_local_only()
        {
            imported_config_model.update(ctx, |model, ctx| {
                model.search_for_settings_to_import(ctx);
            });
        }

        let emit_incremental_updates = matches!(launch_mode, LaunchMode::RemoteServerDaemon { .. });
        ctx.add_singleton_model(|ctx| {
            // Local-only builds have no consumer of whole-repo file trees, so don't build and
            // recursively watch one for every repository the user `cd`s into.
            let model = if emit_incremental_updates {
                RepoMetadataModel::new_with_incremental_updates(ctx)
            } else if ChannelState::is_local_only() {
                RepoMetadataModel::new_without_auto_indexing(ctx)
            } else {
                RepoMetadataModel::new(ctx)
            };
            model.register_force_included_paths(
                ::ai::skills::SKILL_PROVIDER_DEFINITIONS
                    .iter()
                    .map(|provider| provider.skills_path.clone()),
                ctx,
            );
            model.set_project_skill_provider_paths(
                ::ai::skills::SKILL_PROVIDER_DEFINITIONS
                    .iter()
                    .map(|provider| provider.skills_path.clone()),
                ctx,
            );

            // Subscribe to RemoteServerManager push events so that remote repo
            // metadata snapshots and incremental updates populate the remote
            // sub-model and trigger RepoMetadataEvent emissions.
            {
                use remote_server::manager::{RemoteServerManager, RemoteServerManagerEvent};
                let mgr = RemoteServerManager::handle(ctx);
                ctx.subscribe_to_model(&mgr, |me, _, event, ctx| match event {
                    RemoteServerManagerEvent::RepoMetadataSnapshot { host_id, update } => {
                        me.insert_remote_snapshot(host_id.clone(), update, ctx);
                    }
                    RemoteServerManagerEvent::RepoMetadataUpdated { host_id, update }
                    | RemoteServerManagerEvent::RepoMetadataDirectoryLoaded { host_id, update } => {
                        me.apply_remote_incremental_update(host_id, update, ctx);
                    }
                    RemoteServerManagerEvent::HostDisconnected { host_id } => {
                        me.remove_remote_repositories_for_host(host_id, ctx);
                    }
                    _ => {}
                });
            }

            model
        });
    }

    ctx.add_singleton_model(|_| GitRepoModels::new());

    ctx.add_singleton_model(|ctx| {
        ProjectManagementModel::new(persisted_projects, persistence_writer.sender(), ctx)
    });

    ctx.add_singleton_model(move |_| History::new(command_history));

    ctx.add_singleton_model(CustomSecretRegexUpdater::new);

    // Register initial keybindings prior to creating menus
    agent_status::init(ctx);
    app_services::init(ctx);
    // // TODO: Temporarily disabling keybindings for WASM builds. Will be implemented in future WASM support.
    #[cfg(not(target_family = "wasm"))]
    if code::editor_available() {
        code::editor::find::view::init(ctx);
    }
    workspace::init(ctx);
    pane_group::init(ctx);
    terminal::init(ctx);
    input::init(ctx);
    editor::init(ctx);
    menu::init(ctx);
    tips::tip_view::init(ctx);
    launch_configs::init(ctx);
    workflows::init(ctx);
    themes::theme_chooser::init(ctx);
    themes::theme_creator_modal::init(ctx);
    themes::theme_deletion_modal::init(ctx);
    root_view::init(ctx);
    voltron::init(ctx);
    crate::view_components::find::init(ctx);
    prompt::editor_modal::init(ctx);
    undo_close::init(ctx);
    tab_configs::new_worktree_modal::init(ctx);
    tab_configs::params_modal::init(ctx);
    env_vars::env_var_collection_block::init(ctx);
    context_chips::display_menu::init(ctx);
    context_chips::node_version_popup::init(ctx);
    env_vars::view::env_var_collection::init(ctx);
    if FeatureFlag::CodeReviewSaveChanges.is_enabled() {
        code_review::init(ctx);
    }

    let display_count = ctx.windows().display_count();
    ctx.add_singleton_model(|_| DisplayCount(display_count));

    ctx.add_singleton_model(|_| RelaunchModel::new());
    ctx.add_singleton_model(|_| ChangelogModel::new(server_api.clone()));
    ctx.add_singleton_model(|_| NetworkStatus::new());
    ctx.add_singleton_model(|_| SystemStats::new());
    ctx.add_singleton_model(|_| KeybindingChangedNotifier::new());
    ctx.add_singleton_model(|_| TabShortcutModifierState::new());
    ctx.add_singleton_model(|_| search::command_palette::SelectedItems::new());
    ctx.add_singleton_model(search::files::model::FileSearchModel::new);
    ctx.add_singleton_model(|_| VimRegisters::new());
    ctx.add_singleton_model(UndoCloseStack::new);
    ctx.add_singleton_model(|_| ToastStack);
    ctx.add_singleton_model(|_| GlobalCodeReviewModel);
    #[cfg(feature = "local_fs")]
    ctx.add_singleton_model(FileModel::new);
    ctx.add_singleton_model(GlobalBufferModel::new);
    #[cfg(windows)]
    ctx.add_singleton_model(util::traffic_lights::windows::RendererState::new);
    #[cfg(feature = "local_fs")]
    ctx.add_singleton_model(|_| LanguageServerShutdownManager::new());

    let mut all_queue_items = Vec::new();
    let objects_with_pending_changes = cloud_objects
        .iter()
        .filter(|object| object.metadata().has_pending_content_changes())
        .cloned()
        .collect::<Vec<_>>();
    all_queue_items.extend(QueueItem::from_cached_objects(
        objects_with_pending_changes.into_iter(),
    ));

    let cloud_model = ctx.add_singleton_model(|_ctx| {
        CloudModel::new(
            persistence_writer.sender(),
            cloud_objects,
            time_of_next_force_object_refresh,
        )
    });

    let unsynced_actions: Vec<(CloudObjectTypeAndId, ObjectAction)> = object_actions
        .iter()
        .filter(|action| action.is_pending())
        .filter_map(|action| {
            cloud_model.read(ctx, |model, _| {
                let object = model.get_by_uid(&action.uid);
                object.map(|o| (o.cloud_object_type_and_id(), action.clone()))
            })
        })
        .collect::<Vec<_>>();

    all_queue_items.extend(QueueItem::from_unsynced_actions(
        unsynced_actions.into_iter(),
    ));

    ctx.add_singleton_model(|ctx| {
        SyncQueue::new(
            all_queue_items,
            server_api_provider.as_ref(ctx).get_cloud_objects_client(),
            ctx,
        )
    });

    ctx.add_singleton_model(|_| CLIAgentSessionsModel::new());
    ctx.add_singleton_model(AgentNotificationsModel::new);

    ctx.add_singleton_model(|_| UserProfiles::new(restored_user_profiles));

    ctx.add_singleton_model(|_| ObjectActions::new(object_actions));

    ctx.add_singleton_model(|_| AudibleBell::new());

    // This model has to be registered after the user workspaces model because it relies on it,
    // and before the UpdateManager models because they rely on the TeamTester model.
    ctx.add_singleton_model(TeamTesterStatus::new);

    ctx.add_singleton_model(|ctx| {
        TeamUpdateManager::new(
            server_api_provider.as_ref(ctx).get_team_client(),
            persistence_writer.sender(),
            ctx,
        )
    });

    ctx.add_singleton_model(|ctx| {
        UpdateManager::new(
            persistence_writer.sender(),
            server_api_provider.as_ref(ctx).get_cloud_objects_client(),
            ctx,
        )
    });

    let toml_file_path = settings::user_preferences_toml_file_path();
    let cloud_preferences_syncer = ctx.add_singleton_model(move |ctx| {
        initialize_cloud_preferences_syncer(
            toml_file_path,
            startup_toml_parse_error_for_syncer.as_deref(),
            ctx,
        )
    });
    ctx.subscribe_to_model(&cloud_preferences_syncer, |_, event, ctx| {
        if let CloudPreferencesSyncerEvent::InitialLoadCompleted = event {
            window_settings::migrate_legacy_background_backdrop(ctx);
        }
    });

    // LogManager must be registered before any subsystem (e.g. MCP, LSP) that creates file-based loggers.
    ctx.add_singleton_model(|_| simple_logger::manager::LogManager::new());

    // CloudViewModel subscribes to UpdateManager so that it can be notified when objects are
    // created on the server.
    ctx.add_singleton_model(CloudViewModel::new);

    ctx.add_singleton_model(|ctx| NotebookManager::new(ctx));
    ctx.add_singleton_model(|_| CodeManager::default());
    ctx.add_singleton_model(|_| OpenedFilesModel::new());
    ctx.add_singleton_model(NotebookKeybindings::new);
    ctx.add_singleton_model(|_| ActiveSession::default());
    ctx.add_singleton_model(|ctx| {
        Listener::new(
            server_api_provider.as_ref(ctx).get_cloud_objects_client(),
            ctx,
        )
    });

    #[cfg(all(not(target_family = "wasm"), feature = "local_tty"))]
    {
        ctx.add_singleton_model(LocalShellState::new);
        ctx.add_singleton_model(system::SystemInfo::new);
    }

    // `IapManager` drives IAP token refresh for staging builds.
    // Register it after `LocalShellState`: the Manager needs to know where the gcloud
    // cli lives & thus needs PATH config set by ~/.zshrc et al.
    //
    // Registered on all targets (including wasm) so consumers such as the
    // shared-session viewer network — which compiles and runs on wasm — can
    // read the singleton without panicking. On wasm `iap_state` is always
    // `None`, making this an inert no-op: `IapManager::new` early-returns from
    // its refresh loop and `iap_state()` yields no proxy-auth header.
    ctx.add_singleton_model(move |ctx| {
        let path_resolver = Box::new(|_ctx: &mut AppContext| {
            #[cfg(feature = "local_tty")]
            let path_future = LocalShellState::handle(_ctx).update(_ctx, |shell_state, ctx| {
                shell_state.get_interactive_path_env_var(ctx)
            });

            #[cfg(not(feature = "local_tty"))]
            let path_future =
                futures::FutureExt::boxed_local(futures::future::ready(None::<String>));

            path_future
        });
        IapManager::new(iap_state, path_resolver, None, ctx)
    });
    // Subscribe to IAP manager events to show toasts when refresh fails.
    ctx.subscribe_to_model(&IapManager::handle(ctx), |_, e, ctx| {
        match e {
            IapManagerEvent::RefreshFailed {
                message,
                is_first_failure_of_streak,
            } if *is_first_failure_of_streak => {
                let window_id = ctx
                    .windows()
                    .active_window()
                    .or_else(|| ctx.windows().ordered_window_ids().first().copied());
                let Some(window_id) = window_id else {
                    return;
                };
                let toast: DismissibleToast<WorkspaceAction> =
                    DismissibleToast::error(format!("IAP credential refresh failed: {message}"));
                ToastStack::handle(ctx).update(ctx, |stack, ctx| {
                    stack.add_ephemeral_toast(toast, window_id, ctx);
                });
            }
            _ => {}
        };
    });

    let startup_authentication =
        user_is_logged_in.then_some(StartupUserAuthentication::RefreshUser);
    if let Some(authentication) = startup_authentication {
        authenticate_user_after_iap_access(authentication, ctx);
    }

    // Add a singleton model that holds the current prompt configuration.
    ctx.add_singleton_model(Prompt::new);

    // Add a singleton model for resizable modals whose size should be persisted through restarts.
    ctx.add_singleton_model(|_| ResizableData::default());

    ctx.add_singleton_model(EnvVarCollectionManager::new);
    ctx.add_singleton_model(WorkflowManager::new);

    AutoupdateState::register(ctx, server_api.clone());

    ctx.add_singleton_model(LocalWorkflows::new);

    timer.mark_interval_end("SINGLETON_MODELS_REGISTERED");

    ctx.add_singleton_model(move |_| timer);

    ctx.add_singleton_model(DefaultTerminal::new);

    ctx.add_singleton_model(|ctx| {
        PersistedWorkspace::new(
            persisted_workspaces,
            workspace_language_servers,
            persistence_writer.sender(),
            ctx,
        )
    });
    ctx.add_singleton_model(move |_| persistence_writer);

    ctx.add_singleton_model(move |_| IgnoredSuggestionsModel::new(persisted_ignored_suggestions));

    // Subscribe WorkflowAliases to the UpdateManager so that it can be notified when objects are
    // trashed.
    WorkflowAliases::handle(ctx).update(ctx, |aliases, ctx| {
        aliases.connect(ctx);
    });

    #[cfg(not(target_family = "wasm"))]
    if launch_mode.should_start_local_http_server() {
        ctx.add_singleton_model(move |ctx| {
            let routers = vec![
                app_installation_detection::make_router(),
                profiling::make_router(),
            ];
            http_server::HttpServer::new(routers, ctx)
        });
    }
    #[cfg(feature = "local_fs")]
    if matches!(
        launch_mode,
        LaunchMode::App { .. } | LaunchMode::Test { .. }
    ) && FeatureFlag::WarpControlCli.is_enabled()
    {
        ctx.add_singleton_model(local_control::LocalControlBridge::new);
        ctx.add_singleton_model(local_control::LocalControlServer::new);
    }

    app_state
}

pub(crate) fn app_callbacks(is_integration_test: bool) -> torchui::platform::AppCallbacks {
    torchui::platform::AppCallbacks {
        on_internet_reachability_changed: Some(Box::new(move |reachable, ctx| {
            NetworkStatus::handle(ctx)
                .update(ctx, move |me, ctx| me.reachability_changed(reachable, ctx));
        })),
        on_become_active: None,
        on_screen_changed: Some(Box::new(move |ctx| {
            ctx.dispatch_global_action(
                "root_view:move_quake_mode_window_from_screen_change",
                &KeysSettings::as_ref(ctx)
                    .quake_mode_settings
                    .value()
                    .clone(),
            );

            let new_display_count = ctx.windows().display_count();
            DisplayCount::handle(ctx).update(ctx, |display_count, ctx| {
                display_count.0 = new_display_count;
                ctx.notify();
            });
        })),
        on_cpu_awakened: Some(Box::new(move |ctx| {
            SystemStats::handle(ctx).update(ctx, move |system, ctx| {
                log::info!("System has returned from sleep");
                system.dispatch_cpu_was_awakened(ctx);
            });
        })),
        on_cpu_will_sleep: Some(Box::new(move |ctx| {
            SystemStats::handle(ctx).update(ctx, move |system, ctx| {
                log::info!("System is going to sleep...");
                system.dispatch_cpu_will_sleep(ctx);
            });
        })),
        on_resigned_active: Some(Box::new(move |ctx| {
            let active_window_id = ctx.windows().active_window();
            let update_quake_mode_arg = UpdateQuakeModeEventArg { active_window_id };

            ctx.dispatch_global_action("root_view:update_quake_mode_state", &update_quake_mode_arg);
        })),
        on_will_terminate: Some(Box::new(move |ctx| {
            NotebookManager::handle(ctx).update(ctx, |manager, ctx| {
                // Notebooks are only saved periodically, so ensure that any pending changes have
                // been sent to the writer thread before terminating.
                manager.close_notebooks(ctx);
            });
            workspace::flush_pending_save_app(ctx);

            PersistenceWriter::handle(ctx).update(ctx, |writer, _ctx| {
                writer.terminate();
            });

            // Shutdown all LSP servers gracefully before app termination
            lsp::LspManagerModel::handle(ctx).update(ctx, |manager, ctx| {
                manager.terminate(ctx);
            });

            // We want to tear down the terminal server before relaunching for
            // autoupdate, to ensure we're not running any extra Torch processes
            // when we bring up the new process.  Additionally, this must occur
            // after terminating the persistence writer, so we don't keep track
            // of the fact that the shell sessions terminated.
            #[cfg(feature = "local_tty")]
            terminal::local_tty::spawner::PtySpawner::handle(ctx).update(ctx, |pty_spawner, _| {
                pty_spawner.prepare_for_app_termination();
            });

            #[cfg(all(feature = "local_tty", windows))]
            terminal::local_tty::shutdown_all_pty_event_loops(ctx);

            // Tear down app services before spawning the new process, to
            // ensure that the new process doesn't find the old process while
            // attempting to enforce our single-instance policy on Linux.
            app_services::teardown(ctx);
            autoupdate::spawn_child_if_necessary(ctx);

            // Tear down any application profilers that are running, writing
            // results to disk.
            profiling::teardown();

            #[cfg(enable_crash_recovery)]
            crash_recovery::CrashRecovery::handle(ctx).update(ctx, |crash_recovery, _ctx| {
                crash_recovery.teardown();
            });
        })),
        on_should_close_window: Some(Box::new(move |window_id, ctx| {
            let general_settings = GeneralSettings::as_ref(ctx);
            // On Linux or Windows, if we're about to close the final window, we should quit the app instead.
            // On Mac, we do this conditionally based on a user setting.
            let quit_on_last_window_closed =
                cfg!(any(target_os = "linux", target_os = "freebsd", windows))
                    || *general_settings.quit_on_last_window_closed;
            if ctx.window_ids().count() == 1 && quit_on_last_window_closed {
                log::info!("No windows left, terminating app");
                ctx.terminate_app(TerminationMode::Cancellable, None);
                return ApproveTerminateResult::Cancel;
            }

            let summary = UnsavedStateSummary::for_window(window_id, ctx);

            send_telemetry_from_app_ctx!(
                TelemetryEvent::UserInitiatedClose {
                    initiated_on: CloseTarget::Window,
                },
                ctx
            );

            // Don't show dialog on integration test. Machine can't press buttons.
            if !is_integration_test && summary.save_unsaved_code_and_should_warn(ctx) {
                let shown = summary
                    .dialog()
                    .on_confirm(move |ctx| {
                        ctx.windows()
                            .close_window(window_id, TerminationMode::ForceTerminate);
                    })
                    .on_cancel(move |ctx| {
                        on_close_window_cancelled(window_id, false, ctx);
                    })
                    .on_show_processes(move |ctx| {
                        on_close_window_cancelled(window_id, true, ctx);
                    })
                    .show(ctx);
                if shown {
                    ApproveTerminateResult::Cancel
                } else {
                    ApproveTerminateResult::Terminate
                }
            } else {
                ApproveTerminateResult::Terminate
            }
        })),
        on_should_terminate_app: Some(Box::new(move |source, ctx| {
            // Never interrupt a system-initiated termination (logout / restart /
            // scheduled OS update): both cancel paths below return
            // `ApproveTerminateResult::Cancel`, which macOS interprets as Torch
            // refusing to quit. That can abort a scheduled OS update while the
            // quit-warning modal has no visible window to attach to, leaving
            // Torch waiting on a prompt nobody can see (#12441). Skipping
            // `apply_pending_update` here doesn't lose the update: the next
            // update check re-detects it (autoupdate state isn't persisted
            // across restarts, so the artifact may be re-downloaded).
            if source == TerminationRequestSource::System {
                return ApproveTerminateResult::Terminate;
            }

            send_telemetry_from_app_ctx!(
                TelemetryEvent::UserInitiatedClose {
                    initiated_on: CloseTarget::App,
                },
                ctx
            );

            // If there's a pending autoupdate, apply that before showing the unsaved changes
            // dialog. We apply the update first so that the dialog can force-terminate.
            let applying_update = autoupdate::apply_pending_update(ctx, |ctx| {
                // Once the deferred update is applied, re-terminate the app. This termination is
                // cancellable so that we still show the unsaved changes dialog.
                log::info!("Deferred autoupdate applied, terminating app");
                ctx.terminate_app(TerminationMode::Cancellable, None);
            });
            if applying_update {
                return ApproveTerminateResult::Cancel;
            }

            let summary = UnsavedStateSummary::for_app(ctx);
            // Don't show dialog on integration test. Machine can't press buttons.
            if !is_integration_test && summary.save_unsaved_code_and_should_warn(ctx) {
                let shown = summary
                    .dialog()
                    .on_confirm(|ctx| ctx.terminate_app(TerminationMode::ForceTerminate, None))
                    .on_show_processes(|ctx| on_close_app_cancelled(true, ctx))
                    .on_cancel(|ctx| on_close_app_cancelled(false, ctx))
                    .show(ctx);
                if shown {
                    return ApproveTerminateResult::Cancel;
                }
            }

            ApproveTerminateResult::Terminate
        })),
        on_disable_warning_modal: Some(Box::new(move |ctx| {
            GeneralSettings::handle(ctx).update(ctx, |general_settings, ctx| {
                report_if_error!(
                    general_settings
                        .show_warning_before_quitting
                        .toggle_and_save_value(ctx)
                );
            });
            send_telemetry_from_app_ctx!(TelemetryEvent::QuitModalDisabled, ctx);
        })),
        on_notification_clicked: Some(Box::new(move |notification_response, ctx| {
            if let Some(notification_data) = notification_response.data() {
                let context: serde_json::Result<NotificationContext> =
                    serde_json::from_str(notification_data);
                if let Ok(NotificationContext::BlockOrigin {
                    window_id,
                    pane_group_id,
                    pane_id,
                }) = context
                {
                    // Ensure the window ID exists, if so dispatch an action to focus
                    // the correct pane.
                    if ctx.window_ids().contains(&window_id)
                        && let Some(root_view_id) = ctx.root_view_id(window_id)
                    {
                        ctx.dispatch_action(
                            window_id,
                            &[root_view_id],
                            "root_view:handle_notification_click",
                            &PaneViewLocator {
                                pane_group_id,
                                pane_id,
                            },
                            log::Level::Info,
                        );
                    }
                }
            }
        })),
        on_new_window_requested: Some(Box::new(move |ctx| {
            // This one is called when the app is requested to open a new window,
            // e.g. clicking on the Dock icon. It is NOT called from the New Window
            // menu item.
            ctx.dispatch_global_action("root_view:open_new", &());
            ctx.dispatch_global_action("workspace:save_app", &());
        })),
        on_open_urls: Some(Box::new(move |urls, ctx| {
            for url in &urls {
                let parsed_url = Url::parse(url);
                match parsed_url {
                    Ok(url) => uri::handle_incoming_uri(&url, ctx),
                    Err(e) => log::warn!("Unable to parse received url: {e}"),
                }
            }
        })),
        on_os_appearance_changed: Some(Box::new(move |ctx| {
            AppearanceManager::handle(ctx).update(ctx, |appearance_manager, ctx| {
                appearance_manager.refresh_theme_state(ctx);
            });
        })),
        on_active_window_changed: Some(Box::new(move |ctx| {
            let windowing_model = ctx.windows();
            let active_window_id = windowing_model.active_window();
            let key_window_is_modal_panel = windowing_model.key_window_is_modal_panel();

            if !key_window_is_modal_panel {
                let update_quake_mode_arg = UpdateQuakeModeEventArg { active_window_id };
                ctx.dispatch_global_action(
                    "root_view:update_quake_mode_state",
                    &update_quake_mode_arg,
                );
            }

            ctx.dispatch_global_action("workspace:save_app", &());
        })),
        on_window_will_close: Some(Box::new(move |closed_window_data, ctx| {
            if ctx.windows().stage() == ApplicationStage::Terminating {
                return;
            }

            if let Some(window_data) = closed_window_data {
                UndoCloseStack::handle(ctx).update(ctx, |stack, ctx| {
                    stack.handle_window_closed(window_data, ctx);
                });
            }
            workspace::save_app_now(ctx);
        })),
        on_window_moved: Some(Box::new(move |ctx| {
            ctx.dispatch_global_action("workspace:save_app", &());
        })),
        on_window_resized: Some(Box::new(move |ctx| {
            ctx.dispatch_global_action("workspace:save_app", &());
        })),
        ..Default::default()
    }
}

fn on_close_app_cancelled(open_navigation_palette: bool, ctx: &mut AppContext) {
    autoupdate::cancel_relaunch(ctx);

    send_telemetry_from_app_ctx!(
        TelemetryEvent::QuitModalCancel {
            nav_palette: open_navigation_palette,
            modal_for: CloseTarget::App,
        },
        ctx
    );

    let sessions = SessionNavigationData::all_sessions(ctx).collect_vec();
    let sessions_summary = RunningSessionSummary::new(&sessions);

    // If open_navigation_palette is false, return early. Otherwise, we honor the open_navigation_palette
    // param which is true if the user clicked the modal button for that. However, if the running
    // processes in this window have finished since the modal popped, there is nothing to do now and we
    // can return early
    if !open_navigation_palette || sessions_summary.long_running_cmds.is_empty() {
        return;
    }

    let windowing_model = ctx.windows();
    let active_window_id = windowing_model.active_window();
    // show the nav palette in the active window. if there is no active window,
    // arbitrarily pick one of the windows having a running process
    let window_id_to_focus = active_window_id.unwrap_or_else(|| {
        *sessions_summary
            .windows_running()
            .iter()
            .next()
            .expect("already checked len > 0")
    });

    windowing_model.show_window_and_focus_app(window_id_to_focus);

    // open the nav palette in the selected window
    if let Some(workspaces) = ctx.views_of_type::<Workspace>(window_id_to_focus)
        && let Some(handle) = workspaces.first()
    {
        ctx.dispatch_typed_action_for_view(
            window_id_to_focus,
            handle.id(),
            &WorkspaceAction::OpenPalette {
                mode: PaletteMode::Navigation,
                source: PaletteSource::QuitModal,
                query: Some("running".to_owned()),
            },
        );
    }
}

fn on_close_window_cancelled(
    window_id: WindowId,
    open_navigation_palette: bool,
    ctx: &mut AppContext,
) {
    send_telemetry_from_app_ctx!(
        TelemetryEvent::QuitModalCancel {
            nav_palette: open_navigation_palette,
            modal_for: CloseTarget::Window,
        },
        ctx
    );

    let sessions = SessionNavigationData::all_sessions(ctx).collect_vec();
    let sessions_summary = RunningSessionSummary::new(&sessions);
    let num_processes_in_window = sessions_summary.processes_in_window(&window_id).len();

    // If open_navigation_palette is false, return early. Otherwise, we honor the
    // open_navigation_palette param which is true if the user clicked the modal
    // button for that. However, if the running processes in this window have finished
    // since the modal popped, there is nothing to do now and we can return early
    if !open_navigation_palette || num_processes_in_window == 0 {
        return;
    }

    ctx.windows().show_window_and_focus_app(window_id);

    // if we haven't returned early, it means open_navigation_palette is true as the
    // user pressed the modal button for opening the navigation palette to show their
    // running processes
    if let Some(workspaces) = ctx.views_of_type::<Workspace>(window_id)
        && let Some(handle) = workspaces.first()
    {
        ctx.dispatch_typed_action_for_view(
            window_id,
            handle.id(),
            &WorkspaceAction::OpenPalette {
                mode: PaletteMode::Navigation,
                source: PaletteSource::QuitModal,
                query: Some("running".to_owned()),
            },
        );
    }
}

fn is_cloud_agent_web_home_launch_url(url: &Url) -> bool {
    url.scheme() == ChannelState::url_scheme()
        && url.host_str() == Some("action")
        && url.path() == "/new_cloud_agent_conversation"
        && url
            .query_pairs()
            .any(|(key, value)| key == "source" && value == "web_home")
}

#[::tracing::instrument(skip_all, fields(tags.cloud_agent = true))]
fn launch(ctx: &mut torchui::AppContext, app_state: Option<AppState>, launch_mode: LaunchMode) {
    IntervalTimer::handle(ctx).update(ctx, |timer, _ctx| {
        timer.mark_interval_end("APP_LAUNCHED");
    });

    keyboard::load_custom_keybindings(ctx);

    IntervalTimer::handle(ctx).update(ctx, |timer, _ctx| {
        timer.mark_interval_end("KEYBINDINGS_LOADED");
    });

    // For now, we only specify application-level fallback fonts on web.
    #[cfg(target_family = "wasm")]
    ctx.set_fallback_font_fn(font_fallback::fallback_font_fn);

    match launch_mode {
        LaunchMode::App { .. } | LaunchMode::Test { .. } => {
            let should_skip_restore = launch_mode
                .args()
                .urls
                .iter()
                .any(is_cloud_agent_web_home_launch_url);
            let app_state = if should_skip_restore { None } else { app_state };
            // Attempt to restore windows from the persisted application state.
            let arg = OpenFromRestoredArg { app_state };
            ctx.dispatch_global_action("root_view:open_from_restored", &arg);

            // Process any URLs that were provided on the command line (which may be
            // file:// URLs or ones using our custom URL scheme).
            for url in launch_mode.args().urls.iter() {
                uri::handle_incoming_uri(url, ctx);
            }

            // If, after session restoration and command-line argument handling, we
            // haven't opened any windows, open a new window.
            if ctx.window_ids().count() == 0 {
                ctx.dispatch_global_action("root_view:open_new", &());
            }

            IntervalTimer::handle(ctx).update(ctx, |timer, _| {
                timer.mark_interval_end("WINDOWS_CREATED");
            });

            // TODO(ben): We should skip this for LaunchMode::Test.
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            {
                use crate::login_item::maybe_register_app_as_login_item;
                use crate::terminal::general_settings::GeneralSettingsChangedEvent;
                // Note that we put this here because it depends on settings already having been initialized.
                ctx.subscribe_to_model(&GeneralSettings::handle(ctx), |_, event, ctx| {
                    if matches!(event, GeneralSettingsChangedEvent::LoginItem { .. }) {
                        maybe_register_app_as_login_item(ctx);
                    }
                });
                maybe_register_app_as_login_item(ctx);
            }
        }
        // Proxy should never reach launch() — it's a thin byte bridge.
        LaunchMode::RemoteServerProxy => {
            report_error!("Proxy mode should not use the launch() path");
            std::process::exit(1);
        }
        // Daemon: bind the Unix socket and register the ServerModel.
        // initialize_app already set up everything else including crash
        // reporting.
        #[cfg(unix)]
        LaunchMode::RemoteServerDaemon { identity_key } => {
            remote_server::unix::launch_daemon(&identity_key, ctx);
        }
        #[cfg(not(unix))]
        LaunchMode::RemoteServerDaemon { .. } => {
            report_error!("RemoteServerDaemon is not supported on this platform");
            std::process::exit(1);
        }
    }
}

/// Initializes the logger before running tests.
///
/// The `ctor` attribute here means that this runs BEFORE main(), whenever the
/// binary is executed. For this reason, we need to ensure that this function
/// only exists within unit test code. Production bundles and integration tests
/// also initialize the logging system, and initializing it twice causes a panic.
///
/// Additionally, we must not write anything to stdout in this function, as it
/// can interfere with test harnesses collecting the set of tests to run. (This
/// is why we're not simply calling the init() function above.)
#[ctor::ctor]
#[cfg(test)]
fn init_logging_for_unit_tests_glue() {
    // Initialize terminal-friendly logging for tests from the shared logger crate.
    torch_logging::init_logging_for_unit_tests();
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
