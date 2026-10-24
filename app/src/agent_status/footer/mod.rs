pub(super) mod chips;
pub mod editor;
pub mod toolbar_item;

#[cfg(not(target_family = "wasm"))]
use std::env;
#[cfg(not(target_family = "wasm"))]
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(not(target_family = "wasm"))]
use std::time::Duration;

use parking_lot::FairMutex;
use pathfinder_color::ColorU;
use pathfinder_geometry::vector::Vector2F;
#[cfg(not(target_family = "wasm"))]
use tokio::fs;
use toolbar_item::AgentToolbarItemKind;
use torch_core::channel::ChannelState;
use torch_core::ui::color::ContrastingColor;
use torch_core::ui::color::blend::Blend;
use torch_core::ui::color::contrast::MinimumAllowedContrast;
use torch_core::ui::theme::Fill;
use torch_core::ui::theme::color::internal_colors;
#[cfg(not(target_family = "wasm"))]
use torch_errors::report_error;
use torchui::r#async::Timer;
use torchui::elements::{
    ChildView, ConstrainedBox, Container, CrossAxisAlignment, DispatchEventResult, Element, Empty,
    EventHandler, Flex, MainAxisAlignment, MainAxisSize, ParentElement, Wrap, WrapFill,
    WrapFillEntireRun,
};
use torchui::{
    AppContext, Entity, EntityId, ModelHandle, SingletonEntity, TypedActionView, View, ViewContext,
    ViewHandle,
};

use crate::appearance::Appearance;
use crate::completer::SessionContext;
use crate::context_chips::display_chip::{DisplayChip, DisplayChipConfig, PromptChipShellCommand};
use crate::context_chips::prompt_type::PromptType;
use crate::context_chips::{self, ContextChipKind};
use crate::features::FeatureFlag;
use crate::local_hooks::{SetupState, StatusAgent};
use crate::send_telemetry_from_ctx;
#[cfg(not(target_family = "wasm"))]
use crate::server::telemetry::PluginChipTelemetryAction;
use crate::server::telemetry::{PluginChipTelemetryKind, TelemetryEvent};
use crate::settings::{AISettings, CodeSettings, CodeSettingsChangedEvent};
use crate::settings_view::SettingsSection;
#[cfg(not(target_family = "wasm"))]
use crate::terminal::ShellLaunchData;
#[cfg(not(target_family = "wasm"))]
use crate::terminal::cli_agent_sessions::plugin_manager::{
    CliAgentPluginManager, PluginInstallError, PluginModalKind, compare_versions, plugin_manager,
};
use crate::terminal::cli_agent_sessions::{
    CLIAgentInputState, CLIAgentSessionsModel, CLIAgentSessionsModelEvent,
};
use crate::terminal::session_settings::{
    SessionSettings, SessionSettingsChangedEvent, ToolbarChipSelection,
};
use crate::terminal::view::init::{ATTACH_FILE_KEYBINDING, OPEN_CLI_AGENT_RICH_INPUT_KEYBINDING};
use crate::terminal::{CLIAgent, TerminalModel};
use crate::ui_components::icons::Icon;
use crate::view_components::DismissibleToast;
#[cfg(not(target_family = "wasm"))]
use crate::view_components::ToastLink;
use crate::view_components::action_button::{
    ActionButton, ActionButtonTheme, AdjoinedSide, ButtonSize, KeystrokeSource, TooltipAlignment,
};
use crate::workspace::ToastStack;
#[cfg(not(target_family = "wasm"))]
use crate::workspace::WorkspaceAction;
use crate::workspace::view::TOGGLE_PROJECT_EXPLORER_BINDING_NAME;

/// How long to wait after session creation before showing the install chip.
/// Gives the plugin time to connect and send its `SessionStart` event.
#[cfg(not(target_family = "wasm"))]
const PLUGIN_CHIP_DEBOUNCE: Duration = Duration::from_secs(3);

#[cfg_attr(target_family = "wasm", allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginChipKind {
    Install,
    Update,
}

impl From<PluginChipKind> for PluginChipTelemetryKind {
    fn from(kind: PluginChipKind) -> Self {
        match kind {
            PluginChipKind::Install => PluginChipTelemetryKind::Install,
            PluginChipKind::Update => PluginChipTelemetryKind::Update,
        }
    }
}

/// Builds a composite key for per-agent, per-host plugin chip dismissal.
/// Returns `"<agent_prefix>"` for local sessions or `"<agent_prefix>@<host>"` for remote.
fn plugin_chip_key(agent_prefix: &str, remote_host: &Option<String>) -> String {
    match remote_host {
        Some(host) => format!("{agent_prefix}@{host}"),
        None => agent_prefix.to_owned(),
    }
}

/// Control bar shown while a CLI agent session is active: the agent icon, file attach, file
/// explorer, rich input, settings and context chips. It renders nothing without a session.
///
/// A single `ViewHandle<CLIAgentFooter>` is shared between `Input` and `UseAgentToolbar`.
pub struct CLIAgentFooter {
    terminal_view_id: EntityId,
    file_button: ViewHandle<ActionButton>,
    cli_display_chips: Vec<ViewHandle<DisplayChip>>,
    display_chip_config: DisplayChipConfig,

    terminal_model: Arc<FairMutex<TerminalModel>>,

    /// Opens the file explorer side panel.
    file_explorer_button: ViewHandle<ActionButton>,

    rich_input_button: ViewHandle<ActionButton>,
    settings_button: ViewHandle<ActionButton>,
    install_plugin_button: ViewHandle<ActionButton>,
    plugin_instructions_button: ViewHandle<ActionButton>,
    update_plugin_button: ViewHandle<ActionButton>,
    update_instructions_button: ViewHandle<ActionButton>,
    dismiss_plugin_chip_button: ViewHandle<ActionButton>,
    plugin_operation_in_progress: bool,
    /// When `true`, the install chip is allowed to render.
    /// Starts `false` and is set to `true` after a debounce timer fires,
    /// giving the plugin time to connect before we prompt installation.
    /// Reset to `false` when a listener connects.
    plugin_chip_ready: bool,
    /// Offers Torch's own status hooks in local-only builds, in place of Torch's plugins.
    status_setup_button: ViewHandle<ActionButton>,
    /// Whether Torch's status hooks are missing or outdated for this session's agent.
    needs_status_setup: bool,
}

impl CLIAgentFooter {
    pub fn new(
        terminal_view_id: EntityId,
        terminal_model: Arc<FairMutex<TerminalModel>>,
        prompt: ModelHandle<PromptType>,
        display_chip_config: DisplayChipConfig,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let cli_button_size = ButtonSize::AgentInputButton;

        let file_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("", AgentInputButtonTheme)
                .with_icon(Icon::Plus)
                .with_tooltip("Attach file")
                .with_tooltip_keybinding(ATTACH_FILE_KEYBINDING)
                .with_size(cli_button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(CLIAgentFooterAction::SelectFile);
                })
        });

        let file_explorer_button = ctx.add_typed_action_view(|ctx| {
            ActionButton::new("File explorer", AgentInputButtonTheme)
                .with_icon(Icon::FileCopy)
                .with_tooltip("Open file explorer")
                .with_size(cli_button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_keybinding(
                    KeystrokeSource::Binding(TOGGLE_PROJECT_EXPLORER_BINDING_NAME),
                    ctx,
                )
                .with_compact_keybinding(true)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(CLIAgentFooterAction::ToggleFileExplorer);
                })
        });
        // CLI agent-specific buttons (only rendered when a CLI agent session is active).
        let rich_input_button = ctx.add_typed_action_view(|ctx| {
            ActionButton::new("Rich Input", AgentInputButtonTheme)
                .with_icon(Icon::TextInput)
                .with_tooltip("Open Rich Input")
                .with_size(cli_button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_keybinding(
                    KeystrokeSource::Binding(OPEN_CLI_AGENT_RICH_INPUT_KEYBINDING),
                    ctx,
                )
                .with_compact_keybinding(true)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(CLIAgentFooterAction::ToggleRichInput);
                })
        });
        let settings_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("", AgentInputButtonTheme)
                .with_icon(Icon::Settings)
                .with_tooltip("Open coding agent settings")
                .with_size(cli_button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(CLIAgentFooterAction::OpenCodingAgentSettings);
                })
        });

        let install_plugin_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("Enable notifications", InstallPluginButtonTheme)
                .with_icon(Icon::Download)
                .with_tooltip(
                    "Install the Torch plugin to enable rich agent notifications within Torch",
                )
                .with_size(cli_button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_adjoined_side(AdjoinedSide::Right)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(CLIAgentFooterAction::InstallPlugin);
                })
        });

        let plugin_instructions_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("Notifications setup instructions", InstallPluginButtonTheme)
                .with_icon(Icon::Info)
                .with_tooltip("View instructions to install the Torch plugin")
                .with_size(cli_button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_adjoined_side(AdjoinedSide::Right)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(
                        CLIAgentFooterAction::OpenPluginInstallInstructionsPane,
                    );
                })
        });

        let update_plugin_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("Update Torch plugin", InstallPluginButtonTheme)
                .with_icon(Icon::Download)
                .with_tooltip("A new version of the Torch plugin is available")
                .with_size(cli_button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_adjoined_side(AdjoinedSide::Right)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(CLIAgentFooterAction::UpdatePlugin);
                })
        });

        let update_instructions_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("Plugin update instructions", InstallPluginButtonTheme)
                .with_icon(Icon::Info)
                .with_tooltip("View instructions to update the Torch plugin")
                .with_size(cli_button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_adjoined_side(AdjoinedSide::Right)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(
                        CLIAgentFooterAction::OpenPluginUpdateInstructionsPane,
                    );
                })
        });

        let dismiss_plugin_chip_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("", InstallPluginButtonTheme)
                .with_icon(Icon::X)
                .with_size(cli_button_size)
                .with_tooltip("Dismiss")
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_adjoined_side(AdjoinedSide::Left)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(CLIAgentFooterAction::DismissPluginChip);
                })
        });

        let status_setup_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("Set up session status", InstallPluginButtonTheme)
                .with_icon(Icon::Info)
                .with_tooltip("Show in the sidebar when this agent is working or needs you")
                .with_size(cli_button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_adjoined_side(AdjoinedSide::Right)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(CLIAgentFooterAction::OpenStatusSetup);
                })
        });

        // Toggle rich input button label when CLI input session opens/closes.
        // Also reset CLI voice state if the session ends while voice is active.
        ctx.subscribe_to_model(
            &CLIAgentSessionsModel::handle(ctx),
            move |me, _, event, ctx| {
                if event.terminal_view_id() != terminal_view_id {
                    return;
                }

                // Reset the debounce when a session ends so the next
                // session gets a fresh debounce window.
                if let CLIAgentSessionsModelEvent::Ended { .. } = event {
                    me.plugin_chip_ready = false;
                    me.needs_status_setup = false;
                }

                // When a structured plugin connects, the plugin is verified
                // installed — hide the chip. Codex's OSC 9 fallback is not a
                // structured plugin, so its chip stays until the plugin connects.
                if CLIAgentSessionsModel::as_ref(ctx)
                    .session(me.terminal_view_id)
                    .is_some_and(|s| s.supports_rich_status())
                {
                    me.plugin_chip_ready = false;
                }

                // When a session starts, update the install chip label and
                // start a debounce timer for non-auto-install agents.
                #[cfg(not(target_family = "wasm"))]
                if let CLIAgentSessionsModelEvent::Started { .. } = event
                    && let Some(agent) = me.cli_agent(ctx)
                {
                    if ChannelState::is_local_only() {
                        me.check_status_setup(agent, ctx);
                    }
                    let label = format!("Enable {} notifications", agent.display_name());
                    me.install_plugin_button.update(ctx, |button, ctx| {
                        button.set_label(label, ctx);
                    });
                    if let Some(manager) = plugin_manager()
                        && !manager.can_auto_install()
                    {
                        ctx.spawn(
                            Timer::after(PLUGIN_CHIP_DEBOUNCE),
                            |me, _, ctx: &mut ViewContext<Self>| {
                                let suppress = CLIAgentSessionsModel::as_ref(ctx)
                                    .session(me.terminal_view_id)
                                    .is_some_and(|s| s.supports_rich_status());
                                if !suppress {
                                    me.plugin_chip_ready = true;
                                    ctx.notify();
                                }
                            },
                        );
                    }
                }

                let CLIAgentSessionsModelEvent::InputSessionChanged {
                    new_input_state, ..
                } = event
                else {
                    ctx.notify();
                    return;
                };
                let is_open = matches!(new_input_state, CLIAgentInputState::Open { .. });
                me.rich_input_button.update(ctx, |button, ctx| {
                    if is_open {
                        button.set_label("Hide Rich Input", ctx);
                        button.set_tooltip(Some("Hide Rich Input"), ctx);
                        button.set_keybinding(
                            Some(KeystrokeSource::Binding(
                                OPEN_CLI_AGENT_RICH_INPUT_KEYBINDING,
                            )),
                            ctx,
                        );
                    } else {
                        button.set_label("Rich Input", ctx);
                        button.set_tooltip(Some("Open Rich Input"), ctx);
                        button.set_keybinding(
                            Some(KeystrokeSource::Binding(
                                OPEN_CLI_AGENT_RICH_INPUT_KEYBINDING,
                            )),
                            ctx,
                        );
                    }
                });
                ctx.notify();
            },
        );

        // The File explorer item's availability follows this setting, so the footer has to
        // repaint when it is toggled rather than waiting for an unrelated re-render.
        ctx.subscribe_to_model(&CodeSettings::handle(ctx), |_, _, event, ctx| {
            if matches!(event, CodeSettingsChangedEvent::ShowProjectExplorer { .. }) {
                ctx.notify()
            }
        });

        let prompt_for_session_settings = prompt.clone();
        ctx.subscribe_to_model(
            &SessionSettings::handle(ctx),
            move |me, _, event, ctx| match event {
                SessionSettingsChangedEvent::CLIAgentToolbarChipSelectionSetting { .. }
                | SessionSettingsChangedEvent::GithubPrChipDefaultValidation { .. } => {
                    me.update_display_chips(&prompt_for_session_settings, ctx);
                    ctx.notify();
                }
                _ => {}
            },
        );

        ctx.observe(&prompt, |me, model, ctx| {
            me.update_display_chips(&model, ctx);
        });

        let mut me = Self {
            terminal_view_id,
            file_button,
            file_explorer_button,
            rich_input_button,
            settings_button,
            install_plugin_button,
            plugin_instructions_button,
            update_plugin_button,
            update_instructions_button,
            dismiss_plugin_chip_button,
            plugin_operation_in_progress: false,
            plugin_chip_ready: false,
            status_setup_button,
            needs_status_setup: false,
            terminal_model,
            cli_display_chips: vec![],
            display_chip_config,
        };
        me.update_display_chips(&prompt, ctx);
        me
    }

    pub fn set_current_repo_path(
        &mut self,
        repo_path: Option<std::path::PathBuf>,
        ctx: &mut ViewContext<Self>,
    ) {
        self.display_chip_config.current_repo_path = repo_path;
        // Chips will be rebuilt on the next GitRepoStatusEvent::MetadataChanged.
        // Notify to ensure any existing chips reflect the change.
        ctx.notify();
    }

    pub fn update_session_context(
        &mut self,
        session_context: Option<SessionContext>,
        ctx: &mut ViewContext<Self>,
    ) {
        self.display_chip_config.session_context = session_context.clone();
        for chip_view in &self.cli_display_chips {
            chip_view.update(ctx, |chip, chip_ctx| {
                chip.update_session_context(session_context.clone(), chip_ctx);
            });
        }
    }

    fn has_active_cli_agent_input_session(&self, app: &AppContext) -> bool {
        CLIAgentSessionsModel::as_ref(app).is_input_open(self.terminal_view_id)
    }

    fn cli_agent(&self, app: &AppContext) -> Option<CLIAgent> {
        CLIAgentSessionsModel::as_ref(app)
            .session(self.terminal_view_id)
            .map(|session| session.agent)
    }

    fn is_cli_agent_session_active(&self, app: &AppContext) -> bool {
        CLIAgentSessionsModel::as_ref(app)
            .session(self.terminal_view_id)
            .is_some()
    }

    pub(crate) fn select_file(&mut self, ctx: &mut ViewContext<Self>) {
        let window_id = ctx.window_id();
        let view_id = ctx.view_id();
        let file_picker_config = torchui::platform::FilePickerConfiguration::new();

        ctx.open_file_picker(
            move |result, ctx| match result {
                Ok(paths) => {
                    if let Some(path) = paths.first() {
                        ctx.dispatch_typed_action_for_view(
                            window_id,
                            view_id,
                            &CLIAgentFooterAction::InsertFilePath(path.clone()),
                        );
                    }
                }
                Err(err) => {
                    let window_id = ctx.window_id();
                    ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                        toast_stack.add_ephemeral_toast(
                            DismissibleToast::error(format!("{err}")),
                            window_id,
                            ctx,
                        );
                    });
                }
            },
            file_picker_config,
        );
    }

    fn check_status_setup(&mut self, agent: CLIAgent, ctx: &mut ViewContext<Self>) {
        self.needs_status_setup = false;
        let Some(status_agent) = StatusAgent::for_cli_agent(agent) else {
            return;
        };
        let Some(directory) = status_agent.default_config_dir() else {
            return;
        };
        ctx.spawn(
            async move { crate::local_hooks::setup_state(status_agent, &directory) },
            |me, state, ctx| {
                me.needs_status_setup =
                    matches!(state, Ok(SetupState::Missing | SetupState::Outdated));
                ctx.notify();
            },
        );
    }

    fn should_show_status_setup_chip(&self, app: &AppContext) -> bool {
        if !self.needs_status_setup {
            return false;
        }
        let Some(session) = CLIAgentSessionsModel::as_ref(app).session(self.terminal_view_id)
        else {
            return false;
        };
        let chip_key = plugin_chip_key(session.agent.command_prefix(), &session.remote_host);
        !session.supports_rich_status()
            && !session.is_remote()
            && !AISettings::as_ref(app).is_plugin_install_chip_dismissed(&chip_key)
    }

    fn render_plugin_chip(&self, chip_kind: PluginChipKind, app: &AppContext) -> Box<dyn Element> {
        let button = match (chip_kind, self.should_use_manual_mode(app)) {
            (PluginChipKind::Install, false) => &self.install_plugin_button,
            (PluginChipKind::Install, true) => &self.plugin_instructions_button,
            (PluginChipKind::Update, false) => &self.update_plugin_button,
            (PluginChipKind::Update, true) => &self.update_instructions_button,
        };
        ChildView::new(button).finish()
    }

    /// Which plugin chip to show, if any.
    fn plugin_chip_kind(&self, app: &AppContext) -> Option<PluginChipKind> {
        #[cfg(target_family = "wasm")]
        {
            let _ = (app, self.plugin_operation_in_progress);
            None
        }
        #[cfg(not(target_family = "wasm"))]
        {
            if self.plugin_operation_in_progress || !FeatureFlag::HOANotifications.is_enabled() {
                return None;
            }

            let ai_settings = AISettings::as_ref(app);
            if !*ai_settings.show_agent_notifications {
                return None;
            }

            let session = CLIAgentSessionsModel::as_ref(app).session(self.terminal_view_id)?;

            let manager = plugin_manager()?;
            let min_version = manager.minimum_plugin_version();
            let chip_key = plugin_chip_key(session.agent.command_prefix(), &session.remote_host);
            // If a structured plugin is connected and this agent supports
            // version-based updates, check the reported version.
            if session.supports_rich_status() && manager.supports_update() {
                let needs_update = match &session.plugin_version {
                    // No version reported = pre-versioning plugin, definitely outdated.
                    None => true,
                    Some(v) => compare_versions(v, min_version).is_lt(),
                };
                if !needs_update {
                    return None;
                }
                // Check update chip dismissal.
                let dismissed_version = ai_settings.plugin_update_chip_dismissed_version(&chip_key);
                if !dismissed_version.is_empty()
                    && compare_versions(dismissed_version, min_version).is_ge()
                {
                    return None;
                }
                return Some(PluginChipKind::Update);
            }

            // For agents without auto-install, wait for the debounce timer
            // before showing the install chip.
            if !manager.can_auto_install() && !self.plugin_chip_ready {
                return None;
            }

            let install_chip_dismissed = ai_settings.is_plugin_install_chip_dismissed(&chip_key);

            // For remote sessions, we can't check the filesystem.
            if session.is_remote() {
                return (!install_chip_dismissed).then_some(PluginChipKind::Install);
            }

            if manager.is_installed() {
                // Installed but no listener yet. Check the on-disk version as a fallback
                // — the plugin may be too old to send structured events.
                if manager.needs_update() {
                    let dismissed_version =
                        ai_settings.plugin_update_chip_dismissed_version(&chip_key);
                    if !dismissed_version.is_empty()
                        && compare_versions(dismissed_version, min_version).is_ge()
                    {
                        return None;
                    }
                    return Some(PluginChipKind::Update);
                }
                // Up to date on disk — wait for the listener to connect.
                return None;
            }

            // Not installed locally.
            (!install_chip_dismissed).then_some(PluginChipKind::Install)
        }
    }

    /// Whether the chip should open the manual instructions modal instead of auto-operating.
    fn should_use_manual_mode(&self, app: &AppContext) -> bool {
        let sessions_model = CLIAgentSessionsModel::as_ref(app);
        let session = match sessions_model.session(self.terminal_view_id) {
            Some(s) => s,
            None => return false,
        };

        // Custom toolbar commands always use manual mode because the user's
        // binary may differ from the agent's standard CLI tool.
        if session.custom_command_prefix.is_some() {
            return true;
        }

        #[cfg(not(target_family = "wasm"))]
        if let Some(manager) = plugin_manager()
            && !manager.can_auto_install()
        {
            return true;
        }
        if session.is_remote() {
            return true;
        }
        // Docker sandbox sessions run inside a container; our auto-install
        // path operates on the host's shell config and would install the
        // plugin in the wrong place. Fall back to the manual-instructions
        // modal so the user can paste the install command into the sandbox
        // PTY and install inside the container.
        //
        // `ShellLaunchData` is only available on native builds; wasm builds
        // can't produce a `DockerSandbox` variant either way.
        #[cfg(not(target_family = "wasm"))]
        {
            let shell_data = {
                let model = self.terminal_model.lock();
                model.active_shell_launch_data().cloned()
            };
            if matches!(shell_data, Some(ShellLaunchData::DockerSandbox { .. })) {
                return true;
            }
        }
        sessions_model.has_plugin_auto_failed(session.agent, &session.remote_host)
    }

    /// Records that the auto plugin operation could not start, shows an error toast,
    /// and re-renders so the chip switches to manual-instructions mode.
    #[cfg(not(target_family = "wasm"))]
    fn record_plugin_auto_failure_and_notify(&mut self, ctx: &mut ViewContext<Self>) {
        if let Some(agent) = self.cli_agent(ctx) {
            let remote_host = CLIAgentSessionsModel::as_ref(ctx)
                .session(self.terminal_view_id)
                .and_then(|s| s.remote_host.clone());
            CLIAgentSessionsModel::handle(ctx).update(ctx, |model, _| {
                model.record_plugin_auto_failure(agent, remote_host);
            });
        }
        let window_id = ctx.window_id();
        ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
            toast_stack.add_ephemeral_toast(
                DismissibleToast::error(
                    "Could not automatically install plugin. \
                     Please click the chip again for manual installation steps."
                        .to_owned(),
                ),
                window_id,
                ctx,
            );
        });
        ctx.notify();
    }

    /// Shared handler for both install and update plugin operations.
    /// `progress_toast` is shown while the operation runs; `success_toast` on success.
    #[cfg(not(target_family = "wasm"))]
    fn handle_plugin_operation<F, Fut>(
        &mut self,
        progress_toast: &str,
        error_label: &str,
        success_toast: &str,
        operation_kind: PluginChipTelemetryKind,
        operation: F,
        ctx: &mut ViewContext<Self>,
    ) -> bool
    where
        F: FnOnce(Box<dyn CliAgentPluginManager>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), PluginInstallError>> + Send + 'static,
    {
        let Some(agent) = self.cli_agent(ctx) else {
            return false;
        };
        let Some(manager) = plugin_manager() else {
            return false;
        };

        self.plugin_operation_in_progress = true;
        ctx.notify();

        let window_id = ctx.window_id();
        let toast_id = "cli-agent-plugin-operation".to_owned();

        ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
            toast_stack.add_persistent_toast(
                DismissibleToast::default(progress_toast.to_owned())
                    .with_object_id(toast_id.clone()),
                window_id,
                ctx,
            );
        });

        let toast_id_for_callback = toast_id.clone();
        let error_label = error_label.to_owned();
        let success_toast = success_toast.to_owned();
        ctx.spawn(
            async move {
                match operation(manager).await {
                    Ok(()) => Ok(()),
                    Err(err) => {
                        let log_path = write_install_log(agent, &err).await;
                        Err((err, log_path))
                    }
                }
            },
            move |me, result, ctx| {
                me.plugin_operation_in_progress = false;

                if result.is_ok() {
                    send_telemetry_from_ctx!(
                        TelemetryEvent::CLIAgentPluginOperationSucceeded {
                            cli_agent: agent,
                            operation: operation_kind,
                        },
                        ctx
                    );
                    ctx.emit(CLIAgentFooterEvent::PluginInstalled(agent));
                } else {
                    send_telemetry_from_ctx!(
                        TelemetryEvent::CLIAgentPluginOperationFailed {
                            cli_agent: agent,
                            operation: operation_kind,
                        },
                        ctx
                    );
                }

                ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                    let toast = match result {
                        Ok(()) => DismissibleToast::success(success_toast.clone()),
                        Err((err, log_path)) => {
                            let remote_host = CLIAgentSessionsModel::as_ref(ctx)
                                .session(me.terminal_view_id)
                                .and_then(|s| s.remote_host.clone());
                            CLIAgentSessionsModel::handle(ctx).update(ctx, |model, _| {
                                model.record_plugin_auto_failure(agent, remote_host);
                            });
                            log::error!("Failed plugin operation log: {}", err.log);
                            let mut toast =
                                DismissibleToast::error(format!("{error_label}: {err}"));
                            report_error!(
                                anyhow::Error::new(err).context("Failed plugin operation"),
                                extra: { "agent" => ?agent }
                            );
                            if let Some(log_path) = log_path {
                                toast = toast.with_link(
                                    ToastLink::new("See logs for details".to_owned())
                                        .with_onclick_action(WorkspaceAction::OpenFilePath {
                                            path: log_path,
                                        }),
                                );
                            }
                            toast
                        }
                    };
                    toast_stack.add_ephemeral_toast(
                        toast.with_object_id(toast_id_for_callback),
                        window_id,
                        ctx,
                    );
                });
                ctx.notify();
            },
        );
        true
    }

    #[cfg(not(target_family = "wasm"))]
    fn handle_install_plugin(&mut self, ctx: &mut ViewContext<Self>) -> bool {
        let success_msg = self
            .cli_agent(ctx)
            .and(plugin_manager())
            .map(|m| m.install_success_message())
            .unwrap_or("Torch plugin installed. Please restart the session to activate.");
        self.handle_plugin_operation(
            "Installing Torch plugin...",
            "Failed to install Torch plugin",
            success_msg,
            PluginChipTelemetryKind::Install,
            |manager| async move { manager.install().await },
            ctx,
        )
    }

    #[cfg(not(target_family = "wasm"))]
    fn handle_update_plugin(&mut self, ctx: &mut ViewContext<Self>) -> bool {
        let success_msg = self
            .cli_agent(ctx)
            .and(plugin_manager())
            .map(|m| m.update_success_message())
            .unwrap_or("Torch plugin updated. Please restart the session to activate.");
        self.handle_plugin_operation(
            "Updating Torch plugin...",
            "Failed to update Torch plugin",
            success_msg,
            PluginChipTelemetryKind::Update,
            |manager| async move { manager.update().await },
            ctx,
        )
    }

    fn cli_display_chip(
        &self,
        chip_kind: ContextChipKind,
        app: &AppContext,
    ) -> Option<Box<dyn Element>> {
        self.cli_display_chips
            .iter()
            .find(|chip| chip.as_ref(app).chip_kind() == &chip_kind)
            .filter(|chip| chip.as_ref(app).should_render(app))
            .map(|chip| ChildView::new(chip).finish())
    }

    fn render_toolbar_item(
        &self,
        item: &AgentToolbarItemKind,
        app: &AppContext,
    ) -> Option<Box<dyn Element>> {
        match item {
            AgentToolbarItemKind::ContextChip(chip_kind) => {
                self.cli_display_chip(chip_kind.clone(), app)
            }
            AgentToolbarItemKind::FileExplorer => item
                .is_available(app)
                .then(|| ChildView::new(&self.file_explorer_button).finish()),
            AgentToolbarItemKind::RichInput => FeatureFlag::CLIAgentRichInput
                .is_enabled()
                .then(|| ChildView::new(&self.rich_input_button).finish()),
            AgentToolbarItemKind::FileAttach => Some(ChildView::new(&self.file_button).finish()),
            AgentToolbarItemKind::Settings => Some(ChildView::new(&self.settings_button).finish()),
        }
    }

    fn render_footer(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let cli_icon_size = ButtonSize::AgentInputButton.icon_size(appearance, app);

        // Extract everything we need from the terminal model up front and drop
        // the lock before calling into helpers like `should_use_manual_mode`
        // and `render_cli_toolbar_item`, which may re-lock the same model and
        // would deadlock since the lock is non-reentrant.
        let background_color = {
            let terminal_model = self.terminal_model.lock();
            if terminal_model.is_alt_screen_active() {
                terminal_model
                    .alt_screen()
                    .inferred_bg_color()
                    .unwrap_or_else(|| appearance.theme().surface_1().into_solid())
            } else {
                appearance.theme().surface_1().into_solid()
            }
        };

        let session_settings = SessionSettings::as_ref(app);
        let left_items = session_settings
            .cli_agent_footer_chip_selection
            .left_items();
        let right_items = session_settings
            .cli_agent_footer_chip_selection
            .right_items();

        let mut left_buttons = Wrap::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_run_spacing(4.)
            .with_spacing(4.);

        // CLI agent brand icon is always rendered (not configurable).
        if let Some(agent) = self.cli_agent(app)
            && let Some(icon) = agent.icon()
        {
            let icon_color = agent
                .brand_color()
                .map(|c| c.on_background(background_color, MinimumAllowedContrast::NonText))
                .unwrap_or_else(|| appearance.theme().foreground().into_solid());
            left_buttons.add_child(
                Container::new(
                    ConstrainedBox::new(icon.to_warpui_icon(Fill::Solid(icon_color)).finish())
                        .with_width(cli_icon_size)
                        .with_height(cli_icon_size)
                        .finish(),
                )
                .with_padding_right(8.)
                .finish(),
            );
        }

        let plugin_chip = if self.should_show_status_setup_chip(app) {
            Some(ChildView::new(&self.status_setup_button).finish())
        } else {
            self.plugin_chip_kind(app)
                .map(|chip_kind| self.render_plugin_chip(chip_kind, app))
        };
        if let Some(chip) = plugin_chip {
            let chip_with_dismiss = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(chip)
                .with_child(ChildView::new(&self.dismiss_plugin_chip_button).finish())
                .finish();
            left_buttons.add_child(chip_with_dismiss);
        }

        for item in &left_items {
            if let Some(element) = self.render_toolbar_item(item, app) {
                left_buttons.add_child(element);
            }
        }

        let mut right_buttons = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Min)
            .with_spacing(4.);

        for item in &right_items {
            if let Some(element) = self.render_toolbar_item(item, app) {
                right_buttons.add_child(element);
            }
        }

        let content = Wrap::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(WrapFillEntireRun::new(left_buttons.finish()).finish())
            .with_child(WrapFill::new(0., right_buttons.finish()).finish())
            .with_run_spacing(context_chips::spacing::UDI_ROW_RUN_SPACING)
            .finish();
        let content = EventHandler::new(content)
            .on_right_mouse_down(|ctx, _, position, _| {
                ctx.dispatch_typed_action(CLIAgentFooterAction::ShowContextMenu { position });
                DispatchEventResult::StopPropagation
            })
            .finish();

        Container::new(content).with_vertical_padding(4.).finish()
    }

    pub fn has_open_chip_menu(&self, app: &AppContext) -> bool {
        self.cli_display_chips
            .iter()
            .any(|chip| chip.as_ref(app).display_chip_kind().has_open_menu())
    }

    #[cfg(test)]
    pub fn cli_display_chip_kinds(
        &self,
        app: &AppContext,
    ) -> Vec<crate::context_chips::ContextChipKind> {
        self.cli_display_chips
            .iter()
            .map(|chip| chip.as_ref(app).chip_kind().clone())
            .collect()
    }
}

impl View for CLIAgentFooter {
    fn ui_name() -> &'static str {
        "CLIAgentFooter"
    }

    fn render(&self, app: &torchui::AppContext) -> Box<dyn torchui::Element> {
        if !self.is_cli_agent_session_active(app) {
            return Empty::new().finish();
        }
        self.render_footer(app)
    }
}

#[derive(Debug, Clone)]
pub enum CLIAgentFooterAction {
    SelectFile,
    InsertFilePath(String),
    ToggleFileExplorer,
    ToggleRichInput,
    InstallPlugin,
    UpdatePlugin,
    OpenPluginInstallInstructionsPane,
    OpenPluginUpdateInstructionsPane,
    DismissPluginChip,
    OpenCodingAgentSettings,
    OpenStatusSetup,
    ShowContextMenu { position: Vector2F },
}

impl TypedActionView for CLIAgentFooter {
    type Action = CLIAgentFooterAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut torchui::ViewContext<Self>) {
        match action {
            CLIAgentFooterAction::SelectFile => {
                self.select_file(ctx);
            }
            CLIAgentFooterAction::InsertFilePath(path) => {
                if let Some(agent) = self.cli_agent(ctx) {
                    send_telemetry_from_ctx!(
                        TelemetryEvent::CLIAgentToolbarImageAttached { cli_agent: agent },
                        ctx
                    );
                }
                let path_with_space = format!("{path} ");
                if self.has_active_cli_agent_input_session(ctx) {
                    ctx.emit(CLIAgentFooterEvent::InsertIntoCLIRichInput(path_with_space));
                } else {
                    ctx.emit(CLIAgentFooterEvent::WriteToPty(path_with_space));
                }
            }
            CLIAgentFooterAction::ToggleFileExplorer => {
                ctx.emit(CLIAgentFooterEvent::ToggleFileExplorer(self.cli_agent(ctx)));
            }
            CLIAgentFooterAction::ToggleRichInput => {
                if self.has_active_cli_agent_input_session(ctx) {
                    ctx.emit(CLIAgentFooterEvent::HideRichInput);
                } else {
                    ctx.emit(CLIAgentFooterEvent::OpenRichInput);
                }
            }
            CLIAgentFooterAction::InstallPlugin => {
                #[cfg(not(target_family = "wasm"))]
                {
                    if let Some(agent) = self.cli_agent(ctx) {
                        send_telemetry_from_ctx!(
                            TelemetryEvent::CLIAgentPluginChipClicked {
                                cli_agent: agent,
                                action: PluginChipTelemetryAction::Install,
                            },
                            ctx
                        );
                    }
                    if !self.handle_install_plugin(ctx) {
                        self.record_plugin_auto_failure_and_notify(ctx);
                    }
                }
            }
            CLIAgentFooterAction::UpdatePlugin => {
                #[cfg(not(target_family = "wasm"))]
                {
                    if let Some(agent) = self.cli_agent(ctx) {
                        send_telemetry_from_ctx!(
                            TelemetryEvent::CLIAgentPluginChipClicked {
                                cli_agent: agent,
                                action: PluginChipTelemetryAction::Update,
                            },
                            ctx
                        );
                    }
                    if !self.handle_update_plugin(ctx) {
                        self.record_plugin_auto_failure_and_notify(ctx);
                    }
                }
            }
            CLIAgentFooterAction::OpenPluginInstallInstructionsPane => {
                #[cfg(not(target_family = "wasm"))]
                if let Some(agent) = self.cli_agent(ctx) {
                    send_telemetry_from_ctx!(
                        TelemetryEvent::CLIAgentPluginChipClicked {
                            cli_agent: agent,
                            action: PluginChipTelemetryAction::InstallInstructions,
                        },
                        ctx
                    );
                    ctx.emit(CLIAgentFooterEvent::OpenPluginInstructionsPane(
                        agent,
                        PluginModalKind::Install,
                    ));
                }
            }
            CLIAgentFooterAction::OpenPluginUpdateInstructionsPane => {
                #[cfg(not(target_family = "wasm"))]
                if let Some(agent) = self.cli_agent(ctx) {
                    send_telemetry_from_ctx!(
                        TelemetryEvent::CLIAgentPluginChipClicked {
                            cli_agent: agent,
                            action: PluginChipTelemetryAction::UpdateInstructions,
                        },
                        ctx
                    );
                    ctx.emit(CLIAgentFooterEvent::OpenPluginInstructionsPane(
                        agent,
                        PluginModalKind::Update,
                    ));
                }
            }
            CLIAgentFooterAction::DismissPluginChip => {
                let chip_kind = self.plugin_chip_kind(ctx);
                let is_update = matches!(chip_kind, Some(PluginChipKind::Update));
                if let Some(agent) = self.cli_agent(ctx)
                    && let Some(kind) = chip_kind
                {
                    send_telemetry_from_ctx!(
                        TelemetryEvent::CLIAgentPluginChipDismissed {
                            cli_agent: agent,
                            chip_kind: kind.into(),
                        },
                        ctx
                    );
                }
                let session = CLIAgentSessionsModel::as_ref(ctx)
                    .session(self.terminal_view_id)
                    .cloned();
                if let Some(session) = session {
                    let chip_key =
                        plugin_chip_key(session.agent.command_prefix(), &session.remote_host);
                    if is_update {
                        #[cfg(not(target_family = "wasm"))]
                        if let Some(manager) = plugin_manager() {
                            let version = manager.minimum_plugin_version().to_owned();
                            AISettings::handle(ctx).update(ctx, |settings, ctx| {
                                settings.dismiss_plugin_update_chip(&chip_key, version, ctx);
                            });
                        }
                    } else {
                        AISettings::handle(ctx).update(ctx, |settings, ctx| {
                            settings.dismiss_plugin_install_chip(&chip_key, ctx);
                        });
                    }
                }
                ctx.notify();
            }
            CLIAgentFooterAction::OpenCodingAgentSettings => {
                #[cfg(not(target_family = "wasm"))]
                ctx.dispatch_typed_action_deferred(WorkspaceAction::ScrollToSettingsWidget {
                    page: SettingsSection::ThirdPartyCLIAgents,
                    widget_id: crate::settings_view::cli_agent_settings_widget_id(),
                });
            }
            CLIAgentFooterAction::OpenStatusSetup => {
                #[cfg(not(target_family = "wasm"))]
                if let Some(agent) = self.cli_agent(ctx).and_then(StatusAgent::for_cli_agent) {
                    ctx.dispatch_typed_action_deferred(WorkspaceAction::ScrollToSettingsWidget {
                        page: SettingsSection::ThirdPartyCLIAgents,
                        widget_id: crate::settings_view::agent_status_widget_id(agent),
                    });
                }
            }
            CLIAgentFooterAction::ShowContextMenu { position } => {
                ctx.emit(CLIAgentFooterEvent::ShowContextMenu {
                    position: *position,
                });
            }
        }
    }
}

pub enum CLIAgentFooterEvent {
    WriteToPty(String),
    /// Insert text into the CLI agent's PTY input using its paste strategy.
    InsertIntoCLIPty(String),
    /// Insert text into the CLI agent rich input.
    InsertIntoCLIRichInput(String),
    /// Toggle the file explorer side panel. `None` when no CLI agent session is
    /// attached to this pane.
    ToggleFileExplorer(Option<CLIAgent>),
    OpenRichInput,
    HideRichInput,
    ToggledChipMenu {
        open: bool,
    },
    TryExecuteChipCommand(PromptChipShellCommand),
    OpenCodeReview,
    ShowContextMenu {
        position: Vector2F,
    },
    PluginInstalled(CLIAgent),
    #[cfg(not(target_family = "wasm"))]
    OpenPluginInstructionsPane(CLIAgent, PluginModalKind),
}

impl Entity for CLIAgentFooter {
    type Event = CLIAgentFooterEvent;
}

pub(crate) struct AgentInputButtonTheme;

impl ActionButtonTheme for AgentInputButtonTheme {
    fn background(&self, hovered: bool, appearance: &Appearance) -> Option<Fill> {
        // Solid surface fills keep the button readable even when its parent
        // isn't `theme.background()` (for example, over an alt-screen CLI agent).
        let theme = appearance.theme();
        Some(if hovered {
            theme.surface_2()
        } else {
            theme.surface_1()
        })
    }

    fn text_color(
        &self,
        _hovered: bool,
        background: Option<Fill>,
        appearance: &Appearance,
    ) -> ColorU {
        // If a caller overrides `background()` with a translucent fill, blend
        // it over `surface_1` so text contrast is computed against the actual
        // rendered color rather than the raw overlay.
        let base_bg = appearance.theme().surface_1();
        let effective_bg = background
            .map(|overlay| base_bg.blend(&overlay))
            .unwrap_or(base_bg);

        appearance.theme().sub_text_color(effective_bg).into_solid()
    }

    fn border(&self, appearance: &Appearance) -> Option<ColorU> {
        Some(internal_colors::neutral_3(appearance.theme()))
    }

    fn should_opt_out_of_contrast_adjustment(&self) -> bool {
        true
    }
}

/// Green-accented theme for the "Install Torch plugin" chip.
struct InstallPluginButtonTheme;

impl ActionButtonTheme for InstallPluginButtonTheme {
    fn background(&self, hovered: bool, appearance: &Appearance) -> Option<Fill> {
        let green = appearance.theme().ansi_fg_green();
        let base = appearance.theme().surface_1();
        Some(if hovered {
            base.blend(&Fill::Solid(green).with_opacity(30))
        } else {
            base.blend(&Fill::Solid(green).with_opacity(15))
        })
    }

    fn text_color(
        &self,
        _hovered: bool,
        _background: Option<Fill>,
        appearance: &Appearance,
    ) -> ColorU {
        appearance.theme().ansi_fg_green()
    }

    fn border(&self, appearance: &Appearance) -> Option<ColorU> {
        let green = appearance.theme().ansi_fg_green();
        Some(ColorU::new(green.r, green.g, green.b, 80))
    }

    fn should_opt_out_of_contrast_adjustment(&self) -> bool {
        true
    }
}

/// Writes the detailed plugin installation log to a temp file.
/// Returns the log file path on success, or `None` if writing failed.
#[cfg(not(target_family = "wasm"))]
async fn write_install_log(agent: CLIAgent, err: &PluginInstallError) -> Option<PathBuf> {
    let log_path = env::temp_dir().join("warp-plugin-install.log");
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    let contents = format!(
        "Torch plugin installation — {agent:?}\n\
         {now}\n\
         \n\
         {log}",
        log = err.log,
    );
    fs::write(&log_path, contents).await.ok()?;
    Some(log_path)
}
