mod action;
mod active_session;
#[cfg(target_os = "macos")]
pub(crate) mod cli_install;
pub(crate) mod cross_window_tab_drag;
mod global_actions;
pub mod header_toolbar_editor;
pub mod header_toolbar_item;
mod home;
mod lightbox_view;
mod native_modal;
mod registry;
pub mod sync_inputs;
pub mod tab_group;
pub mod tab_settings;
mod toast_stack;
pub mod util;
pub mod view;

pub use action::{
    CommandSearchOptions, InitContent, TabContextMenuAnchor, VerticalTabsPaneContextMenuTarget,
    WorkspaceAction,
};
pub use active_session::ActiveSession;
pub use global_actions::{flush_pending_save_app, save_app_now};
use serde::{Deserialize, Serialize};
use torch_core::context_flag::ContextFlag;
use torchui::AppContext;
use torchui::accessibility::AccessibilityVerbosity;
use torchui::elements::DropTargetData;
use torchui::keymap::{BindingDescription, EditableBinding, FixedBinding};
pub use util::{PaneViewLocator, TabMovement, active_terminal_in_window};
pub use view::{
    NEW_SESSION_MENU_BUTTON_POSITION_ID, NEW_TAB_BUTTON_POSITION_ID, PANEL_HEADER_HEIGHT,
    TAB_BAR_HEIGHT, TOTAL_TAB_BAR_HEIGHT, WORKSPACE_PADDING, Workspace,
};

use crate::channel::{Channel, ChannelState};
use crate::features::FeatureFlag;
use crate::palette::{PaletteMode, PaletteSource};
use crate::settings_view::{self, SettingsSection, flags};
use crate::tab::{NewSessionMenuItem, uses_vertical_tabs};
use crate::util::bindings::{self, CustomAction, cmd_or_ctrl_shift, is_binding_pty_compliant};
use crate::{code, modal, notebooks, tab_configs};

// Helper function to access panel header corner radius from other modules
pub fn panel_header_corner_radius() -> torchui::elements::CornerRadius {
    torchui::elements::CornerRadius::with_top(torchui::elements::Radius::Pixels(8.))
}

pub use registry::WorkspaceRegistry;
pub use toast_stack::{ToastStack, ToastStackEvent};

use crate::workspace::view::{
    LEFT_PANEL_GLOBAL_SEARCH_BINDING_NAME, LEFT_PANEL_PROJECT_EXPLORER_BINDING_NAME,
    NEW_FILE_BINDING_NAME, NEW_TAB_BINDING_NAME, NEW_TERMINAL_TAB_BINDING_NAME,
    NEW_WINDOW_BINDING_NAME, OPEN_GLOBAL_SEARCH_BINDING_NAME,
    TOGGLE_NOTIFICATION_MAILBOX_BINDING_NAME, TOGGLE_PROJECT_EXPLORER_BINDING_NAME,
    TOGGLE_RIGHT_PANEL_BINDING_NAME, TOGGLE_TAB_CONFIGS_MENU_BINDING_NAME,
    TOGGLE_VERTICAL_TABS_PANEL_BINDING_NAME,
};

pub fn init(app: &mut AppContext) {
    app.add_singleton_model(|_| WorkspaceRegistry::new());
    app.add_singleton_model(|_| cross_window_tab_drag::CrossWindowTabDrag::new());
    use torchui::keymap::macros::*;
    app.register_binding_validator::<Workspace>(is_binding_pty_compliant);

    modal::init(app);
    native_modal::init(app);
    lightbox_view::init(app);
    crate::tab_configs::remove_confirmation_dialog::init(app);
    tab_configs::session_config_modal::init(app);
    if code::editor_available() {
        view::global_search::view::GlobalSearchView::init(app);
    }
    view::right_panel::RightPanelView::init(app);
    header_toolbar_editor::init(app);

    settings_view::init_actions_from_parent_view(app, &id!("Workspace"), |settings_action| {
        WorkspaceAction::DispatchToSettingsTab(settings_action)
    });
    global_actions::init_global_actions(app);
    if code::editor_available() {
        notebooks::init(app);
    }
    code::init(app);
    sync_inputs::init(app);
    lsp::init(app);

    app.register_fixed_bindings([FixedBinding::empty(
        "Dump debug info",
        WorkspaceAction::DumpDebugInfo,
        id!("Workspace"),
    )]);
    if ChannelState::enable_debug_features() {
        app.register_editable_bindings([
            EditableBinding::new(
                "workspace:log_review_comment_send_status_for_active_tab",
                "[Debug] Log review comment send status for active tab",
                WorkspaceAction::LogReviewCommentSendStatusForActiveTab,
            )
            .with_context_predicate(id!("Workspace")),
            EditableBinding::new("workspace:panic", "Trigger a panic", WorkspaceAction::Panic)
                .with_context_predicate(id!("Workspace")),
            EditableBinding::new(
                "workspace:open_view_tree_debug_view",
                "Open view tree debugger",
                WorkspaceAction::OpenViewTreeDebugWindow,
            )
            .with_context_predicate(id!("Workspace")),
        ]);
        #[cfg(debug_assertions)]
        {
            // Debug actions (command palette only)
            app.register_editable_bindings([EditableBinding::new(
                "workspace:open_session_config_modal",
                "[Debug] Open Session Config Modal",
                WorkspaceAction::ShowSessionConfigModal,
            )
            .with_context_predicate(id!("Workspace"))]);
        }
    }

    #[cfg(target_os = "macos")]
    app.register_editable_bindings([EditableBinding::new(
        "workspace:sample_process",
        "Sample Process",
        WorkspaceAction::SampleProcess,
    )
    .with_context_predicate(id!("Workspace"))]);

    #[cfg(any(feature = "dhat_heap_profiling", feature = "heap_usage_tracking"))]
    {
        app.register_editable_bindings([EditableBinding::new(
            "workspace:dump_heap_profile",
            "Write heap profile to disk",
            WorkspaceAction::DumpHeapProfile,
        )
        .with_context_predicate(id!("Workspace"))]);
    }

    app.register_fixed_bindings([
        FixedBinding::custom(
            CustomAction::CycleNextSession,
            WorkspaceAction::CycleNextSession,
            "Switch to next tab",
            id!("Workspace") & id!("Workspace_MultipleTabs"),
        ),
        FixedBinding::custom(
            CustomAction::CyclePrevSession,
            WorkspaceAction::CyclePrevSession,
            "Switch to previous tab",
            id!("Workspace") & id!("Workspace_MultipleTabs"),
        ),
    ]);

    app.register_editable_bindings([EditableBinding::new(
        NEW_WINDOW_BINDING_NAME,
        BindingDescription::new("Create New Window"),
        WorkspaceAction::AddWindow,
    )
    .with_custom_action(CustomAction::AddWindow)
    .with_context_predicate(id!("Workspace"))
    .with_enabled(|| ContextFlag::CreateNewSession.is_enabled())]);

    app.register_editable_bindings([EditableBinding::new(
        NEW_FILE_BINDING_NAME,
        BindingDescription::new("New File"),
        WorkspaceAction::NewCodeFile,
    )
    .with_enabled(code::editor_available)
    .with_custom_action(CustomAction::NewFile)
    .with_context_predicate(id!("Workspace"))]);

    if FeatureFlag::UIZoom.is_enabled() {
        app.register_fixed_bindings([
            FixedBinding::custom(
                CustomAction::IncreaseZoom,
                WorkspaceAction::IncreaseZoom,
                "Zoom In",
                id!("Workspace"),
            )
            .with_group(bindings::BindingGroup::Settings.as_str()),
            FixedBinding::custom(
                CustomAction::DecreaseZoom,
                WorkspaceAction::DecreaseZoom,
                "Zoom Out",
                id!("Workspace"),
            )
            .with_group(bindings::BindingGroup::Settings.as_str()),
            FixedBinding::custom(
                CustomAction::ResetZoom,
                WorkspaceAction::ResetZoom,
                "Reset Zoom",
                id!("Workspace"),
            )
            .with_group(bindings::BindingGroup::Settings.as_str()),
        ]);
    } else {
        app.register_fixed_bindings([
            FixedBinding::custom(
                CustomAction::IncreaseFontSize,
                WorkspaceAction::IncreaseFontSize,
                "Increase font size",
                id!("Workspace"),
            )
            .with_group(bindings::BindingGroup::Settings.as_str()),
            FixedBinding::custom(
                CustomAction::DecreaseFontSize,
                WorkspaceAction::DecreaseFontSize,
                "Decrease font size",
                id!("Workspace"),
            )
            .with_group(bindings::BindingGroup::Settings.as_str()),
        ]);
    }

    if ContextFlag::LaunchConfigurations.is_enabled() {
        app.register_fixed_bindings([FixedBinding::custom(
            CustomAction::SaveCurrentConfig,
            WorkspaceAction::OpenLaunchConfigSaveModal,
            "Save new launch configuration",
            id!("Workspace"),
        )]);
    }

    if ChannelState::channel() == Channel::Integration {
        // Hack: Add explicit bindings for the tests, since the tests' injected
        // keypresses won't trigger Mac menu items. Unfortunately we can't use
        // cfg[test] because we are a separate process!
        app.register_fixed_bindings([
            FixedBinding::new(
                cmd_or_ctrl_shift("t"),
                WorkspaceAction::AddDefaultTab,
                id!("Workspace"),
            ),
            FixedBinding::new(
                cmd_or_ctrl_shift("p"),
                WorkspaceAction::TogglePalette {
                    mode: PaletteMode::Command,
                    source: PaletteSource::Other,
                },
                id!("Workspace"),
            ),
            FixedBinding::new(
                "cmdorctrl-,",
                WorkspaceAction::ShowSettings,
                id!("Workspace"),
            ),
        ]);
    }

    if FeatureFlag::UIZoom.is_enabled() {
        app.register_editable_bindings([
            EditableBinding::new(
                "workspace:increase_zoom",
                "Increase zoom level",
                WorkspaceAction::IncreaseZoom,
            )
            .with_context_predicate(id!("Workspace"))
            .with_group(bindings::BindingGroup::Settings.as_str())
            .with_key_binding("cmdorctrl-="),
            EditableBinding::new(
                "workspace:decrease_zoom",
                "Decrease zoom level",
                WorkspaceAction::DecreaseZoom,
            )
            .with_context_predicate(id!("Workspace"))
            .with_group(bindings::BindingGroup::Settings.as_str())
            .with_key_binding("cmdorctrl--"),
            EditableBinding::new(
                "workspace:reset_zoom",
                "Reset zoom level to default",
                WorkspaceAction::ResetZoom,
            )
            .with_group(bindings::BindingGroup::Settings.as_str())
            .with_context_predicate(id!("Workspace")),
            EditableBinding::new(
                "workspace:increase_font_size",
                "Increase font size",
                WorkspaceAction::IncreaseFontSize,
            )
            .with_context_predicate(id!("Workspace"))
            .with_group(bindings::BindingGroup::Settings.as_str())
            .with_key_binding("alt-shift->"),
            EditableBinding::new(
                "workspace:decrease_font_size",
                "Decrease font size",
                WorkspaceAction::DecreaseFontSize,
            )
            .with_context_predicate(id!("Workspace"))
            .with_group(bindings::BindingGroup::Settings.as_str())
            .with_key_binding("alt-shift-<"),
            EditableBinding::new(
                "workspace:reset_font_size",
                "Reset font size to default",
                WorkspaceAction::ResetFontSize,
            )
            .with_group(bindings::BindingGroup::Settings.as_str())
            .with_context_predicate(id!("Workspace")),
        ]);
    } else {
        app.register_editable_bindings([
            EditableBinding::new(
                "workspace:increase_font_size",
                "Increase font size",
                WorkspaceAction::IncreaseFontSize,
            )
            .with_context_predicate(id!("Workspace"))
            .with_group(bindings::BindingGroup::Settings.as_str())
            .with_key_binding("cmdorctrl-="),
            EditableBinding::new(
                "workspace:decrease_font_size",
                "Decrease font size",
                WorkspaceAction::DecreaseFontSize,
            )
            .with_context_predicate(id!("Workspace"))
            .with_group(bindings::BindingGroup::Settings.as_str())
            .with_key_binding("cmdorctrl--"),
            EditableBinding::new(
                "workspace:reset_font_size",
                "Reset font size to default",
                WorkspaceAction::ResetFontSize,
            )
            .with_group(bindings::BindingGroup::Settings.as_str())
            .with_context_predicate(id!("Workspace"))
            .with_key_binding("cmdorctrl-0")
            .with_custom_action(CustomAction::ResetFontSize),
        ]);
    }

    app.register_editable_bindings([
        EditableBinding::new(
            "workspace:show_theme_chooser",
            "Open theme picker",
            WorkspaceAction::ShowThemeChooserForActiveTheme,
        )
        .with_context_predicate(id!("Workspace"))
        .with_group(bindings::BindingGroup::Settings.as_str()),
        EditableBinding::new(
            TOGGLE_TAB_CONFIGS_MENU_BINDING_NAME,
            "Open tab configs menu",
            WorkspaceAction::ToggleTabConfigsMenu,
        )
        .with_context_predicate(id!("Workspace"))
        .with_mac_key_binding("cmd-ctrl-t")
        .with_linux_or_windows_key_binding("ctrl-alt-shift-T"),
        EditableBinding::new(
            "workspace:activate_first_tab",
            "Switch to 1st tab",
            WorkspaceAction::ActivateTabByNumber(1),
        )
        .with_context_predicate(id!("Workspace"))
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_key_binding("cmdorctrl-1"),
        EditableBinding::new(
            "workspace:activate_second_tab",
            "Switch to 2nd tab",
            WorkspaceAction::ActivateTabByNumber(2),
        )
        .with_context_predicate(id!("Workspace"))
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_key_binding("cmdorctrl-2"),
        EditableBinding::new(
            "workspace:activate_third_tab",
            "Switch to 3rd tab",
            WorkspaceAction::ActivateTabByNumber(3),
        )
        .with_context_predicate(id!("Workspace"))
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_key_binding("cmdorctrl-3"),
        EditableBinding::new(
            "workspace:activate_fourth_tab",
            "Switch to 4th tab",
            WorkspaceAction::ActivateTabByNumber(4),
        )
        .with_context_predicate(id!("Workspace"))
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_key_binding("cmdorctrl-4"),
        EditableBinding::new(
            "workspace:activate_fifth_tab",
            "Switch to 5th tab",
            WorkspaceAction::ActivateTabByNumber(5),
        )
        .with_context_predicate(id!("Workspace"))
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_key_binding("cmdorctrl-5"),
        EditableBinding::new(
            "workspace:activate_sixth_tab",
            "Switch to 6th tab",
            WorkspaceAction::ActivateTabByNumber(6),
        )
        .with_context_predicate(id!("Workspace"))
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_key_binding("cmdorctrl-6"),
        EditableBinding::new(
            "workspace:activate_seventh_tab",
            "Switch to 7th tab",
            WorkspaceAction::ActivateTabByNumber(7),
        )
        .with_context_predicate(id!("Workspace"))
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_key_binding("cmdorctrl-7"),
        EditableBinding::new(
            "workspace:activate_eighth_tab",
            "Switch to 8th tab",
            WorkspaceAction::ActivateTabByNumber(8),
        )
        .with_context_predicate(id!("Workspace"))
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_key_binding("cmdorctrl-8"),
        EditableBinding::new(
            "workspace:activate_last_tab",
            session_label("Switch to last tab", "Switch to last session"),
            WorkspaceAction::ActivateLastTab,
        )
        .with_context_predicate(id!("Workspace"))
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_key_binding("cmdorctrl-9"),
        EditableBinding::new(
            "workspace:activate_prev_tab",
            session_label("Activate previous tab", "Previous session"),
            WorkspaceAction::ActivatePrevTab,
        )
        .with_context_predicate(
            id!("Workspace") & id!("Workspace_MultipleTabs") & !id!("Workspace_PaneDragging"),
        )
        .with_mac_key_binding("shift-cmd-{")
        .with_linux_or_windows_key_binding("ctrl-pageup"),
        EditableBinding::new(
            "workspace:jump_to_attention_session",
            "Jump to next session needing attention",
            WorkspaceAction::JumpToAttentionSession,
        )
        .with_context_predicate(id!("Workspace") & !id!("Workspace_PaneDragging"))
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_enabled(ChannelState::is_local_only)
        .with_mac_key_binding("cmd-j")
        .with_linux_or_windows_key_binding("ctrl-shift-J"),
        EditableBinding::new(
            "workspace:toggle_agents_palette",
            BindingDescription::new("Show agents")
                .with_custom_description(bindings::MAC_MENUS_CONTEXT, "Show Agents"),
            WorkspaceAction::TogglePalette {
                mode: PaletteMode::Agents,
                source: PaletteSource::Other,
            },
        )
        .with_context_predicate(id!("Workspace"))
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_enabled(ChannelState::is_local_only)
        .with_custom_action(CustomAction::AgentsPalette),
        EditableBinding::new(
            "workspace:activate_next_tab",
            session_label("Activate next tab", "Next session"),
            WorkspaceAction::ActivateNextTab,
        )
        .with_context_predicate(
            id!("Workspace") & id!("Workspace_MultipleTabs") & !id!("Workspace_PaneDragging"),
        )
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_mac_key_binding("shift-cmd-}")
        .with_linux_or_windows_key_binding("ctrl-pagedown"),
        EditableBinding::new(
            "pane_group:navigate_prev",
            "Activate previous pane",
            WorkspaceAction::NavigatePrevPaneOrPanel,
        )
        .with_context_predicate(id!("Workspace"))
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_custom_action(CustomAction::ActivatePreviousPane),
        EditableBinding::new(
            "pane_group:navigate_next",
            "Activate next pane",
            WorkspaceAction::NavigateNextPaneOrPanel,
        )
        .with_context_predicate(id!("Workspace"))
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_custom_action(CustomAction::ActivateNextPane),
        EditableBinding::new(
            NEW_TAB_BINDING_NAME,
            BindingDescription::new(session_label("Create new tab", "New session")),
            WorkspaceAction::AddDefaultTab,
        )
        .with_context_predicate(id!("Workspace") & !id!("Workspace_PaneDragging"))
        .with_custom_action(CustomAction::NewTab)
        .with_enabled(|| ContextFlag::CreateNewSession.is_enabled()),
        EditableBinding::new(
            NEW_TERMINAL_TAB_BINDING_NAME,
            BindingDescription::new("New Terminal Tab"),
            WorkspaceAction::AddTerminalTab {
                hide_homepage: false,
            },
        )
        .with_context_predicate(id!("Workspace") & !id!("Workspace_PaneDragging"))
        .with_custom_action(CustomAction::NewTerminalTab)
        .with_enabled(|| ContextFlag::CreateNewSession.is_enabled()),
        EditableBinding::new(
            "workspace:toggle_left_panel",
            BindingDescription::new("Open Left Panel"),
            WorkspaceAction::ToggleLeftPanel,
        )
        .with_enabled(code::editor_available)
        .with_context_predicate(id!("Workspace"))
        .with_custom_action(CustomAction::ToggleLeftPanel),
        EditableBinding::new(
            TOGGLE_RIGHT_PANEL_BINDING_NAME,
            BindingDescription::new("Toggle code review")
                .with_custom_description(bindings::MAC_MENUS_CONTEXT, "Toggle Code Review"),
            WorkspaceAction::ToggleRightPanel,
        )
        .with_enabled(|| cfg!(feature = "local_fs") && code::editor_available())
        .with_context_predicate(id!("Workspace"))
        .with_mac_key_binding("cmd-shift-+")
        .with_linux_or_windows_key_binding("ctrl-shift-+"),
        EditableBinding::new(
            TOGGLE_VERTICAL_TABS_PANEL_BINDING_NAME,
            if ChannelState::is_local_only() {
                BindingDescription::new("Toggle sidebar")
                    .with_custom_description(bindings::MAC_MENUS_CONTEXT, "Toggle Sidebar")
            } else {
                BindingDescription::new("Toggle vertical tabs panel").with_custom_description(
                    bindings::MAC_MENUS_CONTEXT,
                    "Toggle Vertical Tabs Panel",
                )
            },
            WorkspaceAction::ToggleVerticalTabsPanel,
        )
        .with_context_predicate(id!("Workspace") & id!(flags::USE_VERTICAL_TABS_FLAG))
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_enabled(|| FeatureFlag::VerticalTabs.is_enabled())
        .with_custom_action(CustomAction::ToggleSidebar),
        EditableBinding::new(
            LEFT_PANEL_PROJECT_EXPLORER_BINDING_NAME,
            BindingDescription::new("Left Panel: Project explorer"),
            WorkspaceAction::ToggleProjectExplorer,
        )
        .with_enabled(code::editor_available)
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_context_predicate(id!("Workspace") & id!(flags::SHOW_PROJECT_EXPLORER))
        .with_custom_action(CustomAction::ToggleProjectExplorer),
        EditableBinding::new(
            LEFT_PANEL_GLOBAL_SEARCH_BINDING_NAME,
            BindingDescription::new("Left Panel: Global search"),
            WorkspaceAction::ToggleGlobalSearch,
        )
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_context_predicate(id!("Workspace") & id!(flags::SHOW_GLOBAL_SEARCH))
        .with_enabled(|| FeatureFlag::GlobalSearch.is_enabled())
        .with_custom_action(CustomAction::ToggleGlobalSearch),
        EditableBinding::new(
            "file_tree:toggle_hidden_files",
            BindingDescription::new("Toggle hidden files in Project Explorer"),
            WorkspaceAction::ToggleHiddenFiles,
        )
        .with_enabled(code::editor_available)
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_context_predicate(id!("Workspace") & id!(flags::SHOW_PROJECT_EXPLORER))
        .with_mac_key_binding("cmd-shift->")
        .with_linux_or_windows_key_binding("ctrl-shift->"),
        EditableBinding::new(
            TOGGLE_PROJECT_EXPLORER_BINDING_NAME,
            BindingDescription::new("Toggle project explorer")
                .with_custom_description(bindings::MAC_MENUS_CONTEXT, "Project Explorer"),
            WorkspaceAction::ToggleProjectExplorer,
        )
        .with_enabled(code::editor_available)
        .with_context_predicate(id!("Workspace") & id!(flags::SHOW_PROJECT_EXPLORER)),
        EditableBinding::new(
            OPEN_GLOBAL_SEARCH_BINDING_NAME,
            BindingDescription::new("Open global search")
                .with_custom_description(bindings::MAC_MENUS_CONTEXT, "Global Search"),
            WorkspaceAction::OpenGlobalSearch,
        )
        .with_enabled(code::editor_available)
        .with_context_predicate(id!("Workspace") & id!(flags::SHOW_GLOBAL_SEARCH))
        .with_mac_key_binding("cmd-shift-F")
        // we use alt because we use ctrl-shift-f for find because ctrl-f needs to be reserved for the shell
        .with_linux_or_windows_key_binding("alt-shift-F"),
        EditableBinding::new(
            "workspace:close_panel",
            BindingDescription::new("Close focused panel")
                .with_custom_description(bindings::MAC_MENUS_CONTEXT, "Close focused panel"),
            WorkspaceAction::ClosePanel,
        )
        .with_context_predicate(id!("Workspace"))
        .with_custom_action(CustomAction::CloseCurrentSession),
        EditableBinding::new(
            "workspace:toggle_command_palette",
            BindingDescription::new("Toggle command palette")
                .with_custom_description(bindings::MAC_MENUS_CONTEXT, "Command Palette"),
            WorkspaceAction::TogglePalette {
                mode: PaletteMode::Command,
                source: PaletteSource::Other,
            },
        )
        .with_group(bindings::BindingGroup::Settings.as_str())
        .with_context_predicate(id!("Workspace") & !id!("Workspace_CloudConversationWebViewer"))
        .with_custom_action(CustomAction::CommandPalette),
        EditableBinding::new(
            "workspace:move_tab_left",
            BindingDescription::new(session_label("Move tab left", "Move session up"))
                .with_dynamic_override(|ctx| {
                    uses_vertical_tabs(ctx)
                        .then(|| session_label("move tab up", "move session up").into())
                }),
            WorkspaceAction::MoveActiveTabLeft,
        )
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_context_predicate(
            id!("Workspace")
                & id!("Workspace_MultipleTabs")
                & !id!("Workspace_LeftmostTabActive")
                & !id!("Workspace_PaneDragging"),
        )
        .with_custom_action(CustomAction::MoveTabLeft),
        EditableBinding::new(
            "workspace:move_tab_right",
            BindingDescription::new(session_label("Move tab right", "Move session down"))
                .with_dynamic_override(|ctx| {
                    uses_vertical_tabs(ctx)
                        .then(|| session_label("move tab down", "move session down").into())
                }),
            WorkspaceAction::MoveActiveTabRight,
        )
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_context_predicate(
            id!("Workspace")
                & id!("Workspace_MultipleTabs")
                & !id!("Workspace_RightmostTabActive")
                & !id!("Workspace_PaneDragging"),
        )
        .with_custom_action(CustomAction::MoveTabRight),
        EditableBinding::new(
            "workspace:toggle_keybindings_page",
            "Toggle keyboard shortcuts",
            WorkspaceAction::ToggleKeybindingsPage,
        )
        .with_group(bindings::BindingGroup::KeyboardShortcuts.as_str())
        .with_context_predicate(id!("Workspace") & !id!("Workspace_TextOpen"))
        .with_custom_action(CustomAction::ToggleKeybindingsPage),
        EditableBinding::new(
            "workspace:show_keybinding_settings",
            "Open keybindings editor",
            WorkspaceAction::ConfigureKeybindingSettings {
                keybinding_name: None,
            },
        )
        .with_group(bindings::BindingGroup::KeyboardShortcuts.as_str())
        .with_context_predicate(id!("Workspace"))
        .with_mac_key_binding("cmd-ctrl-k"),
        EditableBinding::new(
            "workspace:toggle_block_snackbar",
            "Toggle sticky command header",
            WorkspaceAction::ToggleBlockSnackbar,
        )
        .with_group(bindings::BindingGroup::Settings.as_str())
        .with_context_predicate(id!("Workspace")),
    ]);

    // TODO(PLAT-113): Support a11y on non-MacOS platforms
    if cfg!(target_os = "macos") {
        app.register_editable_bindings([
            EditableBinding::new(
                "workspace:set_a11y_concise_verbosity_level",
                "[a11y] Set concise accessibility announcements",
                WorkspaceAction::SetA11yVerbosityLevel(AccessibilityVerbosity::Concise),
            )
            .with_context_predicate(id!("Workspace"))
            .with_key_binding("cmdorctrl-alt-c"),
            EditableBinding::new(
                "workspace:set_a11y_verbose_verbosity_level",
                "[a11y] Set verbose accessibility announcements",
                WorkspaceAction::SetA11yVerbosityLevel(AccessibilityVerbosity::Verbose),
            )
            .with_context_predicate(id!("Workspace"))
            .with_key_binding("cmdorctrl-alt-v"),
        ]);
    }

    app.register_editable_bindings([EditableBinding::new(
        "workspace:rename_active_tab",
        session_label("Rename the current tab", "Rename session"),
        WorkspaceAction::RenameActiveTab,
    )
    .with_group(bindings::BindingGroup::Settings.as_str())
    .with_custom_action(CustomAction::RenameTab)
    .with_context_predicate(id!("Workspace"))]);

    // Pane rename — same shape as RenameActiveTab but acts on the focused pane
    // in the active tab. Ships with no default keybinding so it surfaces in
    // Settings → Keyboard shortcuts as remappable; resolves issue #9351, where
    // the action existed only in the right-click context menu and was not
    // reachable via the binding registry.
    app.register_editable_bindings([EditableBinding::new(
        "workspace:rename_active_pane",
        "Rename the current pane",
        WorkspaceAction::RenameActivePane,
    )
    .with_group(bindings::BindingGroup::Settings.as_str())
    .with_context_predicate(id!("Workspace"))]);

    app.register_editable_bindings([EditableBinding::new(
        "workspace:cycle_active_tab_color",
        "Cycle current tab color",
        WorkspaceAction::CycleActiveTabColor,
    )
    .with_group(bindings::BindingGroup::Settings.as_str())
    .with_context_predicate(id!("Workspace"))]);

    // Tab grouping bindings (keyless by default; gated on `GroupedTabs`).
    app.register_editable_bindings([
        EditableBinding::new(
            "workspace:new_tab_group",
            "Create new tab group",
            // Reuse the new-session dropdown's action, not a dedicated variant.
            WorkspaceAction::SelectNewSessionMenuItem(NewSessionMenuItem::CreateNewTabGroup),
        )
        .with_enabled(|| FeatureFlag::GroupedTabs.is_enabled())
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_context_predicate(id!("Workspace") & !id!("Workspace_PaneDragging")),
        EditableBinding::new(
            "workspace:new_tab_group_from_active_or_selected_tabs",
            "Create tab group from active or selected tab(s)",
            WorkspaceAction::NewTabGroupFromActiveOrSelectedTabs,
        )
        .with_enabled(|| FeatureFlag::GroupedTabs.is_enabled())
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_context_predicate(id!("Workspace") & !id!("Workspace_PaneDragging")),
        // Gated on `Workspace_ActiveOrSelectedTabsInGroup`: offered only when
        // there's an unambiguous group to leave — a single-group multi-selection,
        // or (with no selection) a grouped active tab. Mixed selections aren't
        // offered, matching the multi-tab right-click menu.
        EditableBinding::new(
            "workspace:remove_active_or_selected_tabs_from_group",
            "Remove active or selected tab(s) from group",
            WorkspaceAction::RemoveActiveOrSelectedTabsFromGroup,
        )
        .with_enabled(|| FeatureFlag::GroupedTabs.is_enabled())
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_context_predicate(
            id!("Workspace")
                & id!("Workspace_ActiveOrSelectedTabsInGroup")
                & !id!("Workspace_PaneDragging"),
        ),
    ]);

    // Tab/group pinning bindings (keyless by default; gated on `PinnedTabs`).
    // Pin/unpin are split into separate entries so the palette label tracks
    // the active tab/group's current state.
    app.register_editable_bindings([
        EditableBinding::new(
            "workspace:pin_active_tab",
            "Pin current tab",
            WorkspaceAction::PinActiveTab,
        )
        .with_enabled(|| FeatureFlag::PinnedTabs.is_enabled())
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_context_predicate(
            id!("Workspace") & !id!("Workspace_ActiveTabPinned") & !id!("Workspace_PaneDragging"),
        ),
        EditableBinding::new(
            "workspace:unpin_active_tab",
            "Unpin current tab",
            WorkspaceAction::UnpinActiveTab,
        )
        .with_enabled(|| FeatureFlag::PinnedTabs.is_enabled())
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_context_predicate(
            id!("Workspace") & id!("Workspace_ActiveTabPinned") & !id!("Workspace_PaneDragging"),
        ),
        EditableBinding::new(
            "workspace:pin_active_tab_group",
            "Pin current tab group",
            WorkspaceAction::PinActiveTabGroup,
        )
        .with_enabled(|| {
            FeatureFlag::PinnedTabs.is_enabled() && FeatureFlag::GroupedTabs.is_enabled()
        })
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_context_predicate(
            id!("Workspace")
                & id!("Workspace_ActiveTabInGroup")
                & !id!("Workspace_ActiveTabGroupPinned")
                & !id!("Workspace_PaneDragging"),
        ),
        EditableBinding::new(
            "workspace:unpin_active_tab_group",
            "Unpin current tab group",
            WorkspaceAction::UnpinActiveTabGroup,
        )
        .with_enabled(|| {
            FeatureFlag::PinnedTabs.is_enabled() && FeatureFlag::GroupedTabs.is_enabled()
        })
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_context_predicate(
            id!("Workspace")
                & id!("Workspace_ActiveTabInGroup")
                & id!("Workspace_ActiveTabGroupPinned")
                & !id!("Workspace_PaneDragging"),
        ),
    ]);

    app.register_editable_bindings([
        EditableBinding::new(
            "workspace:terminate_app",
            "Quit Torch",
            WorkspaceAction::TerminateApp,
        )
        .with_context_predicate(id!("Workspace"))
        .with_group(bindings::BindingGroup::Close.as_str())
        .with_enabled(|| ContextFlag::CloseWindow.is_enabled()),
        EditableBinding::new(
            "workspace:close_window",
            BindingDescription::new("Close Window")
                .with_custom_description(bindings::MAC_MENUS_CONTEXT, "Close Window"),
            WorkspaceAction::CloseWindow,
        )
        .with_mac_key_binding("cmd-shift-W")
        .with_context_predicate(id!("Workspace"))
        .with_group(bindings::BindingGroup::Close.as_str())
        .with_custom_action(CustomAction::CloseWindow)
        .with_enabled(|| ContextFlag::CloseWindow.is_enabled()),
        EditableBinding::new(
            "workspace:close_active_tab",
            session_label("Close the current tab", "Close session"),
            WorkspaceAction::CloseActiveTab,
        )
        .with_custom_action(CustomAction::CloseTab)
        .with_group(bindings::BindingGroup::Close.as_str())
        .with_context_predicate(
            id!("Workspace") & (id!("Workspace_CloseWindow") | id!("Workspace_MultipleTabs")),
        ),
        EditableBinding::new(
            "workspace:close_other_tabs",
            session_label("Close other tabs", "Close other sessions"),
            WorkspaceAction::CloseNonActiveTabs,
        )
        .with_custom_action(CustomAction::CloseOtherTabs)
        .with_group(bindings::BindingGroup::Close.as_str())
        .with_context_predicate(id!("Workspace")),
        EditableBinding::new(
            "workspace:close_tabs_right_active_tab",
            BindingDescription::new(session_label(
                "Close tabs to the right",
                "Close sessions below",
            ))
            .with_dynamic_override(|ctx| {
                uses_vertical_tabs(ctx)
                    .then(|| session_label("close tabs below", "close sessions below").into())
            }),
            WorkspaceAction::CloseTabsRightActiveTab,
        )
        .with_group(bindings::BindingGroup::Close.as_str())
        .with_custom_action(CustomAction::CloseTabsRight)
        .with_context_predicate(id!("Workspace")),
        // We have two actions depending on the current state
        // (i.e. whether notifications are already on or off).
        EditableBinding::new(
            "workspace:toggle_notifications_on",
            "Turn notifications on",
            WorkspaceAction::ToggleNotifications,
        )
        .with_group(bindings::BindingGroup::Notifications.as_str())
        .with_context_predicate(id!("Workspace") & !id!("Notifications_Enabled")),
        EditableBinding::new(
            "workspace:toggle_notifications_off",
            "Turn notifications off",
            WorkspaceAction::ToggleNotifications,
        )
        .with_group(bindings::BindingGroup::Notifications.as_str())
        .with_context_predicate(id!("Workspace") & id!("Notifications_Enabled")),
        EditableBinding::new(
            "workspace:toggle_navigation_palette",
            BindingDescription::new("Toggle navigation palette")
                .with_custom_description(bindings::MAC_MENUS_CONTEXT, "Navigation Palette"),
            WorkspaceAction::TogglePalette {
                mode: PaletteMode::Navigation,
                source: PaletteSource::Other,
            },
        )
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_context_predicate(id!("Workspace"))
        .with_custom_action(CustomAction::NavigationPalette),
        EditableBinding::new(
            "workspace:toggle_launch_config_palette",
            "Launch configuration palette",
            WorkspaceAction::TogglePalette {
                mode: PaletteMode::LaunchConfig,
                source: PaletteSource::Other,
            },
        )
        .with_context_predicate(id!("Workspace"))
        .with_custom_action(CustomAction::LaunchConfigPalette)
        .with_enabled(|| ContextFlag::LaunchConfigurations.is_enabled()),
        EditableBinding::new(
            "workspace:toggle_files_palette",
            "Toggle Files Palette",
            WorkspaceAction::TogglePalette {
                mode: PaletteMode::Files,
                source: PaletteSource::Other,
            },
        )
        .with_context_predicate(id!("Workspace"))
        .with_custom_action(CustomAction::FilesPalette)
        .with_enabled(|| FeatureFlag::CommandPaletteFileSearch.is_enabled()),
        EditableBinding::new(
            "workspace:open_launch_config_save_modal",
            "Save new launch configuration",
            WorkspaceAction::OpenLaunchConfigSaveModal,
        )
        .with_context_predicate(id!("Workspace"))
        .with_custom_action(CustomAction::SaveCurrentConfig)
        .with_enabled(|| ContextFlag::LaunchConfigurations.is_enabled()),
    ]);

    if FeatureFlag::Autoupdate.is_enabled() {
        app.register_editable_bindings([
            EditableBinding::new(
                "workspace:update_and_relaunch",
                "Install update and relaunch",
                // TODO(vorporeal): I wonder if we should change wording here?
                WorkspaceAction::ApplyUpdate,
            )
            .with_group(bindings::BindingGroup::AutoUpdate.as_str())
            .with_context_predicate(id!("Workspace") & id!("AutoupdateState_UpdateReady"))
            .with_enabled(|| ContextFlag::PromptForVersionUpdates.is_enabled()),
            EditableBinding::new(
                "workspace:check_for_updates",
                "Check for updates",
                WorkspaceAction::CheckForUpdate,
            )
            .with_group(bindings::BindingGroup::AutoUpdate.as_str())
            .with_context_predicate(id!("Workspace") & !id!("AutoupdateState_UpdateReady"))
            .with_enabled(|| ContextFlag::PromptForVersionUpdates.is_enabled()),
        ]);
    }

    if !FeatureFlag::AvatarInTabBar.is_enabled() && !ChannelState::is_local_only() {
        app.register_editable_bindings([EditableBinding::new(
            "workspace:toggle_resource_center",
            "Toggle resource center",
            WorkspaceAction::ToggleResourceCenter,
        )
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_context_predicate(id!("Workspace"))
        .with_custom_action(CustomAction::ToggleResourceCenter)]);
    }

    // Torch Control CLI install/uninstall actions (macOS only)
    #[cfg(target_os = "macos")]
    {
        if FeatureFlag::WarpControlCli.is_enabled() {
            app.register_editable_bindings([
                EditableBinding::new(
                    "workspace:install_warpctrl",
                    "Install Torch Control CLI globally for use outside of Torch",
                    WorkspaceAction::InstallWarpctrl,
                )
                .with_group(bindings::BindingGroup::Settings.as_str())
                .with_context_predicate(id!("Workspace")),
                EditableBinding::new(
                    "workspace:uninstall_warpctrl",
                    "Undo global Torch Control CLI installation (warpctrl will still work within Torch)",
                    WorkspaceAction::UninstallWarpctrl,
                )
                .with_group(bindings::BindingGroup::Settings.as_str())
                .with_context_predicate(id!("Workspace")),
            ]);
        }
    }

    if FeatureFlag::Changelog.is_enabled() {
        app.register_editable_bindings([
            // Always show the "View latest changelog" action in the command palette,
            // but without a keybinding when the update toast is not visible.
            EditableBinding::new(
                "workspace:view_changelog",
                "View latest changelog",
                WorkspaceAction::ViewLatestChangelog,
            )
            .with_context_predicate(id!("Workspace") & !id!("UpdateToastVisible"))
            .with_group(bindings::BindingGroup::Settings.as_str())
            // Note that while the changelog resides in WarpEssentials, we should gate access to
            // the changelog based on whether WarpEssentials is an available view.
            .with_enabled(|| ContextFlag::WarpEssentials.is_enabled()),
            // When the update toast is visible, register the keybinding as well.
            EditableBinding::new(
                "workspace:view_changelog",
                "View latest changelog",
                WorkspaceAction::ViewLatestChangelog,
            )
            .with_context_predicate(id!("Workspace") & id!("UpdateToastVisible"))
            .with_group(bindings::BindingGroup::Settings.as_str())
            .with_custom_action(CustomAction::ViewChangelog)
            .with_linux_or_windows_key_binding(format!("alt-{}", cmd_or_ctrl_shift("o")))
            .with_enabled(|| ContextFlag::WarpEssentials.is_enabled()),
        ]);
    }

    app.register_editable_bindings([
        EditableBinding::new(
            "workspace:shift_focus_left",
            "Switch Focus to Left Panel",
            WorkspaceAction::FocusLeftPanel,
        )
        .with_context_predicate(id!("Workspace"))
        .with_key_binding("cmdorctrl-shift-("),
        EditableBinding::new(
            "workspace:shift_focus_right",
            "Switch Focus to Right Panel",
            WorkspaceAction::FocusRightPanel,
        )
        .with_context_predicate(id!("Workspace"))
        .with_key_binding("cmdorctrl-shift-)"),
    ]);

    app.register_editable_bindings([
        EditableBinding::new(
            "workspace:copy_current_path",
            BindingDescription::new("Copy current path")
                .with_custom_description(bindings::MAC_MENUS_CONTEXT, "Copy Current Path"),
            WorkspaceAction::CopyCurrentPath,
        )
        .with_context_predicate(id!("Workspace")),
        EditableBinding::new(
            "workspace:open_repository",
            if ChannelState::is_local_only() {
                BindingDescription::new("New session in folder…")
                    .with_custom_description(bindings::MAC_MENUS_CONTEXT, "New Session in Folder…")
            } else {
                BindingDescription::new("Open repository")
                    .with_custom_description(bindings::MAC_MENUS_CONTEXT, "Open Repository")
            },
            WorkspaceAction::OpenRepository { path: None },
        )
        .with_context_predicate(id!("Workspace"))
        .with_custom_action(CustomAction::OpenRepository)
        .with_group(bindings::BindingGroup::Folders.as_str()),
    ]);

    app.register_editable_bindings([EditableBinding::new(
        TOGGLE_NOTIFICATION_MAILBOX_BINDING_NAME,
        "Toggle notification mailbox",
        WorkspaceAction::ToggleNotificationMailbox { select_first: true },
    )
    .with_enabled(|| FeatureFlag::HOANotifications.is_enabled())
    .with_context_predicate(id!("Workspace"))
    .with_mac_key_binding("cmd-shift-U")
    .with_linux_or_windows_key_binding("ctrl-shift-U")
    .with_group(bindings::BindingGroup::WarpAi.as_str())]);

    add_open_setting_pages_as_editable_binding(app);
    add_overflow_menu_items_as_editable_binding(app);
}

fn add_open_setting_pages_as_editable_binding(app: &mut AppContext) {
    use torchui::keymap::macros::*;

    // Add the ability to open setting modals to the command palette.
    app.register_editable_bindings([
        EditableBinding::new(
            "workspace:show_settings",
            BindingDescription::new("Open Settings")
                .with_custom_description(bindings::MAC_MENUS_CONTEXT, "Settings"),
            WorkspaceAction::ShowSettings,
        )
        .with_context_predicate(id!("Workspace"))
        .with_group(bindings::BindingGroup::Settings.as_str())
        .with_custom_action(CustomAction::ShowSettings),
        EditableBinding::new(
            "workspace:show_settings_appearance_page",
            BindingDescription::new("Open Settings: Appearance")
                .with_custom_description(bindings::MAC_MENUS_CONTEXT, "Appearance..."),
            WorkspaceAction::ShowSettingsPage(SettingsSection::Appearance),
        )
        .with_group(bindings::BindingGroup::Settings.as_str())
        .with_context_predicate(id!("Workspace"))
        .with_custom_action(CustomAction::ShowAppearance),
        EditableBinding::new(
            "workspace:show_settings_features_page",
            "Open Settings: Features",
            WorkspaceAction::ShowSettingsPage(SettingsSection::Features),
        )
        .with_group(bindings::BindingGroup::Settings.as_str())
        .with_context_predicate(id!("Workspace")),
        EditableBinding::new(
            "workspace:show_settings_keyboard_shortcuts_page",
            BindingDescription::new("Open Settings: Keyboard Shortcuts").with_custom_description(
                bindings::MAC_MENUS_CONTEXT,
                "Configure Keyboard Shortcuts...",
            ),
            WorkspaceAction::ShowSettingsPage(SettingsSection::Keybindings),
        )
        .with_group(bindings::BindingGroup::KeyboardShortcuts.as_str())
        .with_context_predicate(id!("Workspace"))
        .with_custom_action(CustomAction::ConfigureKeybindings),
        EditableBinding::new(
            "workspace:show_settings_about_page",
            BindingDescription::new("Open Settings: About")
                .with_custom_description(bindings::MAC_MENUS_CONTEXT, "About Torch"),
            WorkspaceAction::ShowSettingsPage(SettingsSection::About),
        )
        .with_group(bindings::BindingGroup::Settings.as_str())
        .with_context_predicate(id!("Workspace"))
        .with_custom_action(CustomAction::ShowAboutWarp),
        EditableBinding::new(
            "workspace:show_settings_privacy_page",
            BindingDescription::new(format!("Open Settings: {}", SettingsSection::Privacy)),
            WorkspaceAction::ShowSettingsPage(SettingsSection::Privacy),
        )
        .with_group(bindings::BindingGroup::Settings.as_str())
        .with_context_predicate(id!("Workspace")),
        EditableBinding::new(
            "workspace:open_settings_file",
            "Open settings file",
            WorkspaceAction::OpenSettingsFile,
        )
        .with_enabled(|| FeatureFlag::SettingsFile.is_enabled() && cfg!(feature = "local_fs"))
        .with_group(bindings::BindingGroup::Settings.as_str())
        .with_context_predicate(id!("Workspace")),
    ]);
}

/// Torch's sidebar presents tabs as sessions.
fn session_label(tab_label: &'static str, session_label: &'static str) -> &'static str {
    if ChannelState::is_local_only() {
        session_label
    } else {
        tab_label
    }
}

/// Whether the build includes Torch account and cloud surfaces, which local-only builds omit.
fn has_cloud_surfaces() -> bool {
    !ChannelState::is_local_only()
}

fn add_overflow_menu_items_as_editable_binding(app: &mut AppContext) {
    use torchui::keymap::macros::*;

    // Add the ability to open all overflow menu items to the command palette.
    app.register_editable_bindings([
        EditableBinding::new(
            "workspace:link_to_slack",
            if ChannelState::is_local_only() {
                "Torch on GitHub (opens external link)"
            } else {
                "Join our Slack community (opens external link)"
            },
            WorkspaceAction::JoinSlack,
        )
        .with_context_predicate(id!("Workspace")),
        EditableBinding::new(
            "workspace:link_to_user_docs",
            "View user docs (opens external link)",
            WorkspaceAction::ViewUserDocs,
        )
        .with_context_predicate(id!("Workspace")),
        EditableBinding::new(
            "workspace:send_feedback",
            BindingDescription::new("Send feedback (opens external link)"),
            WorkspaceAction::SendFeedback,
        )
        .with_context_predicate(id!("Workspace")),
        #[cfg(not(target_family = "wasm"))]
        EditableBinding::new(
            "workspace:view_logs",
            "View Torch logs",
            WorkspaceAction::ViewLogs,
        )
        .with_context_predicate(id!("Workspace")),
        EditableBinding::new(
            "workspace:link_to_privacy_policy",
            "View privacy policy (opens external link)",
            WorkspaceAction::ViewPrivacyPolicy,
        )
        .with_enabled(has_cloud_surfaces)
        .with_context_predicate(id!("Workspace")),
    ]);
}

#[derive(PartialEq, Copy, Clone, Debug)]
pub struct TabBarDropTargetData {
    pub tab_bar_location: TabBarLocation,
}

#[derive(PartialEq, Copy, Clone, Debug)]
pub struct VerticalTabsPaneDropTargetData {
    pub tab_bar_location: TabBarLocation,
}

#[derive(PartialEq, Copy, Clone, Debug, Serialize, Deserialize)]
pub enum TabBarLocation {
    TabIndex(usize),
    AfterTabIndex(usize), // Indicates any area after the tabs in the tab bar, includes the total tab count
}

impl DropTargetData for TabBarDropTargetData {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl DropTargetData for VerticalTabsPaneDropTargetData {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
