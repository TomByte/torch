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

use ai::document::{AIDocumentId, AIDocumentVersion};
use chrono::{DateTime, Local};
use parking_lot::FairMutex;
use pathfinder_color::ColorU;
use pathfinder_geometry::vector::{Vector2F, vec2f};
#[cfg(feature = "voice_input")]
use settings::Setting;
use settings::ToggleableSetting;
#[cfg(not(target_family = "wasm"))]
use tokio::fs;
use toolbar_item::AgentToolbarItemKind;
use torch_core::channel::ChannelState;
use torch_core::ui::color::ContrastingColor;
use torch_core::ui::color::blend::Blend;
use torch_core::ui::color::contrast::MinimumAllowedContrast;
use torch_core::ui::theme::color::internal_colors;
use torch_core::ui::theme::{AnsiColorIdentifier, Fill};
#[cfg(any(not(target_family = "wasm"), feature = "voice_input"))]
use torch_errors::report_error;
use torch_errors::report_if_error;
use torchui::r#async::{SpawnedFutureHandle, Timer};
use torchui::elements::{
    ChildAnchor, ChildView, Clipped, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment,
    DispatchEventResult, Element, Empty, EventHandler, Flex, MainAxisAlignment, MainAxisSize,
    OffsetPositioning, ParentAnchor, ParentElement, ParentOffsetBounds, Radius, SavePosition,
    Shrinkable, Stack, Wrap, WrapFill, WrapFillEntireRun,
};
use torchui::{
    AppContext, Entity, EntityId, ModelHandle, SingletonEntity, TypedActionView, View, ViewContext,
    ViewHandle,
};
#[cfg(feature = "voice_input")]
use voice_input::{
    StartListeningError, VoiceInputLifecycle, VoiceInputLifecycleState, VoiceSessionResult,
};

use crate::ai::AIRequestUsageModel;
use crate::ai::blocklist::BlocklistAIInputModel;
use crate::ai::blocklist::history_model::{BlocklistAIHistoryEvent, BlocklistAIHistoryModel};
use crate::ai::blocklist::prompt::prompt_alert::{PromptAlertEvent, PromptAlertView};
use crate::ai::blocklist::usage::icon_for_context_window_usage;
use crate::ai::blocklist::usage::usage_popover_view::{
    UsagePopoverEvent, UsagePopoverView, conversation_total_text, conversation_usage_display_unit,
};
use crate::ai::execution_profiles::profiles::AIExecutionProfilesModel;
use crate::appearance::Appearance;
use crate::completer::SessionContext;
use crate::context_chips::display_chip::{DisplayChip, DisplayChipConfig, PromptChipShellCommand};
use crate::context_chips::prompt_type::PromptType;
use crate::context_chips::{self, ContextChipKind};
use crate::features::FeatureFlag;
use crate::local_hooks::{SetupState, StatusAgent};
use crate::network::NetworkStatus;
use crate::send_telemetry_from_ctx;
#[cfg(feature = "voice_input")]
use crate::server::server_api::TranscribeError;
#[cfg(feature = "voice_input")]
use crate::server::team_scope::RequestTeamScope;
#[cfg(not(target_family = "wasm"))]
use crate::server::telemetry::PluginChipTelemetryAction;
use crate::server::telemetry::{PluginChipTelemetryKind, TelemetryEvent};
use crate::settings::{
    AISettings, AISettingsChangedEvent, CodeSettings, CodeSettingsChangedEvent, PrivacySettings,
    PrivacySettingsChangedEvent,
};
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
use crate::terminal::input::models::InlineModelSelectorTab;
use crate::terminal::input::{MenuPositioning, MenuPositioningProvider};
use crate::terminal::profile_model_selector::{ProfileModelSelector, ProfileModelSelectorEvent};
use crate::terminal::session_settings::{
    SessionSettings, SessionSettingsChangedEvent, ToolbarChipSelection,
};
use crate::terminal::view::TerminalAction;
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
use crate::workspaces::user_workspaces::{UserWorkspaces, UserWorkspacesEvent};
use crate::workspaces::workspace::ChargeUnit;

const ENABLE_NLD_TOOLTIP: &str = "Enable terminal command autodetection";
const DISABLE_NLD_TOOLTIP: &str = "Disable terminal command autodetection";

const FAST_FORWARD_ON_TOOLTIP: &str = "Turn off auto-approve all agent actions";
const FAST_FORWARD_OFF_TOOLTIP: &str = "Auto-approve all agent actions for this task";

/// id for the conversation usage popover's trigger button to anchor the popover overlay
const USAGE_BUTTON_SAVE_POSITION_ID: &str = "agent_input_footer::usage_button";

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

/// Footer control bar at the bottom of the agent input.
///
/// Renders in two modes:
/// - **Agent View mode** (default): model selector, NLD toggle, chips, etc.
/// - **CLI agent mode**: agent icon, image, mic, file explorer, view changes, rich input.
///
/// The mode is determined by reading `CLIAgentSessionsModel` at render time.
/// A single `ViewHandle<AgentInputFooter>` is shared between `Input` and
/// `UseAgentToolbar`, rendering the appropriate mode in each context.
pub struct AgentInputFooter {
    terminal_view_id: EntityId,
    #[cfg_attr(not(feature = "voice_input"), allow(unused))]
    mic_button: ViewHandle<ActionButton>,
    nld_button: ViewHandle<ActionButton>,
    file_button: ViewHandle<ActionButton>,
    context_window_button: ViewHandle<ActionButton>,
    usage_button: ViewHandle<ActionButton>,
    model_selector: ViewHandle<ProfileModelSelector>,
    prompt_alert: ViewHandle<PromptAlertView>,
    left_display_chips: Vec<ViewHandle<DisplayChip>>,
    right_display_chips: Vec<ViewHandle<DisplayChip>>,
    // Separate set of display chips for the CLI agent footer.
    // Needed because the CLI footer chip selection can include chips not present in the agent view selection.
    cli_display_chips: Vec<ViewHandle<DisplayChip>>,
    display_chip_config: DisplayChipConfig,

    terminal_model: Arc<FairMutex<TerminalModel>>,

    /// Opens the file explorer side panel. Available in both footers, but only
    /// present in the CLI agent toolbar by default.
    file_explorer_button: ViewHandle<ActionButton>,

    // CLI agent-specific buttons (rendered when a CLI agent session is active).
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

    // Fast-forward (auto-approve) toggle button shown in the agent view footer.
    fast_forward_button: ViewHandle<ActionButton>,

    // CLI agent voice input state (self-contained, bypasses editor voice flow).
    #[cfg(feature = "voice_input")]
    cli_voice_input_lifecycle: VoiceInputLifecycle,
    #[cfg(feature = "voice_input")]
    cli_recording_handle: Option<SpawnedFutureHandle>,
    #[cfg(feature = "voice_input")]
    cli_transcription_handle: Option<SpawnedFutureHandle>,

    /// Pending one-shot timer that refreshes the context-window button at the
    /// prompt-cache expiry instant so the notification dot appears while idle.
    prompt_cache_expiry_timer_handle: Option<SpawnedFutureHandle>,

    /// Whether the active conversation's prompt cache has expired. Drives the
    /// yellow notification dot on the context-window chip when the
    /// `PromptCacheExpiryWarning` flag is enabled.
    prompt_cache_expired: bool,

    /// Used to anchor the usage popover above or below the input box, matching the
    /// surrounding menu positioning.
    menu_positioning_provider: Arc<dyn MenuPositioningProvider>,
    usage_popover: ViewHandle<UsagePopoverView>,
    usage_popover_open: bool,
}

impl AgentInputFooter {
    pub fn new(
        menu_positioning_provider: Arc<dyn MenuPositioningProvider>,
        terminal_view_id: EntityId,
        ai_input_model: ModelHandle<BlocklistAIInputModel>,
        terminal_model: Arc<FairMutex<TerminalModel>>,
        prompt: ModelHandle<PromptType>,
        display_chip_config: DisplayChipConfig,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let button_size = ButtonSize::AgentInputButton;

        let nld_button = ctx.add_typed_action_view(|ctx| {
            let is_nld_enabled = AISettings::as_ref(ctx).is_ai_autodetection_enabled(ctx);
            let mut button = ActionButton::new("", NLDButtonTheme)
                .with_icon(Icon::NLD)
                .with_size(button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(AgentInputFooterAction::ToggleAutodetectionSetting);
                });
            button.set_active(is_nld_enabled, ctx);
            button.set_tooltip(
                Some(if is_nld_enabled {
                    DISABLE_NLD_TOOLTIP
                } else {
                    ENABLE_NLD_TOOLTIP
                }),
                ctx,
            );
            button
        });
        ctx.subscribe_to_model(&AISettings::handle(ctx), |me, settings, event, ctx| {
            let AISettingsChangedEvent::AIAutoDetectionEnabled { .. } = event else {
                return;
            };
            let is_nld_enabled = settings.as_ref(ctx).is_ai_autodetection_enabled(ctx);
            me.nld_button.update(ctx, |button, ctx| {
                button.set_active(is_nld_enabled, ctx);
                button.set_tooltip(
                    Some(if is_nld_enabled {
                        DISABLE_NLD_TOOLTIP
                    } else {
                        ENABLE_NLD_TOOLTIP
                    }),
                    ctx,
                );
            });
        });

        let mic_button = ctx.add_typed_action_view(|_ctx| {
            let button = ActionButton::new("", ActiveMicButtonTheme)
                .with_icon(Icon::Microphone)
                .with_tooltip("Voice input")
                .with_size(button_size)
                .with_tooltip_alignment(TooltipAlignment::Left);
            #[cfg(feature = "voice_input")]
            let button = button.on_click(|ctx| {
                ctx.dispatch_typed_action(AgentInputFooterAction::ToggleVoiceInput);
            });
            button
        });

        #[cfg(feature = "voice_input")]
        {
            let tooltip = AISettings::as_ref(ctx)
                .voice_input_toggle_key
                .value()
                .tooltip_message();
            mic_button.update(ctx, |button, ctx| {
                button.set_tooltip(Some(tooltip), ctx);
            });

            ctx.subscribe_to_model(&AISettings::handle(ctx), |me, _, event, ctx| {
                if let AISettingsChangedEvent::VoiceInputToggleKey { .. } = event {
                    let tooltip = AISettings::as_ref(ctx)
                        .voice_input_toggle_key
                        .value()
                        .tooltip_message();
                    me.mic_button.update(ctx, |button, ctx| {
                        button.set_tooltip(Some(tooltip), ctx);
                    });
                }
            });
        }

        let file_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("", AgentInputButtonTheme)
                .with_icon(Icon::Plus)
                .with_tooltip("Attach file")
                .with_tooltip_keybinding(ATTACH_FILE_KEYBINDING)
                .with_size(button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(AgentInputFooterAction::SelectFile);
                })
        });

        // Fast-forward (auto-approve) toggle button.
        // Uses FastForwardButtonTheme so the button keeps its one-off semantics.
        // The theme still delegates its fill to the shared chip background.
        let fast_forward_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("", FastForwardButtonTheme)
                .with_icon(Icon::FastForward)
                .with_tooltip(FAST_FORWARD_OFF_TOOLTIP)
                .with_size(button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(TerminalAction::ToggleAutoexecuteMode);
                })
        });

        let cli_button_size = ButtonSize::AgentInputButton;
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
                    ctx.dispatch_typed_action(AgentInputFooterAction::ToggleFileExplorer);
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
                    ctx.dispatch_typed_action(AgentInputFooterAction::ToggleRichInput);
                })
        });
        let settings_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("", AgentInputButtonTheme)
                .with_icon(Icon::Settings)
                .with_tooltip("Open coding agent settings")
                .with_size(cli_button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(AgentInputFooterAction::OpenCodingAgentSettings);
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
                    ctx.dispatch_typed_action(AgentInputFooterAction::InstallPlugin);
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
                        AgentInputFooterAction::OpenPluginInstallInstructionsPane,
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
                    ctx.dispatch_typed_action(AgentInputFooterAction::UpdatePlugin);
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
                        AgentInputFooterAction::OpenPluginUpdateInstructionsPane,
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
                    ctx.dispatch_typed_action(AgentInputFooterAction::DismissPluginChip);
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
                    ctx.dispatch_typed_action(AgentInputFooterAction::OpenStatusSetup);
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
                    #[cfg(feature = "voice_input")]
                    me.stop_cli_voice_and_reset(ctx);
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

        let context_window_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("", AgentInputButtonTheme)
                .with_icon(Icon::ContextRemaining100)
                .with_tooltip("Context window usage")
                .with_size(button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
        });

        let usage_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("", AgentInputButtonTheme)
                .with_icon(Icon::PieChart)
                .with_size(button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(AgentInputFooterAction::ToggleUsagePopover);
                })
        });

        let usage_popover = ctx.add_typed_action_view(|ctx| UsagePopoverView::new(None, ctx));
        ctx.subscribe_to_view(&usage_popover, |me, _, event, ctx| match event {
            UsagePopoverEvent::Close => {
                me.usage_popover_open = false;
                ctx.notify();
            }
        });

        let profile_model_selector_full = ctx.add_typed_action_view(|ctx| {
            let mut selector = ProfileModelSelector::new(
                menu_positioning_provider.clone(),
                terminal_view_id,
                ai_input_model,
                terminal_model.clone(),
                None,
                ctx,
            );
            selector.set_render_compact(false, ctx);
            selector
        });

        ctx.subscribe_to_view(&profile_model_selector_full, |me, _, event, ctx| {
            me.handle_profile_model_selector_event(event, ctx);
        });

        let prompt_alert = ctx.add_typed_action_view(PromptAlertView::new);
        ctx.subscribe_to_view(&prompt_alert, |_, _, event, ctx| {
            ctx.emit(AgentInputFooterEvent::PromptAlert(event.clone()));
        });

        ctx.subscribe_to_model(&NetworkStatus::handle(ctx), |_, _, _, ctx| {
            ctx.notify();
        });
        ctx.subscribe_to_model(&UserWorkspaces::handle(ctx), |me, _, event, ctx| {
            // The tier carries the unit the usage tooltip renders in.
            if matches!(
                event,
                UserWorkspacesEvent::TeamsChanged | UserWorkspacesEvent::CurrentWorkspaceChanged
            ) {
                me.update_usage_button(ctx);
            }
            ctx.notify();
        });
        ctx.subscribe_to_model(&AIRequestUsageModel::handle(ctx), |_, _, _, ctx| {
            ctx.notify()
        });
        ctx.subscribe_to_model(&AISettings::handle(ctx), |me, _, event, ctx| {
            if matches!(event, AISettingsChangedEvent::UsageDisplayUnit { .. }) {
                me.update_usage_button(ctx);
                ctx.notify()
            } else if matches!(event, AISettingsChangedEvent::AIAutoDetectionEnabled { .. }) {
                ctx.notify()
            }
        });
        ctx.subscribe_to_model(&PrivacySettings::handle(ctx), |_, _, event, ctx| {
            if matches!(
                event,
                PrivacySettingsChangedEvent::UpdateIsCloudConversationStorageEnabled { .. }
            ) {
                ctx.notify()
            }
        });
        // The File explorer item's availability follows this setting, so the footer has to
        // repaint when it is toggled rather than waiting for an unrelated re-render.
        ctx.subscribe_to_model(&CodeSettings::handle(ctx), |_, _, event, ctx| {
            if matches!(event, CodeSettingsChangedEvent::ShowProjectExplorer { .. }) {
                ctx.notify()
            }
        });
        ctx.subscribe_to_model(
            &display_chip_config.agent_view_controller,
            |me, _, _, ctx| {
                me.sync_fast_forward_button(ctx);
            },
        );

        let prompt_for_session_settings = prompt.clone();
        ctx.subscribe_to_model(
            &SessionSettings::handle(ctx),
            move |me, _, event, ctx| match event {
                SessionSettingsChangedEvent::ShowModelSelectorsInPrompt { .. } => {
                    ctx.notify();
                }
                SessionSettingsChangedEvent::AgentToolbarChipSelectionSetting { .. }
                | SessionSettingsChangedEvent::CLIAgentToolbarChipSelectionSetting { .. }
                | SessionSettingsChangedEvent::GithubPrChipDefaultValidation { .. } => {
                    me.update_display_chips(&prompt_for_session_settings, ctx);
                    ctx.notify();
                }
                _ => {}
            },
        );
        // Subscribe to AIExecutionProfilesModel to potentially show/hide the profile selector button when profiles are added/removed
        ctx.subscribe_to_model(&AIExecutionProfilesModel::handle(ctx), |_, _, _, ctx| {
            ctx.notify();
        });

        ctx.subscribe_to_model(
            &BlocklistAIHistoryModel::handle(ctx),
            |me, _, event, ctx| {
                if event
                    .terminal_surface_id()
                    .is_some_and(|id| id != me.terminal_view_id)
                {
                    return;
                }

                match event {
                    BlocklistAIHistoryEvent::StartedNewConversation { .. }
                    | BlocklistAIHistoryEvent::SetActiveConversation { .. }
                    | BlocklistAIHistoryEvent::ClearedActiveConversation { .. }
                    | BlocklistAIHistoryEvent::ClearedConversationsForTerminalSurface { .. }
                    | BlocklistAIHistoryEvent::RemoveConversation { .. }
                    | BlocklistAIHistoryEvent::UpdatedAutoexecuteOverride { .. } => {
                        me.sync_fast_forward_button(ctx);
                        me.update_context_window_button(ctx);
                        me.update_usage_button(ctx);
                        me.retarget_usage_popover_if_open(ctx);
                        me.model_selector.update(ctx, |_, ctx| ctx.notify());
                        ctx.notify();
                    }
                    BlocklistAIHistoryEvent::ConversationUsageMetadataUpdated {
                        conversation_id,
                    }
                    | BlocklistAIHistoryEvent::UpdatedConversationMetadata {
                        conversation_id,
                        ..
                    } => {
                        // Only the active conversation's usage affects this
                        // footer's figures, and a metadata-only event can flip
                        // the usage item's visibility, so the footer must
                        // repaint too.
                        let is_active_conversation = BlocklistAIHistoryModel::as_ref(ctx)
                            .active_conversation(me.terminal_view_id)
                            .is_some_and(|conversation| conversation.id() == *conversation_id);
                        if is_active_conversation {
                            me.update_usage_button(ctx);
                            ctx.notify();
                        }
                    }
                    BlocklistAIHistoryEvent::UpdatedTodoList { .. }
                    | BlocklistAIHistoryEvent::UpdatedConversationStatus { .. }
                    | BlocklistAIHistoryEvent::AppendedExchange { .. }
                    | BlocklistAIHistoryEvent::UpdatedStreamingExchange { .. } => {
                        me.update_context_window_button(ctx);
                        me.update_usage_button(ctx);
                        me.model_selector.update(ctx, |_, ctx| ctx.notify());
                        ctx.notify();
                    }
                    _ => (),
                }
            },
        );

        ctx.observe(&prompt, |me, model, ctx| {
            me.update_display_chips(&model, ctx);
        });

        let mut me = Self {
            terminal_view_id,
            nld_button,
            mic_button,
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
            context_window_button,
            usage_button,
            model_selector: profile_model_selector_full,
            prompt_alert,
            terminal_model,
            left_display_chips: vec![],
            right_display_chips: vec![],
            cli_display_chips: vec![],
            display_chip_config,
            fast_forward_button,
            #[cfg(feature = "voice_input")]
            cli_voice_input_lifecycle: VoiceInputLifecycle::default(),
            #[cfg(feature = "voice_input")]
            cli_recording_handle: None,
            #[cfg(feature = "voice_input")]
            cli_transcription_handle: None,
            prompt_cache_expiry_timer_handle: None,
            prompt_cache_expired: false,
            menu_positioning_provider: menu_positioning_provider.clone(),
            usage_popover,
            usage_popover_open: false,
        };
        me.sync_fast_forward_button(ctx);
        me.update_context_window_button(ctx);
        me.update_usage_button(ctx);
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

    fn all_display_chips(&self) -> impl Iterator<Item = &ViewHandle<DisplayChip>> {
        self.left_display_chips
            .iter()
            .chain(self.right_display_chips.iter())
            .chain(self.cli_display_chips.iter())
    }

    pub fn update_session_context(
        &mut self,
        session_context: Option<SessionContext>,
        ctx: &mut ViewContext<Self>,
    ) {
        self.display_chip_config.session_context = session_context.clone();
        for chip_view in self.all_display_chips() {
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
        if self.is_cli_agent_session_active(ctx) {
            self.select_cli_file(ctx);
        } else {
            ctx.emit(AgentInputFooterEvent::SelectFile);
        }
    }

    fn select_cli_file(&mut self, ctx: &mut ViewContext<Self>) {
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
                            &AgentInputFooterAction::InsertFilePath(path.clone()),
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
                    ctx.emit(AgentInputFooterEvent::PluginInstalled(agent));
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

    fn render_cli_toolbar_item(
        &self,
        item: &AgentToolbarItemKind,
        app: &AppContext,
    ) -> Option<Box<dyn Element>> {
        if !item.available_in().is_available_for_cli() {
            return None;
        }

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
            AgentToolbarItemKind::VoiceInput => {
                #[cfg(feature = "voice_input")]
                {
                    let enabled = AISettings::as_ref(app).is_voice_input_enabled(app);
                    enabled.then(|| ChildView::new(&self.mic_button).finish())
                }
                #[cfg(not(feature = "voice_input"))]
                None
            }
            AgentToolbarItemKind::Settings => Some(ChildView::new(&self.settings_button).finish()),
            // Handled by the available_in() guard above; included for exhaustiveness.
            AgentToolbarItemKind::ModelSelector
            | AgentToolbarItemKind::NLDToggle
            | AgentToolbarItemKind::ContextWindowUsage
            | AgentToolbarItemKind::UsageSummary
            | AgentToolbarItemKind::FastForwardToggle => None,
        }
    }

    fn render_cli_mode_footer(&self, app: &AppContext) -> Box<dyn Element> {
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
            if let Some(element) = self.render_cli_toolbar_item(item, app) {
                left_buttons.add_child(element);
            }
        }

        let mut right_buttons = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Min)
            .with_spacing(4.);

        for item in &right_items {
            if let Some(element) = self.render_cli_toolbar_item(item, app) {
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
                ctx.dispatch_typed_action(AgentInputFooterAction::ShowContextMenu { position });
                DispatchEventResult::StopPropagation
            })
            .finish();

        Container::new(content).with_vertical_padding(4.).finish()
    }

    pub fn has_open_chip_menu(&self, app: &AppContext) -> bool {
        let has_open_display_chip = self
            .all_display_chips()
            .any(|chip| chip.as_ref(app).display_chip_kind().has_open_menu());

        has_open_display_chip
    }

    pub fn is_model_selector_open(&self, app: &AppContext) -> bool {
        self.model_selector.as_ref(app).is_open()
    }

    fn handle_profile_model_selector_event(
        &mut self,
        event: &ProfileModelSelectorEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            ProfileModelSelectorEvent::MenuVisibilityChanged { open } => {
                if *open {
                    ctx.emit(AgentInputFooterEvent::ModelSelectorOpened);
                } else {
                    ctx.emit(AgentInputFooterEvent::ModelSelectorClosed);
                }
            }
            ProfileModelSelectorEvent::ToggleInlineModelSelector => {
                let initial_tab = if self
                    .terminal_model
                    .lock()
                    .block_list()
                    .active_block()
                    .is_agent_in_control_or_tagged_in()
                {
                    InlineModelSelectorTab::FullTerminalUse
                } else {
                    InlineModelSelectorTab::BaseAgent
                };

                ctx.emit(AgentInputFooterEvent::ToggleInlineModelSelector { initial_tab });
            }
        }
    }

    pub fn set_voice_is_active(&mut self, is_active: bool, ctx: &mut ViewContext<Self>) {
        self.mic_button.update(ctx, |button, ctx| {
            button.set_active(is_active, ctx);
        });
    }

    #[cfg(feature = "voice_input")]
    fn stop_cli_voice_and_reset(&mut self, ctx: &mut ViewContext<Self>) {
        let lifecycle_state = self.cli_voice_input_lifecycle.state();
        if lifecycle_state == VoiceInputLifecycleState::Idle
            && self.cli_recording_handle.is_none()
            && self.cli_transcription_handle.is_none()
        {
            return;
        }
        if let Some(handle) = self.cli_recording_handle.take() {
            handle.abort();
        }
        if let Some(handle) = self.cli_transcription_handle.take() {
            handle.abort();
        }
        voice_input::VoiceInput::handle(ctx).update(ctx, |voice_input, _| {
            if voice_input.is_listening() {
                voice_input.abort_listening();
            }
            voice_input.set_transcribing_active(false);
        });

        self.cli_voice_input_lifecycle.cancel();
        self.update_cli_mic_button_state(ctx);
    }

    // ── CLI agent voice input (self-contained, bypasses editor) ──────

    /// Toggle voice input for CLI agent mode. Records audio and writes the
    /// transcription directly to the PTY, bypassing the editor voice flow.
    #[cfg(feature = "voice_input")]
    pub fn toggle_cli_voice_input(
        &mut self,
        source: &voice_input::VoiceInputToggledFrom,
        ctx: &mut ViewContext<Self>,
    ) {
        if !UserWorkspaces::as_ref(ctx).is_voice_enabled() {
            return;
        }

        if !AISettings::as_ref(ctx).is_voice_input_enabled(ctx) {
            return;
        }

        // For key-based toggling, validate the key state against current voice state.
        if let voice_input::VoiceInputToggledFrom::Key { state } = source {
            match (self.cli_voice_input_lifecycle.state(), state) {
                (VoiceInputLifecycleState::Idle, torchui::event::KeyState::Released) => return,
                (VoiceInputLifecycleState::Listening, torchui::event::KeyState::Pressed) => return,
                _ => {}
            }
        }

        match self.cli_voice_input_lifecycle.state() {
            VoiceInputLifecycleState::Idle => {
                if !crate::ai::AIRequestUsageModel::as_ref(ctx).can_request_voice() {
                    self.show_cli_voice_error_toast("Voice input limit reached", ctx);
                    return;
                }

                let session_result = voice_input::VoiceInput::handle(ctx)
                    .update(ctx, |voice_input, ctx| {
                        voice_input.start_listening(ctx, source.clone())
                    });

                match session_result {
                    Ok(session) => {
                        if !self.cli_voice_input_lifecycle.start() {
                            return;
                        }
                        self.update_cli_mic_button_state(ctx);

                        if let Some(agent) = self.cli_agent(ctx) {
                            send_telemetry_from_ctx!(
                                TelemetryEvent::CLIAgentToolbarVoiceInputUsed { cli_agent: agent },
                                ctx
                            );
                        }

                        if matches!(*source, voice_input::VoiceInputToggledFrom::Button) {
                            self.maybe_show_first_time_cli_voice_toast(ctx);
                        }

                        self.cli_recording_handle = Some(ctx.spawn(
                            async move { session.await_result().await },
                            AgentInputFooter::handle_cli_voice_session_result,
                        ));
                    }
                    Err(StartListeningError::AccessDenied) => {
                        self.show_cli_microphone_access_toast(ctx);
                    }
                    Err(e) => {
                        report_error!(
                            anyhow::Error::new(e).context("Failed to start CLI voice input")
                        );
                    }
                }
            }
            VoiceInputLifecycleState::Listening => {
                voice_input::VoiceInput::handle(ctx).update(ctx, |voice_input, ctx| {
                    if let Err(e) = anyhow::Context::context(
                        voice_input.stop_listening(ctx),
                        "Failed to stop CLI voice input",
                    ) {
                        report_error!(e);
                    }
                });
            }
            VoiceInputLifecycleState::Transcribing => {
                // Don't allow toggling while transcribing.
            }
        }
        ctx.notify();
    }

    #[cfg(feature = "voice_input")]
    fn handle_cli_voice_session_result(
        &mut self,
        result: VoiceSessionResult,
        ctx: &mut ViewContext<Self>,
    ) {
        use crate::editor::VoiceTranscriber;
        self.cli_recording_handle = None;

        match result {
            VoiceSessionResult::Audio {
                wav_base64,
                session_duration_ms: _,
            } => {
                let voice_transcriber = VoiceTranscriber::as_ref(ctx);
                if let Some(transcriber) = voice_transcriber.transcriber() {
                    let transcriber = transcriber.clone();
                    let language = AISettings::as_ref(ctx)
                        .voice_input_language_code()
                        .map(str::to_owned);
                    let team_scope = RequestTeamScope::from_scope(
                        &UserWorkspaces::as_ref(ctx).team_context_for_view(ctx),
                    );
                    if !self.cli_voice_input_lifecycle.begin_transcribing() {
                        return;
                    }

                    voice_input::VoiceInput::handle(ctx).update(ctx, |voice, _| {
                        voice.set_transcribing_active(true);
                    });

                    self.cli_transcription_handle = Some(ctx.spawn(
                        async move {
                            transcriber
                                .transcribe(wav_base64, language, team_scope)
                                .await
                        },
                        AgentInputFooter::apply_cli_transcribed_voice_input,
                    ));
                } else {
                    self.cli_voice_input_lifecycle.fail();
                }
            }
            VoiceSessionResult::Aborted { .. } => {
                self.cli_voice_input_lifecycle.fail();
            }
        }
        self.update_cli_mic_button_state(ctx);
        ctx.notify();
    }

    #[cfg(feature = "voice_input")]
    fn apply_cli_transcribed_voice_input(
        &mut self,
        result: Result<String, TranscribeError>,
        ctx: &mut ViewContext<Self>,
    ) {
        if !self.cli_voice_input_lifecycle.complete() {
            return;
        }

        voice_input::VoiceInput::handle(ctx).update(ctx, |voice, _| {
            voice.set_transcribing_active(false);
        });

        match result {
            Ok(transcribed_text) => {
                if !transcribed_text.is_empty() {
                    if self.has_active_cli_agent_input_session(ctx) {
                        ctx.emit(AgentInputFooterEvent::InsertIntoCLIRichInput(
                            transcribed_text,
                        ));
                    } else {
                        ctx.emit(AgentInputFooterEvent::InsertIntoCLIPty(transcribed_text));
                    }
                }
            }
            Err(e) => match e {
                TranscribeError::QuotaLimit => {
                    self.show_cli_voice_error_toast("Voice input limit reached", ctx);
                }
                _ => {
                    report_error!(
                        anyhow::Error::new(e).context("Failed to transcribe CLI voice input")
                    );
                    self.show_cli_voice_error_toast("Failed to transcribe voice input", ctx);
                }
            },
        }

        self.cli_transcription_handle = None;
        self.update_cli_mic_button_state(ctx);
        ctx.notify();
    }

    #[cfg(feature = "voice_input")]
    fn update_cli_mic_button_state(&self, ctx: &mut ViewContext<Self>) {
        let icon = match self.cli_voice_input_lifecycle.state() {
            VoiceInputLifecycleState::Idle => Icon::Microphone,
            VoiceInputLifecycleState::Listening => Icon::Stop,
            VoiceInputLifecycleState::Transcribing => Icon::DotsHorizontal,
        };
        let is_transcribing = matches!(
            self.cli_voice_input_lifecycle.state(),
            VoiceInputLifecycleState::Transcribing
        );

        self.mic_button.update(ctx, |button, ctx| {
            button.set_icon(Some(icon), ctx);
            button.set_active(is_transcribing, ctx);
        });
    }

    #[cfg(feature = "voice_input")]
    fn show_cli_voice_error_toast(&self, message: &str, ctx: &mut ViewContext<Self>) {
        let window_id = ctx.window_id();
        ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
            let toast = DismissibleToast::error(message.to_string());
            toast_stack.add_ephemeral_toast(toast, window_id, ctx);
        });
    }

    #[cfg(feature = "voice_input")]
    fn show_cli_microphone_access_toast(&self, ctx: &mut ViewContext<Self>) {
        let window_id = ctx.window_id();
        ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
            let toast = DismissibleToast::error(String::from(
                "Failed to start voice input (you may need to enable Microphone access)",
            ));
            toast_stack.add_ephemeral_toast(toast, window_id, ctx);
        });
    }

    #[cfg(feature = "voice_input")]
    fn maybe_show_first_time_cli_voice_toast(&self, ctx: &mut ViewContext<Self>) {
        let window_id = ctx.window_id();
        AISettings::handle(ctx).update(ctx, |settings, ctx| {
            if let Some(toggle_key) = settings.maybe_setup_first_time_voice(ctx) {
                ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                    let toast = DismissibleToast::success(format!(
                        "Voice input is enabled. You can also press and hold the `{}` key to activate voice input (configure in Settings > AI > Voice)",
                        toggle_key.display_name()
                    ));
                    toast_stack.add_ephemeral_toast(toast, window_id, ctx);
                });
            }
        });
    }

    fn sync_fast_forward_button(&self, ctx: &mut ViewContext<Self>) {
        // Read directly from the conversation, same data source as the warping
        // indicator footer's auto-approve chip.
        let is_active = BlocklistAIHistoryModel::as_ref(ctx)
            .active_conversation(self.terminal_view_id)
            .map(|c| c.autoexecute_any_action())
            .unwrap_or(false);

        let icon = if is_active {
            Icon::FastForwardFilled
        } else {
            Icon::FastForward
        };
        let tooltip = if is_active {
            FAST_FORWARD_ON_TOOLTIP
        } else {
            FAST_FORWARD_OFF_TOOLTIP
        };

        self.fast_forward_button.update(ctx, |button, ctx| {
            button.set_icon(Some(icon), ctx);
            button.set_tooltip(Some(tooltip), ctx);
            button.set_active(is_active, ctx);
        });
    }

    fn update_context_window_button(&mut self, ctx: &mut ViewContext<Self>) {
        if let Some(conversation) =
            BlocklistAIHistoryModel::as_ref(ctx).active_conversation(self.terminal_view_id)
        {
            let usage = conversation.context_window_usage();
            let icon = icon_for_context_window_usage(usage);
            let remaining_pct = ((1.0 - usage) * 100.0).round() as i32;

            let expiry = conversation.latest_exchange().and_then(|exchange| {
                let output = exchange.output_status.output()?;
                output.get().model_info.as_ref()?.prompt_cache_expires_at
            });
            let is_cache_expired = FeatureFlag::PromptCacheExpiryWarning.is_enabled()
                && expiry.is_some_and(|expiry| expiry <= Local::now());
            let context_remaining_tooltip = format!("{remaining_pct}% context remaining");
            let tooltip = if is_cache_expired {
                format!("{context_remaining_tooltip} · prompt cache expired")
            } else {
                context_remaining_tooltip
            };

            self.prompt_cache_expired = is_cache_expired;
            self.context_window_button.update(ctx, |button, ctx| {
                button.set_icon(Some(icon), ctx);
                button.set_tooltip(Some(tooltip), ctx);
            });

            self.reschedule_prompt_cache_expiry_timer(expiry, ctx);
        }
    }

    /// Retargets (or closes) an open usage popover when the active
    /// conversation changes, so it never shows the previous conversation's
    /// figures.
    fn retarget_usage_popover_if_open(&mut self, ctx: &mut ViewContext<Self>) {
        if !self.usage_popover_open {
            return;
        }
        let active_conversation_id = BlocklistAIHistoryModel::as_ref(ctx)
            .active_conversation(self.terminal_view_id)
            .map(|conversation| conversation.id());
        if self.usage_popover.as_ref(ctx).conversation_id() == active_conversation_id {
            return;
        }
        match active_conversation_id {
            Some(conversation_id) => self.usage_popover.update(ctx, |popover, ctx| {
                popover.reset_for_conversation(conversation_id, ctx);
            }),
            None => self.usage_popover_open = false,
        }
    }

    /// Refreshes the usage button's tooltip with the active conversation's
    /// total cost — the same figure the popover's header shows.
    fn update_usage_button(&mut self, ctx: &mut ViewContext<Self>) {
        // Falls back to the bare label rather than keeping the previous
        // conversation's figure, which would attribute another conversation's
        // spend to this one.
        let tooltip = BlocklistAIHistoryModel::as_ref(ctx)
            .active_conversation(self.terminal_view_id)
            .map(|conversation| {
                format!(
                    "Conversation usage: {}",
                    conversation_total_text(
                        conversation,
                        conversation_usage_display_unit(conversation, ctx),
                    )
                )
            })
            .unwrap_or_else(|| "Conversation usage".to_string());
        self.usage_button.update(ctx, |button, ctx| {
            button.set_tooltip(Some(tooltip), ctx);
        });
    }

    /// Schedules a refresh of the context-window button at the prompt-cache
    /// expiry instant so the notification dot appears while the conversation is idle.
    fn reschedule_prompt_cache_expiry_timer(
        &mut self,
        expiry: Option<DateTime<Local>>,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(handle) = self.prompt_cache_expiry_timer_handle.take() {
            handle.abort();
        }
        if !FeatureFlag::PromptCacheExpiryWarning.is_enabled() {
            return;
        }
        // Only future expiries need a timer; past ones already render as expired.
        let Some(delay) = expiry.and_then(|expiry| (expiry - Local::now()).to_std().ok()) else {
            return;
        };
        let handle = ctx.spawn(
            async move {
                Timer::after(delay).await;
            },
            |me, _, ctx| {
                me.prompt_cache_expiry_timer_handle = None;
                me.update_context_window_button(ctx);
                ctx.notify();
            },
        );
        self.prompt_cache_expiry_timer_handle = Some(handle);
    }

    fn render_toolbar_item(
        &self,
        item: &AgentToolbarItemKind,
        app: &AppContext,
    ) -> Option<Box<dyn Element>> {
        if !item.available_in().is_available_for_agent_view() {
            return None;
        }

        match item {
            AgentToolbarItemKind::ContextChip(chip_kind) => {
                let chips = match SessionSettings::as_ref(app)
                    .agent_footer_chip_selection
                    .left_chips()
                    .contains(chip_kind)
                {
                    true => &self.left_display_chips,
                    false => &self.right_display_chips,
                };
                chips
                    .iter()
                    .find(|chip| chip.as_ref(app).chip_kind() == chip_kind)
                    .filter(|chip| chip.as_ref(app).should_render(app))
                    .map(|chip| ChildView::new(chip).finish())
            }
            AgentToolbarItemKind::ModelSelector => {
                let show = FeatureFlag::ProfilesDesignRevamp.is_enabled()
                    || *SessionSettings::as_ref(app).show_model_selectors_in_prompt;
                show.then(|| ChildView::new(&self.model_selector).finish())
            }
            AgentToolbarItemKind::NLDToggle => Some(ChildView::new(&self.nld_button).finish()),
            AgentToolbarItemKind::VoiceInput => {
                #[cfg(feature = "voice_input")]
                {
                    let enabled =
                        crate::settings::AISettings::as_ref(app).is_voice_input_enabled(app);
                    enabled.then(|| ChildView::new(&self.mic_button).finish())
                }
                #[cfg(not(feature = "voice_input"))]
                None
            }
            AgentToolbarItemKind::FileAttach => Some(ChildView::new(&self.file_button).finish()),
            AgentToolbarItemKind::ContextWindowUsage => {
                let has_conversation = FeatureFlag::ContextWindowUsageV2.is_enabled()
                    && BlocklistAIHistoryModel::as_ref(app)
                        .active_conversation(self.terminal_view_id)
                        .is_some();
                has_conversation.then(|| {
                    let chip = ChildView::new(&self.context_window_button).finish();
                    if !self.prompt_cache_expired {
                        return chip;
                    }

                    let appearance = Appearance::as_ref(app);
                    let dot = Container::new(
                        ConstrainedBox::new(Empty::new().finish())
                            .with_width(6.)
                            .with_height(6.)
                            .finish(),
                    )
                    .with_corner_radius(CornerRadius::with_all(Radius::Percentage(50.)))
                    .with_background(Fill::Solid(
                        AnsiColorIdentifier::Yellow
                            .to_ansi_color(&appearance.theme().terminal_colors().normal)
                            .into(),
                    ))
                    .finish();

                    let mut stack = Stack::new();
                    stack.add_child(chip);
                    stack.add_positioned_overlay_child(
                        dot,
                        OffsetPositioning::offset_from_parent(
                            vec2f(3., -3.),
                            ParentOffsetBounds::WindowByPosition,
                            ParentAnchor::TopRight,
                            ChildAnchor::TopRight,
                        ),
                    );
                    stack.finish()
                })
            }
            AgentToolbarItemKind::UsageSummary => {
                // A persisted custom toolbar layout is replayed verbatim at render time, so
                // the tier has to be checked here rather than only in `is_available`, which
                // the render path does not consult.
                match UserWorkspaces::as_ref(app).charge_unit() {
                    ChargeUnit::Cents => {}
                    ChargeUnit::Credits => return None,
                }
                let conversation = BlocklistAIHistoryModel::as_ref(app)
                    .active_conversation(self.terminal_view_id)?;
                if !conversation.usage_totals().has_usage {
                    return None;
                }

                let button = SavePosition::new(
                    ChildView::new(&self.usage_button).finish(),
                    USAGE_BUTTON_SAVE_POSITION_ID,
                )
                .finish();
                let mut stack = Stack::new().with_child(button);
                if self.usage_popover_open {
                    let positioning = match self.menu_positioning_provider.menu_position(app) {
                        MenuPositioning::BelowInputBox => {
                            OffsetPositioning::offset_from_save_position_element(
                                USAGE_BUTTON_SAVE_POSITION_ID,
                                vec2f(0., 4.),
                                torchui::elements::PositionedElementOffsetBounds::WindowByPosition,
                                torchui::elements::PositionedElementAnchor::BottomRight,
                                ChildAnchor::TopRight,
                            )
                        }
                        MenuPositioning::AboveInputBox => {
                            OffsetPositioning::offset_from_save_position_element(
                                USAGE_BUTTON_SAVE_POSITION_ID,
                                vec2f(0., -4.),
                                torchui::elements::PositionedElementOffsetBounds::WindowByPosition,
                                torchui::elements::PositionedElementAnchor::TopRight,
                                ChildAnchor::BottomRight,
                            )
                        }
                    };
                    stack.add_positioned_overlay_child(
                        ChildView::new(&self.usage_popover).finish(),
                        positioning,
                    );
                }
                Some(stack.finish())
            }
            AgentToolbarItemKind::FastForwardToggle => FeatureFlag::FastForwardAutoexecuteButton
                .is_enabled()
                .then(|| ChildView::new(&self.fast_forward_button).finish()),
            AgentToolbarItemKind::FileExplorer => item
                .is_available(app)
                .then(|| ChildView::new(&self.file_explorer_button).finish()),
            // Handled by the available_in() guard above; included for exhaustiveness.
            AgentToolbarItemKind::RichInput | AgentToolbarItemKind::Settings => None,
        }
    }

    #[cfg(test)]
    pub fn displayed_chip_kinds(
        &self,
        app: &AppContext,
    ) -> (
        Vec<crate::context_chips::ContextChipKind>,
        Vec<crate::context_chips::ContextChipKind>,
    ) {
        let collect_chip_kinds = |chips: &[ViewHandle<DisplayChip>]| {
            chips
                .iter()
                .map(|chip| chip.as_ref(app).chip_kind().clone())
                .collect()
        };

        (
            collect_chip_kinds(&self.left_display_chips),
            collect_chip_kinds(&self.right_display_chips),
        )
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

    #[cfg(test)]
    pub fn usage_tooltip_for_test(&self, app: &AppContext) -> Option<String> {
        self.usage_button
            .as_ref(app)
            .tooltip_for_test()
            .map(str::to_string)
    }
}

impl View for AgentInputFooter {
    fn ui_name() -> &'static str {
        "AgentViewFooter"
    }

    fn render(&self, app: &torchui::AppContext) -> Box<dyn torchui::Element> {
        // When a CLI agent session is active, render the CLI agent toolbar instead.
        if self.is_cli_agent_session_active(app) {
            return self.render_cli_mode_footer(app);
        }

        let session_settings = SessionSettings::as_ref(app);
        let left_items = session_settings.agent_footer_chip_selection.left_items();
        let right_items = session_settings.agent_footer_chip_selection.right_items();

        let mut left_buttons = Wrap::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_run_spacing(4.)
            .with_spacing(4.);

        for item in &left_items {
            if let Some(element) = self.render_toolbar_item(item, app) {
                left_buttons.add_child(element);
            }
        }

        let mut right_buttons = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Min)
            .with_spacing(4.);

        let has_prompt_alert = !self.prompt_alert.as_ref(app).is_no_alert();
        if has_prompt_alert {
            right_buttons.add_child(
                Shrinkable::new(
                    1.,
                    Clipped::new(ChildView::new(&self.prompt_alert).finish()).finish(),
                )
                .finish(),
            );
        } else {
            for item in &right_items {
                if let Some(element) = self.render_toolbar_item(item, app) {
                    right_buttons.add_child(element);
                }
            }
        }

        let content = Wrap::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(WrapFill::new(0., left_buttons.finish()).finish())
            .with_child(WrapFill::new(0., right_buttons.finish()).finish())
            .with_run_spacing(context_chips::spacing::UDI_ROW_RUN_SPACING)
            .finish();
        let content = EventHandler::new(content)
            .on_right_mouse_down(|ctx, _, position, _| {
                ctx.dispatch_typed_action(AgentInputFooterAction::ShowContextMenu { position });
                DispatchEventResult::StopPropagation
            })
            .finish();

        let mut container = Container::new(content).with_padding_bottom(8.0);
        if !has_prompt_alert {
            container = container.with_padding_right(16.);
        }

        container.finish()
    }
}

#[derive(Debug, Clone)]
pub enum AgentInputFooterAction {
    #[cfg(feature = "voice_input")]
    ToggleVoiceInput,
    SelectFile,
    InsertFilePath(String),
    ToggleCodeReview,
    ToggleFileExplorer,
    ToggleRichInput,
    ToggleAutodetectionSetting,
    InstallPlugin,
    UpdatePlugin,
    OpenPluginInstallInstructionsPane,
    OpenPluginUpdateInstructionsPane,
    DismissPluginChip,
    OpenCodingAgentSettings,
    OpenStatusSetup,
    ShowContextMenu {
        position: Vector2F,
    },
    ToggleUsagePopover,
}

impl TypedActionView for AgentInputFooter {
    type Action = AgentInputFooterAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut torchui::ViewContext<Self>) {
        match action {
            #[cfg(feature = "voice_input")]
            AgentInputFooterAction::ToggleVoiceInput => {
                // In CLI agent mode, handle voice recording/transcription
                // directly so text is written to the PTY instead of the editor.
                if self.is_cli_agent_session_active(ctx) {
                    self.toggle_cli_voice_input(&voice_input::VoiceInputToggledFrom::Button, ctx);
                } else {
                    ctx.emit(AgentInputFooterEvent::ToggleVoiceInput(
                        voice_input::VoiceInputToggledFrom::Button,
                    ));
                }
            }
            AgentInputFooterAction::SelectFile => {
                self.select_file(ctx);
            }
            AgentInputFooterAction::InsertFilePath(path) => {
                if let Some(agent) = self.cli_agent(ctx) {
                    send_telemetry_from_ctx!(
                        TelemetryEvent::CLIAgentToolbarImageAttached { cli_agent: agent },
                        ctx
                    );
                }
                let path_with_space = format!("{path} ");
                if self.has_active_cli_agent_input_session(ctx) {
                    ctx.emit(AgentInputFooterEvent::InsertIntoCLIRichInput(
                        path_with_space,
                    ));
                } else {
                    ctx.emit(AgentInputFooterEvent::WriteToPty(path_with_space));
                }
            }
            AgentInputFooterAction::ToggleCodeReview => {
                if let Some(agent) = self.cli_agent(ctx) {
                    ctx.emit(AgentInputFooterEvent::ToggleCodeReviewPane(agent));
                }
            }
            AgentInputFooterAction::ToggleFileExplorer => {
                ctx.emit(AgentInputFooterEvent::ToggleFileExplorer(
                    self.cli_agent(ctx),
                ));
            }
            AgentInputFooterAction::ToggleRichInput => {
                if self.has_active_cli_agent_input_session(ctx) {
                    ctx.emit(AgentInputFooterEvent::HideRichInput);
                } else {
                    ctx.emit(AgentInputFooterEvent::OpenRichInput);
                }
            }
            AgentInputFooterAction::ToggleAutodetectionSetting => {
                let ai_settings = AISettings::handle(ctx);
                ai_settings.update(ctx, |settings, ctx| {
                    report_if_error!(
                        settings
                            .ai_autodetection_enabled_internal
                            .toggle_and_save_value(ctx)
                    );
                });
            }
            AgentInputFooterAction::InstallPlugin => {
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
            AgentInputFooterAction::UpdatePlugin => {
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
            AgentInputFooterAction::OpenPluginInstallInstructionsPane => {
                #[cfg(not(target_family = "wasm"))]
                if let Some(agent) = self.cli_agent(ctx) {
                    send_telemetry_from_ctx!(
                        TelemetryEvent::CLIAgentPluginChipClicked {
                            cli_agent: agent,
                            action: PluginChipTelemetryAction::InstallInstructions,
                        },
                        ctx
                    );
                    ctx.emit(AgentInputFooterEvent::OpenPluginInstructionsPane(
                        agent,
                        PluginModalKind::Install,
                    ));
                }
            }
            AgentInputFooterAction::OpenPluginUpdateInstructionsPane => {
                #[cfg(not(target_family = "wasm"))]
                if let Some(agent) = self.cli_agent(ctx) {
                    send_telemetry_from_ctx!(
                        TelemetryEvent::CLIAgentPluginChipClicked {
                            cli_agent: agent,
                            action: PluginChipTelemetryAction::UpdateInstructions,
                        },
                        ctx
                    );
                    ctx.emit(AgentInputFooterEvent::OpenPluginInstructionsPane(
                        agent,
                        PluginModalKind::Update,
                    ));
                }
            }
            AgentInputFooterAction::DismissPluginChip => {
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
            AgentInputFooterAction::OpenCodingAgentSettings => {
                #[cfg(not(target_family = "wasm"))]
                ctx.dispatch_typed_action_deferred(WorkspaceAction::ScrollToSettingsWidget {
                    page: SettingsSection::ThirdPartyCLIAgents,
                    widget_id: crate::settings_view::cli_agent_settings_widget_id(),
                });
            }
            AgentInputFooterAction::OpenStatusSetup => {
                #[cfg(not(target_family = "wasm"))]
                if let Some(agent) = self.cli_agent(ctx).and_then(StatusAgent::for_cli_agent) {
                    ctx.dispatch_typed_action_deferred(WorkspaceAction::ScrollToSettingsWidget {
                        page: SettingsSection::ThirdPartyCLIAgents,
                        widget_id: crate::settings_view::agent_status_widget_id(agent),
                    });
                }
            }
            AgentInputFooterAction::ShowContextMenu { position } => {
                ctx.emit(AgentInputFooterEvent::ShowContextMenu {
                    position: *position,
                });
            }
            AgentInputFooterAction::ToggleUsagePopover => {
                self.usage_popover_open = !self.usage_popover_open;
                if self.usage_popover_open {
                    let conversation_id = BlocklistAIHistoryModel::as_ref(ctx)
                        .active_conversation(self.terminal_view_id)
                        .map(|conversation| conversation.id());
                    match conversation_id {
                        Some(conversation_id) => {
                            self.usage_popover.update(ctx, |popover, ctx| {
                                popover.reset_for_conversation(conversation_id, ctx);
                            });
                        }
                        None => self.usage_popover_open = false,
                    }
                }
                ctx.notify();
            }
        }
    }
}

pub enum AgentInputFooterEvent {
    #[cfg(feature = "voice_input")]
    ToggleVoiceInput(voice_input::VoiceInputToggledFrom),
    SelectFile,
    WriteToPty(String),
    /// Insert text into the CLI agent's PTY input using its paste strategy.
    InsertIntoCLIPty(String),
    /// Insert text into the CLI agent rich input.
    InsertIntoCLIRichInput(String),
    ToggleCodeReviewPane(CLIAgent),
    /// Toggle the file explorer side panel. `None` when no CLI agent session is
    /// attached to this pane.
    ToggleFileExplorer(Option<CLIAgent>),
    OpenRichInput,
    HideRichInput,
    ToggledChipMenu {
        open: bool,
    },
    TryExecuteChipCommand(PromptChipShellCommand),
    PromptAlert(PromptAlertEvent),
    ModelSelectorOpened,
    ModelSelectorClosed,
    ToggleInlineModelSelector {
        initial_tab: InlineModelSelectorTab,
    },
    OpenCodeReview,
    OpenAIDocument {
        document_id: AIDocumentId,
        document_version: AIDocumentVersion,
    },
    ShowContextMenu {
        position: Vector2F,
    },
    OpenEnvironmentManagementPane,
    PluginInstalled(CLIAgent),
    #[cfg(not(target_family = "wasm"))]
    OpenPluginInstructionsPane(CLIAgent, PluginModalKind),
}

impl Entity for AgentInputFooter {
    type Event = AgentInputFooterEvent;
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

/// Theme for the mic button.
/// Uses a blue icon when active (hovered, listening, or transcribing).
pub(crate) struct ActiveMicButtonTheme;

impl ActionButtonTheme for ActiveMicButtonTheme {
    fn background(&self, hovered: bool, appearance: &Appearance) -> Option<Fill> {
        AgentInputButtonTheme.background(hovered, appearance)
    }

    fn text_color(
        &self,
        hovered: bool,
        _background: Option<Fill>,
        appearance: &Appearance,
    ) -> ColorU {
        if hovered {
            appearance.theme().ansi_fg_blue()
        } else {
            appearance
                .theme()
                .sub_text_color(appearance.theme().surface_1())
                .into_solid()
        }
    }

    fn border(&self, appearance: &Appearance) -> Option<ColorU> {
        AgentInputButtonTheme.border(appearance)
    }

    fn should_opt_out_of_contrast_adjustment(&self) -> bool {
        true
    }

    fn font_properties(&self) -> Option<torchui::fonts::Properties> {
        AgentInputButtonTheme.font_properties()
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

/// Keeps the auto-approve chip's muted text semantics while using the shared opaque chip fill.
struct FastForwardButtonTheme;

impl ActionButtonTheme for FastForwardButtonTheme {
    fn background(&self, hovered: bool, appearance: &Appearance) -> Option<Fill> {
        AgentInputButtonTheme.background(hovered, appearance)
    }

    fn text_color(
        &self,
        _hovered: bool,
        _background: Option<Fill>,
        appearance: &Appearance,
    ) -> ColorU {
        appearance
            .theme()
            .sub_text_color(appearance.theme().surface_1())
            .into_solid()
    }

    fn border(&self, appearance: &Appearance) -> Option<ColorU> {
        AgentInputButtonTheme.border(appearance)
    }

    fn should_opt_out_of_contrast_adjustment(&self) -> bool {
        true
    }
}

/// Same as `AgentInputButtonTheme`, except with one-off special active styling for the NLD button.
struct NLDButtonTheme;

impl ActionButtonTheme for NLDButtonTheme {
    fn background(&self, hovered: bool, appearance: &Appearance) -> Option<Fill> {
        AgentInputButtonTheme.background(hovered, appearance)
    }

    fn text_color(
        &self,
        hovered: bool,
        _background: Option<Fill>,
        appearance: &Appearance,
    ) -> ColorU {
        if hovered {
            appearance.theme().ansi_fg_blue()
        } else {
            appearance
                .theme()
                .disabled_text_color(appearance.theme().surface_1())
                .into_solid()
        }
    }

    fn border(&self, appearance: &Appearance) -> Option<ColorU> {
        AgentInputButtonTheme.border(appearance)
    }

    fn should_opt_out_of_contrast_adjustment(&self) -> bool {
        true
    }
}
