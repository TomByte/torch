use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use torch_util::path::LineAndColumnArg;
use torchui::accessibility::AccessibilityVerbosity;
use torchui::geometry::rect::RectF;
use torchui::geometry::vector::Vector2F;
use torchui::platform::Cursor;
use torchui::platform::keyboard::KeyCode;
use torchui::{EntityId, WindowId};
use ui_components::lightbox;

use super::global_actions::{ForkFromExchange, ForkedConversationDestination};
use super::tab_settings::{
    VerticalTabsCompactSubtitle, VerticalTabsDisplayGranularity, VerticalTabsPrimaryInfo,
    VerticalTabsTabItemMode, VerticalTabsViewMode,
};
use super::view::WorkspaceBanner;
use crate::ai::agent::AIAgentExchangeId;
use crate::ai::agent::conversation::AIConversationId;
use crate::ai::blocklist::PendingAttachment;
use crate::ai::document::ai_document_model::{AIDocumentId, AIDocumentVersion};
use crate::palette::PaletteMode;
use crate::prompt::editor_modal::OpenSource as PromptEditorOpenSource;
use crate::search;
use crate::server::ids::SyncId;
use crate::server::telemetry::{AddTabWithShellSource, AgentModeEntrypoint, PaletteSource};
use crate::settings_view::{SettingsAction as SettingsTabAction, SettingsSection};
use crate::tab::{NewSessionMenuItem, SelectedTabColor};
use crate::tab_configs::TabConfig;
use crate::terminal::CLIAgent;
use crate::terminal::available_shells::AvailableShell;
use crate::terminal::view::inline_banner::ZeroStatePromptSuggestionType;
use crate::themes::theme::AnsiColorIdentifier;
use crate::themes::theme_chooser::ThemeChooserMode;
use crate::workflows::{WorkflowSelectionSource, WorkflowSource, WorkflowType};
use crate::workspace::PaneViewLocator;
use crate::workspace::tab_group::TabGroupId;

/// This enum determines how the search query is initialized when opening command search.
#[derive(Clone, Default, Debug)]
pub enum InitContent {
    /// Read the content of the active terminal input, and make that the initial search query.
    #[default]
    FromInputBuffer,
    /// Specify an exact string to initialize the query to.
    Custom(String),
}

/// To initialize command search, we may want to specify a search filter, or the content of the
/// query itself.
#[derive(Clone, Default, Debug)]
pub struct CommandSearchOptions {
    pub filter: Option<search::QueryFilter>,
    pub init_content: InitContent,
}

/// Specifies how to restore a conversation when it's not already open in a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum RestoreConversationLayout {
    /// Restore the conversation into the currently active pane.
    ActivePane,
    /// Restore the conversation in a new split pane.
    SplitPane,
    /// Restore the conversation in a new tab.
    #[default]
    NewTab,
}

#[derive(Debug, Clone, Copy)]
pub enum TabContextMenuAnchor {
    Pointer(Vector2F),
    VerticalTabsKebab,
}

/// Describes how the new-session dropdown menu was opened so the renderer
/// can pick the right anchor strategy.
#[derive(Debug, Clone, Copy)]
pub enum NewSessionMenuAnchor {
    /// Menu was opened from the `+` add-tab button. When vertical tabs are
    /// active, the renderer anchors below the button's save position;
    /// otherwise the contained position is used directly.
    AddTabButton(Vector2F),
    /// Menu was opened by right-clicking the vertical tabs panel.
    /// Always anchored at the contained pointer position.
    Pointer(Vector2F),
}

impl NewSessionMenuAnchor {
    pub fn position(&self) -> Vector2F {
        match self {
            Self::AddTabButton(position) | Self::Pointer(position) => *position,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum VerticalTabsPaneContextMenuTarget {
    ClickedPane(PaneViewLocator),
    ActivePane(PaneViewLocator),
}

impl VerticalTabsPaneContextMenuTarget {
    pub fn locator(self) -> PaneViewLocator {
        match self {
            Self::ClickedPane(locator) | Self::ActivePane(locator) => locator,
        }
    }
}

#[derive(Debug, Clone)]
pub enum WorkspaceAction {
    ActivateTab(usize),
    ActivatePrevTab,
    ActivateNextTab,
    ActivateLastTab,
    CyclePrevSession,
    CycleNextSession,
    MoveActiveTabLeft,
    MoveActiveTabRight,
    MoveTabLeft(usize),
    MoveTabRight(usize),
    RenameTab(usize),
    ResetTabName(usize),
    RenamePane(PaneViewLocator),
    ResetPaneName(PaneViewLocator),
    RenameActiveTab,
    RenameActivePane,
    SetActiveTabName(String),
    CycleActiveTabColor,
    SetActiveTabColor(SelectedTabColor),
    ToggleTabRightClickMenu {
        tab_index: usize,
        anchor: TabContextMenuAnchor,
    },
    ToggleTabSelectionRightClickMenu {
        tab_index: usize,
        anchor: TabContextMenuAnchor,
    },
    ToggleVerticalTabsPaneContextMenu {
        tab_index: usize,
        target: VerticalTabsPaneContextMenuTarget,
        position: Vector2F,
    },
    TabHoverWidthStart {
        width: f32,
    },
    TabHoverWidthEnd,
    ToggleTabBarOverflowMenu,
    ToggleWelcomeTips,
    CloseTab(usize),
    CloseActiveTab,
    CloseOtherTabs(usize),
    CloseNonActiveTabs,
    CloseTabsRight(usize),
    CloseTabsRightActiveTab,
    /// Close every tab that belongs to the given tab group.
    CloseTabGroup(TabGroupId),
    /// Toggle collapsed state for the given tab group.
    ToggleTabGroupCollapsed(TabGroupId),
    ToggleSidebarGroupCollapsed(String),
    ToggleSidebarAgentsCollapsed,
    /// The Torch sidebar was resized; its width is saved with the window.
    SidebarResized,
    /// Opens an inline editor over the given group's header for renaming.
    RenameTabGroup(TabGroupId),
    CancelActiveRename,
    /// Creates a new tab group containing the tab at the given index.
    NewTabGroupFromTab(usize),
    /// Moves the tab at `tab_index` into `group_id`, appending it to the
    /// end of the group's contiguous run.
    MoveTabToGroup {
        tab_index: usize,
        group_id: TabGroupId,
    },
    /// Removes the tab at the given index from its current group.
    RemoveTabFromGroup(usize),
    /// Selects every tab between the active tab and the shift-clicked row (inclusive).
    ShiftSelectTabRange {
        locator: PaneViewLocator,
    },
    /// Toggles whether the tab at `locator` is part of the active multi-selection.
    /// Dispatched on cmd-click of a vertical tab row.
    ToggleTabMultiSelection {
        locator: PaneViewLocator,
    },
    /// Clears the tab multi-selection.
    ClearTabMultiSelection,
    /// Creates a new tab group from the current tab multi-selection.
    NewTabGroupFromSelectedTabs,
    /// Context-aware "create group" entry point for the keybinding: groups
    /// the multi-selection when 2+ tabs are selected, otherwise groups the
    /// active tab.
    NewTabGroupFromActiveOrSelectedTabs,
    /// Moves every selected tab into `group_id`.
    MoveSelectedTabsToGroup {
        group_id: TabGroupId,
    },
    /// Removes every selected tab from its group (requires a single shared group).
    RemoveSelectedTabsFromGroup,
    /// Context-aware "remove from group" entry point for the keybinding:
    /// removes the multi-selection from its shared group when 2+ tabs are
    /// selected, otherwise removes the active tab.
    RemoveActiveOrSelectedTabsFromGroup,
    ToggleTabGroupRightClickMenu {
        group_id: TabGroupId,
        anchor: TabContextMenuAnchor,
    },
    UngroupTabs(TabGroupId),
    NewTabInGroup(TabGroupId),
    MoveTabGroupUp(TabGroupId),
    MoveTabGroupDown(TabGroupId),
    CloseTabsOutsideGroup(TabGroupId),
    CloseTabsAboveGroup(TabGroupId),
    CloseTabsBelowGroup(TabGroupId),
    /// Pins the tab at the given index. If the tab is part of a group, it
    /// is first extracted from the group and then pinned as ungrouped.
    PinTab(usize),
    /// Unpins the tab at the given index.
    UnpinTab(usize),
    /// Pins the active tab.
    PinActiveTab,
    /// Unpins the active tab.
    UnpinActiveTab,
    /// Pins the entire tab group: sets the group as pinned
    /// and moves the group block to the end of the pinned region.
    PinTabGroup(TabGroupId),
    /// Unpins the entire tab group: clears the pinned flag on the group
    /// and moves the group block to the start of the unpinned region.
    UnpinTabGroup(TabGroupId),
    /// Pins the active tab's group.
    PinActiveTabGroup,
    /// Unpins the active tab's group.
    UnpinActiveTabGroup,
    AddDefaultTab,
    AddTerminalTab {
        hide_homepage: bool,
    },
    NewSessionInDirectory(PathBuf),
    /// Starts a session in the directory that runs the agent as its first command.
    NewAgentSessionInDirectory {
        directory: PathBuf,
        agent: CLIAgent,
    },
    /// Starts a session that creates a new git worktree of the directory's repository and runs
    /// the agent there.
    NewAgentWorktreeSession {
        directory: PathBuf,
        agent: CLIAgent,
    },
    /// Opens the menu of session kinds for a project, at the clicked position.
    ToggleProjectSessionMenu {
        directory: PathBuf,
        position: Vector2F,
    },
    /// Opens a project group's context menu, with its color, at the clicked position.
    ToggleProjectGroupMenu {
        directory: PathBuf,
        position: Vector2F,
    },
    /// Sets the directory's tab color, or clears it if it already has that color.
    ToggleDirectoryColor {
        color: AnsiColorIdentifier,
        directory: PathBuf,
    },
    /// Reorders sidebar groups as the group with this key is dragged past its neighbors.
    DragSidebarGroup {
        key: String,
        cursor_position: Vector2F,
    },
    /// Activates the next sidebar session that needs input, or else the next unread finished one.
    JumpToAttentionSession,
    AddTabWithShell {
        shell: AvailableShell,
        source: AddTabWithShellSource,
    },
    /// Add a new tab that immediately enters agent view with a new conversation.
    AddAgentTab,
    OpenNewSessionMenu {
        anchor: NewSessionMenuAnchor,
    },
    ToggleTabConfigsMenu,
    ToggleNewSessionMenu {
        anchor: NewSessionMenuAnchor,
    },
    SelectNewSessionMenuItem(NewSessionMenuItem),
    AutoupdateFailureLink,
    ApplyUpdate,
    CopyVersion(&'static str),
    DownloadNewVersion,
    ConfigureKeybindingSettings {
        keybinding_name: Option<String>,
    },
    ShowSettings,
    ShowSettingsPage(SettingsSection),
    ShowSettingsPageWithSearch {
        search_query: String,
        section: Option<SettingsSection>,
    },
    ShowThemeChooser(ThemeChooserMode),
    ShowThemeChooserForActiveTheme,
    IncreaseFontSize,
    DecreaseFontSize,
    ResetFontSize,
    IncreaseZoom,
    DecreaseZoom,
    ResetZoom,
    ActivateTabByNumber(usize),
    SetTabShortcutModifierKey {
        key_code: KeyCode,
        pressed: bool,
    },
    OpenPalette {
        mode: PaletteMode,
        source: PaletteSource,
        query: Option<String>,
    },
    TogglePalette {
        mode: PaletteMode,
        source: PaletteSource,
    },
    JoinSlack,
    ViewUserDocs,
    ViewLatestChangelog,
    ViewPrivacyPolicy,
    SendFeedback,
    /// Open the log directory in the system file explorer with the current log file selected.
    #[cfg(not(target_family = "wasm"))]
    ViewLogs,
    ChangeCursor(Cursor),
    ToggleBlockSnackbar,
    ToggleErrorUnderlining,
    ToggleSyntaxHighlighting,
    CheckForUpdate,
    SetA11yVerbosityLevel(AccessibilityVerbosity),
    ToggleNotifications,
    ToggleTabColor {
        color: AnsiColorIdentifier,
        tab_index: usize,
    },
    ToggleTabGroupColor {
        color: AnsiColorIdentifier,
        group_id: TabGroupId,
    },
    OpenLaunchConfigSaveModal,
    SelectTabConfig(TabConfig),
    DispatchToSettingsTab(SettingsTabAction),
    ToggleResourceCenter,
    ToggleUserMenu,
    ToggleKeybindingsPage,
    ShowCommandSearch(CommandSearchOptions),
    TriggerExternalCtrlTFileSearch,
    TriggerExternalAltCDirectorySearch,
    ToggleMouseReporting,
    ToggleScrollReporting,
    ToggleFocusReporting,
    StartTabDrag,
    DragTab {
        tab_index: usize,
        tab_position: RectF,
    },
    CommitTabDrag {
        pane_group_id: EntityId,
        tab_position: RectF,
    },
    DropTab,
    StartGroupDrag(TabGroupId),
    DragGroup {
        group_id: TabGroupId,
        /// The dragged group's painted rect.
        position: RectF,
        /// The position of the cursor while dragging a group.
        cursor_position: Vector2F,
    },
    DropGroup,
    /// Toggles the left panel. In Code Mode V1 this toggles Torch Drive.
    /// In Code Mode V2 this toggles the left panel which contains both the project explorer and
    /// Torch Drive. This happens as explicit action from the user.
    ToggleLeftPanel,
    /// Toggles directly to the Torch Drive tab of the left panel in Code Mode V2
    /// Unconditionally opens Torch Drive.
    /// Toggles the right panel. This happens as an explicit action from the user.
    ToggleRightPanel,
    /// Opens the code review panel (right panel) without toggling. If already open,
    /// switches to the target pane's repo. Used by vertical tabs diff stats chip.
    OpenCodeReviewPanel(PaneViewLocator),
    /// Toggles the vertical tabs panel. This happens as an explicit action from the user.
    ToggleVerticalTabsPanel,
    OpenVerticalTabsPanel,
    ToggleVerticalTabsSettingsPopup,
    SetVerticalTabsDisplayGranularity(VerticalTabsDisplayGranularity),
    SetVerticalTabsTabItemMode(VerticalTabsTabItemMode),
    SetVerticalTabsViewMode(VerticalTabsViewMode),
    SetVerticalTabsPrimaryInfo(VerticalTabsPrimaryInfo),
    SetVerticalTabsCompactSubtitle(VerticalTabsCompactSubtitle),
    ToggleVerticalTabsShowPrLink,
    ToggleVerticalTabsShowDiffStats,
    ToggleVerticalTabsShowDetailsOnHover,
    /// Closes the focused panel. This happens as an explicit action from the user.
    ClosePanel,
    CopyTextToClipboard(String),
    /// Copies a path to the clipboard based on the focused pane: the open file's display path
    /// if the focused pane is the rendered file viewer (`FilePane`), otherwise the focused
    /// terminal session's working directory. No-op if neither yields a path.
    CopyCurrentPath,
    /// An action only registered in dev and local builds, which writes the user's current access
    /// token to the system clipboard to aid debugging and development.
    CopyAccessTokenToClipboard,
    DismissWorkspaceBanner(WorkspaceBanner),
    /// An action only registered in dev and local builds, which triggers a
    /// panic immediately when called.
    Panic,
    /// Writes a heap profile to disk.
    DumpHeapProfile,
    /// An action to open a new window with a view hierarchy debugger.
    OpenViewTreeDebugWindow,
    /// An action to either upgrade syncing status from none or just in one tab
    /// to syncing all tabs, or downgrade from syncing all tabs to no syncing
    ToggleSyncAllTerminalInputsInAllTabs,
    /// An action to either cancel syncing
    /// or switch from no syncing/syncing all tabs to syncing within one tab
    ToggleSyncTerminalInputsInTab,
    /// An action to force terminal input syncing off
    DisableTerminalInputSync,
    HandleConflictingWorkflow(SyncId),
    HandleConflictingEnvVarCollection(SyncId),
    OpenPromptEditor {
        open_source: PromptEditorOpenSource,
    },
    OpenCLIAgentToolbarEditor,
    OpenHeaderToolbarEditor,
    ShowHeaderToolbarContextMenu {
        position: Vector2F,
    },
    OpenLink(String),
    /// On WASM, opens a given URL in the desktop Torch app (if installed) or redirects to download page.
    #[cfg(target_family = "wasm")]
    OpenLinkOnDesktop(url::Url),
    ReopenClosedSession,
    AddWindow,
    AddWindowWithShell {
        shell: AvailableShell,
    },
    /// Moves focus to the panel on the left
    FocusLeftPanel,
    /// Moves focus to the panel on the right
    FocusRightPanel,
    /// Open a local path in the file explorer.
    OpenInExplorer {
        path: PathBuf,
    },
    /// Open a local file with the system's default application.
    OpenFilePath {
        path: PathBuf,
    },
    TerminateApp,
    CloseWindow,
    /// Help the user call the Torch executable with the [`crate::args::DEBUG_DUMP_FLAG`].
    DumpDebugInfo,
    /// Log review comment send eligibility for panes in the active tab.
    LogReviewCommentSendStatusForActiveTab,
    ToggleRecordingMode,
    ToggleInBandGenerators,
    ToggleDebugNetworkStatus,
    ToggleShowMemoryStats,
    RunAISuggestedCommand(String),
    RunCommand(String),
    InsertInInput {
        content: String,
        replace_buffer: bool,
        /// Whether to ensure agent mode is enabled when inserting content
        ensure_agent_mode: bool,
    },
    /// Open a new tab with its input in AI mode.
    NewTabInAgentMode {
        /// The entrypoint that triggered this action.
        entrypoint: AgentModeEntrypoint,
        /// The type of zero state prompt suggestion to start with (optional).
        zero_state_prompt_suggestion_type: Option<ZeroStatePromptSuggestionType>,
    },
    /// Open a new pane with its input in AI mode.
    NewPaneInAgentMode {
        /// The entrypoint that triggered this action.
        entrypoint: AgentModeEntrypoint,
        /// The type of zero state prompt suggestion to start with (optional).
        zero_state_prompt_suggestion_type: Option<ZeroStatePromptSuggestionType>,
    },
    /// Dismisses the Wayland crash recovery banner and opens a link to our docs page with more
    /// information.
    #[cfg(target_os = "linux")]
    DismissWaylandCrashRecoveryBannerAndOpenLink,
    /// Open a new pane with its input in AI mode
    /// with query "Fix this" with error name and details from AI summary.
    FixInAgentMode {
        query: String,
    },
    OpenAIFactCollection,
    ToggleAIDocumentPane {
        document_id: AIDocumentId,
        document_version: AIDocumentVersion,
    },
    /// Closes all visible AI document panes in the active pane group.
    HideAIDocumentPanes,
    /// Closes any other ai document panes in the active pane group, and opens the specified document_id.
    OpenAIDocumentPane {
        document_id: AIDocumentId,
        document_version: AIDocumentVersion,
    },
    FocusTerminalViewInWorkspace {
        terminal_view_id: EntityId,
    },
    /// Focus a specific pane by its locator (pane_group_id and pane_id).
    FocusPane(PaneViewLocator),
    /// Start a new AI conversation in a terminal view. This sets the pending query state
    /// to default and focuses the terminal view.
    StartNewConversation {
        terminal_view_id: EntityId,
    },
    /// Jump to the terminal pane of the most recent agent toast
    JumpToLatestToast,
    /// Open a file in a new tab with a code pane
    OpenFileInNewTab {
        full_path: PathBuf,
        line_and_column: Option<LineAndColumnArg>,
    },
    OpenNotebook {
        id: SyncId,
    },
    RunWorkflow {
        workflow: Arc<WorkflowType>,
        workflow_source: WorkflowSource,
        workflow_selection_source: WorkflowSelectionSource,
        argument_override: Option<HashMap<String, String>>,
    },
    ScrollToSettingsWidget {
        page: SettingsSection,
        widget_id: &'static str,
    },
    /// Navigate to an existing AI conversation, focusing on its terminal view.
    ///
    /// If the conversation is not in an open pane, restore it based on the layout setting or override.
    RestoreOrNavigateToConversation {
        pane_view_locator: Option<PaneViewLocator>,
        window_id: Option<WindowId>,
        conversation_id: AIConversationId,
        terminal_view_id: Option<EntityId>,
        /// If provided, use this layout to restore the conversation.
        /// Otherwise, fall back to the user's setting.
        restore_layout: Option<RestoreConversationLayout>,
    },
    /// Fork an existing AI conversation.
    /// Optionally summarizes the conversation after forking and/or sends an initial prompt.
    ForkAIConversation {
        conversation_id: AIConversationId,
        /// When Some, fork from the given response (or exchange if `fork_from_exact_exchange`
        /// is true). When None, fork from the last exchange.
        fork_from_exchange: Option<ForkFromExchange>,
        /// Whether to summarize the conversation after forking.
        summarize_after_fork: bool,
        /// Prompt to use for summarization when `summarize_after_fork` is true.
        summarization_prompt: Option<String>,
        /// Initial prompt to send in the forked conversation (sent after summarization if enabled).
        initial_prompt: Option<String>,
        /// Attachments (images/files) to send along with the initial prompt in the forked pane.
        initial_attachments: Vec<PendingAttachment>,
        /// Where to open the forked conversation.
        destination: ForkedConversationDestination,
    },
    /// Fork an existing AI conversation into a new pane and prefill the input with a local
    /// continuation command (selecting all text).
    #[cfg(not(target_family = "wasm"))]
    ContinueConversationLocally {
        conversation_id: AIConversationId,
    },
    /// Insert the /fork slash command into the active terminal's input.
    InsertForkSlashCommand,
    /// Summarize the active AI conversation in the focused pane.
    SummarizeAIConversation {
        prompt: Option<String>,
        /// Optional prompt to send after summarization completes successfully.
        initial_prompt: Option<String>,
    },
    /// Install the Torch Control CLI command to /usr/local/bin
    #[cfg(target_os = "macos")]
    InstallWarpctrl,
    /// Uninstall the Torch Control CLI command from /usr/local/bin
    #[cfg(target_os = "macos")]
    UninstallWarpctrl,
    UndoRevertInCodeReviewPane {
        window_id: WindowId,
        view_id: EntityId,
    },
    /// Handle a file being renamed in the file tree
    #[cfg(feature = "local_fs")]
    FileRenamed {
        old_path: PathBuf,
        new_path: PathBuf,
    },
    /// Handle a file being deleted in the file tree
    #[cfg(feature = "local_fs")]
    FileDeleted {
        path: PathBuf,
    },
    /// Open a repository directory via file picker. The `path` is an `Option` because some
    /// dispatchers don't know the path to open yet (so the Workspace must open the file picker).
    OpenRepository {
        path: Option<String>,
    },
    /// Open the native folder picker for a repo param in the tab-config modal after the
    /// current interaction cycle finishes.
    OpenTabConfigRepoPicker {
        param_index: usize,
    },
    /// Open a new blank code file in the current tab
    NewCodeFile,
    NavigatePrevPaneOrPanel,
    NavigateNextPaneOrPanel,
    ToggleProjectExplorer,
    OpenProjectExplorer,
    ToggleGlobalSearch,
    ToggleHiddenFiles,
    OpenGlobalSearch,
    ToggleConversationListView,
    OpenConversationListView,
    /// Reset the AWS Bedrock login banner dismissed state (for debugging).
    #[cfg(debug_assertions)]
    DebugResetAwsBedrockLoginBannerDismissed,
    /// Open the OpenWarp Launch Modal (for debugging)
    #[cfg(debug_assertions)]
    OpenOpenWarpLaunchModal,
    /// Reset the OpenWarp launch modal dismissed state (for debugging)
    #[cfg(debug_assertions)]
    ResetOpenWarpLaunchModalState,
    /// Open the Torch Agent CLI Launch Modal (for debugging)
    #[cfg(debug_assertions)]
    OpenAgentCliLaunchModal,
    /// Reset the Torch Agent CLI launch modal dismissed state (for debugging)
    #[cfg(debug_assertions)]
    ResetAgentCliLaunchModalState,
    /// Open the Feature Intro Modal (for debugging)
    #[cfg(debug_assertions)]
    OpenFeatureIntroModal,
    /// Reset the feature intro seen state (for debugging)
    #[cfg(debug_assertions)]
    ResetFeatureIntroModalState,
    /// Open the "You're using your ChatGPT plan" modal (for debugging)
    #[cfg(debug_assertions)]
    OpenChatGPTPlanModal,
    /// Reset the ChatGPT plan modal shown state (for debugging)
    #[cfg(debug_assertions)]
    ResetChatGPTPlanModalState,
    /// Take a process sample of the app (equivalent to Activity Monitor > Sample Process).
    #[cfg(target_os = "macos")]
    SampleProcess,
    ToggleNotificationMailbox {
        select_first: bool,
    },
    /// Show the rewind confirmation dialog before rewinding an AI conversation
    ShowRewindConfirmationDialog {
        ai_block_view_id: EntityId,
        exchange_id: AIAgentExchangeId,
        conversation_id: AIConversationId,
    },
    /// Execute the actual rewind after confirmation
    ExecuteRewindAIConversation {
        ai_block_view_id: EntityId,
        exchange_id: AIAgentExchangeId,
        conversation_id: AIConversationId,
    },
    /// Execute the actual deletion of a conversation after confirmation
    ExecuteDeleteConversation {
        conversation_id: AIConversationId,
        terminal_view_id: Option<EntityId>,
    },
    /// Open a full-window lightbox displaying the given images.
    OpenLightbox {
        images: Vec<lightbox::LightboxImage>,
        /// The index of the image to display initially.
        initial_index: usize,
    },
    /// Update a single image in the currently open lightbox.
    UpdateLightboxImage {
        index: usize,
        image: lightbox::LightboxImage,
    },
    ShowSessionConfigModal,
    /// Dismiss the non-blocking feature-intro popover without requiring it to hold focus.
    DismissFeatureIntroModal,
    /// Open the "New worktree" modal for creating a reusable worktree tab config.
    OpenNewWorktreeModal,
    /// Open the native folder picker for the repo field in the new-worktree modal.
    OpenNewWorktreeRepoPicker,
    /// Create a new worktree in the given repo using the default worktree tab config.
    /// The branch name is auto-generated.
    OpenWorktreeInRepo {
        repo_path: String,
    },
    /// Open a folder picker to add a new repo to PersistedWorkspace (from the
    /// "New worktree config" submenu's "+ Add new repo..." item).
    OpenWorktreeAddRepoPicker,
    SaveCurrentTabAsNewConfig(usize),
    SyncTrafficLights,
    /// Opens a tab config file in the editor and dismisses the associated error toast.
    OpenTabConfigErrorFile {
        path: PathBuf,
        toast_object_id: String,
    },
    /// Sidecar action: set the hovered item as the Cmd+T default.
    TabConfigSidecarMakeDefault {
        mode: crate::settings::ai::DefaultSessionMode,
        tab_config_path: Option<PathBuf>,
        shell: Option<AvailableShell>,
    },
    /// Sidecar action: open the tab config TOML in the user's editor.
    TabConfigSidecarEditConfig {
        path: PathBuf,
    },
    /// Sidecar action: show the remove confirmation dialog for a tab config.
    TabConfigSidecarRemoveConfig {
        name: String,
        path: PathBuf,
    },
    /// Opens the settings.toml file in a code editor pane.
    OpenSettingsFile,
    /// Opens a new agent session to fix settings.toml errors using the modify-settings skill.
    FixSettingsWithOz {
        error_description: String,
    },
    /// Opens (or focuses) the in-app network log pane as a right-split of the
    /// active pane group. Gated on `ContextFlag::NetworkLogConsole`.
    OpenNetworkLogPane,
}

impl WorkspaceAction {
    /// Matches what actions require the app state to be saved, and which don't. We match all
    /// actions directly, rather than using _, so we're forced to make a conscious decision for each
    /// of them, rather than following some default.
    pub fn should_save_app_state_on_action(&self) -> bool {
        use WorkspaceAction::*;
        match self {
            #[cfg(not(target_family = "wasm"))]
            ContinueConversationLocally { .. } => true,
            ActivateTab(_)
            | ActivateTabByNumber(_)
            | JumpToAttentionSession
            | SetTabShortcutModifierKey { .. }
            | ActivatePrevTab
            | ActivateNextTab
            | ActivateLastTab
            | CyclePrevSession
            | CycleNextSession
            | MoveActiveTabLeft
            | MoveActiveTabRight
            | MoveTabLeft(_)
            | MoveTabRight(_)
            | DropTab
            | DropGroup
            | RenameTab(_)
            | ResetTabName(_)
            | RenamePane(_)
            | ResetPaneName(_)
            | RenameActiveTab
            | RenameActivePane
            | SetActiveTabName(_)
            | CycleActiveTabColor
            | SetActiveTabColor(_)
            | CloseTab(_)
            | CloseActiveTab
            | CloseOtherTabs(_)
            | CloseNonActiveTabs
            | CloseTabsRight(_)
            | CloseTabsRightActiveTab
            | CloseTabGroup(_)
            | ToggleTabGroupCollapsed(_)
            | ToggleSidebarGroupCollapsed(_)
            | ToggleSidebarAgentsCollapsed
            | SidebarResized
            | RenameTabGroup(_)
            | NewTabGroupFromTab(_)
            | MoveTabToGroup { .. }
            | RemoveTabFromGroup(_)
            | NewTabGroupFromSelectedTabs
            | NewTabGroupFromActiveOrSelectedTabs
            | MoveSelectedTabsToGroup { .. }
            | RemoveSelectedTabsFromGroup
            | RemoveActiveOrSelectedTabsFromGroup
            | UngroupTabs(_)
            | NewTabInGroup(_)
            | MoveTabGroupUp(_)
            | MoveTabGroupDown(_)
            | CloseTabsOutsideGroup(_)
            | CloseTabsAboveGroup(_)
            | CloseTabsBelowGroup(_)
            | PinTab(_)
            | UnpinTab(_)
            | PinActiveTab
            | UnpinActiveTab
            | PinTabGroup(_)
            | UnpinTabGroup(_)
            | PinActiveTabGroup
            | UnpinActiveTabGroup
            | ToggleTabColor { .. }
            | ToggleTabGroupColor { .. }
            | AddDefaultTab
            | AddTerminalTab { .. }
            | NewSessionInDirectory(_)
            | NewAgentSessionInDirectory { .. }
            | NewAgentWorktreeSession { .. }
            | AddTabWithShell { .. }
            | AddAgentTab
            | AddWindow
            | AddWindowWithShell { .. }
            | CloseWindow
            | ScrollToSettingsWidget { .. }
            | NewTabInAgentMode { .. }
            | NewPaneInAgentMode { .. }
            | FixInAgentMode { .. }
            | OpenNotebook { .. }
            | RunWorkflow { .. }
            | OpenFileInNewTab { .. }
            | RestoreOrNavigateToConversation { .. }
            | NewCodeFile
            | ForkAIConversation { .. }
            | SummarizeAIConversation { .. }
            | OpenRepository { .. }
            | SelectTabConfig(_)
            | ToggleVerticalTabsPanel
            | OpenVerticalTabsPanel => true, // actions that actually change a state of the state of user's
            // workspace would most likely require a save, so that if the app gets
            // restarted, the user can continue working
            AutoupdateFailureLink
            | ApplyUpdate
            | CopyVersion(_)
            | DownloadNewVersion
            | ConfigureKeybindingSettings { .. }
            | ShowSettings
            | ShowSettingsPage(_)
            | ShowSettingsPageWithSearch { .. }
            | ShowThemeChooser(_)
            | ShowThemeChooserForActiveTheme
            | IncreaseFontSize
            | DecreaseFontSize
            | ResetFontSize
            | IncreaseZoom
            | DecreaseZoom
            | ResetZoom
            | OpenPalette { .. }
            | TogglePalette { mode: _, source: _ }
            | JoinSlack
            | ViewUserDocs
            | ViewLatestChangelog
            | ViewPrivacyPolicy
            | SendFeedback
            | ChangeCursor(_)
            | ToggleBlockSnackbar
            | ToggleErrorUnderlining
            | ToggleSyntaxHighlighting
            | OpenLaunchConfigSaveModal
            | ToggleTabRightClickMenu { .. }
            | ToggleTabSelectionRightClickMenu { .. }
            | ToggleTabGroupRightClickMenu { .. }
            | ToggleVerticalTabsPaneContextMenu { .. }
            | OpenNewSessionMenu { .. }
            | ToggleTabConfigsMenu
            | ToggleNewSessionMenu { .. }
            | ToggleProjectSessionMenu { .. }
            | ToggleProjectGroupMenu { .. }
            | ToggleDirectoryColor { .. }
            | SelectNewSessionMenuItem(_)
            | ToggleTabBarOverflowMenu
            | CheckForUpdate
            | SetA11yVerbosityLevel(_)
            | ToggleNotifications
            | DispatchToSettingsTab { .. }
            | ToggleResourceCenter
            | ToggleUserMenu
            | ToggleKeybindingsPage
            | ShowCommandSearch(_)
            | TriggerExternalCtrlTFileSearch
            | TriggerExternalAltCDirectorySearch
            | ToggleMouseReporting
            | ToggleScrollReporting
            | ToggleFocusReporting
            | OpenInExplorer { .. }
            | DragTab { .. }
            | CommitTabDrag { .. }
            | StartTabDrag
            | DragGroup { .. }
            | DragSidebarGroup { .. }
            | StartGroupDrag(_)
            | ToggleLeftPanel
            | ClosePanel
            | ToggleRightPanel
            | OpenCodeReviewPanel(..)
            | ToggleVerticalTabsSettingsPopup
            | SetVerticalTabsDisplayGranularity(_)
            | SetVerticalTabsTabItemMode(_)
            | SetVerticalTabsViewMode(_)
            | SetVerticalTabsPrimaryInfo(_)
            | SetVerticalTabsCompactSubtitle(_)
            | ToggleVerticalTabsShowPrLink
            | ToggleVerticalTabsShowDiffStats
            | ToggleVerticalTabsShowDetailsOnHover
            | ToggleWelcomeTips
            | CopyTextToClipboard(_)
            | CopyCurrentPath
            | CopyAccessTokenToClipboard
            | OpenTabConfigRepoPicker { .. }
            | OpenNewWorktreeModal
            | OpenNewWorktreeRepoPicker
            | OpenWorktreeInRepo { .. }
            | OpenWorktreeAddRepoPicker
            | Panic
            | DumpHeapProfile
            | OpenViewTreeDebugWindow
            | DismissWorkspaceBanner(..)
            | ToggleSyncAllTerminalInputsInAllTabs
            | ToggleSyncTerminalInputsInTab
            | DisableTerminalInputSync
            | HandleConflictingWorkflow(_)
            | HandleConflictingEnvVarCollection(_)
            | OpenPromptEditor { .. }
            | OpenCLIAgentToolbarEditor
            | OpenHeaderToolbarEditor
            | ShowHeaderToolbarContextMenu { .. }
            | OpenLink(_)
            | ReopenClosedSession
            | FocusLeftPanel
            | FocusRightPanel
            | DumpDebugInfo
            | LogReviewCommentSendStatusForActiveTab
            | ToggleRecordingMode
            | ToggleInBandGenerators
            | ToggleDebugNetworkStatus
            | ToggleShowMemoryStats
            | RunAISuggestedCommand { .. }
            | RunCommand { .. }
            | InsertInInput { .. }
            | InsertForkSlashCommand
            | OpenFilePath { .. }
            | TerminateApp
            | TabHoverWidthStart { .. }
            | TabHoverWidthEnd
            | OpenAIFactCollection
            | FocusTerminalViewInWorkspace { .. }
            | FocusPane(..)
            | ShiftSelectTabRange { .. }
            | ToggleTabMultiSelection { .. }
            | ClearTabMultiSelection
            | CancelActiveRename
            | StartNewConversation { .. }
            | UndoRevertInCodeReviewPane { .. }
            | JumpToLatestToast
            | NavigatePrevPaneOrPanel
            | NavigateNextPaneOrPanel
            | ToggleProjectExplorer
            | OpenProjectExplorer
            | ToggleGlobalSearch
            | ToggleHiddenFiles
            | OpenGlobalSearch
            | ToggleConversationListView
            | OpenConversationListView
            | ToggleNotificationMailbox { .. }
            | ToggleAIDocumentPane { .. }
            | HideAIDocumentPanes
            | OpenAIDocumentPane { .. }
            | ShowRewindConfirmationDialog { .. }
            | ExecuteRewindAIConversation { .. }
            | ExecuteDeleteConversation { .. }
            | OpenLightbox { .. }
            | UpdateLightboxImage { .. }
            | ShowSessionConfigModal
            | DismissFeatureIntroModal
            | SaveCurrentTabAsNewConfig(_)
            | SyncTrafficLights
            | OpenTabConfigErrorFile { .. }
            | TabConfigSidecarMakeDefault { .. }
            | TabConfigSidecarEditConfig { .. }
            | TabConfigSidecarRemoveConfig { .. }
            | OpenSettingsFile
            | FixSettingsWithOz { .. }
            | OpenNetworkLogPane => false,
            #[cfg(debug_assertions)]
            DebugResetAwsBedrockLoginBannerDismissed
            | OpenOpenWarpLaunchModal
            | ResetOpenWarpLaunchModalState
            | OpenAgentCliLaunchModal
            | ResetAgentCliLaunchModalState
            | OpenFeatureIntroModal
            | ResetFeatureIntroModalState
            | OpenChatGPTPlanModal
            | ResetChatGPTPlanModalState => false,
            #[cfg(not(target_family = "wasm"))]
            ViewLogs => false,
            #[cfg(target_os = "macos")]
            SampleProcess => false,
            #[cfg(target_os = "macos")]
            InstallWarpctrl | UninstallWarpctrl => false,
            #[cfg(feature = "local_fs")]
            FileRenamed { .. } => false, // File rename doesn't change workspace state
            #[cfg(feature = "local_fs")]
            FileDeleted { .. } => false, // File deletion doesn't change workspace state
            #[cfg(target_os = "linux")]
            DismissWaylandCrashRecoveryBannerAndOpenLink => false,
            #[cfg(target_family = "wasm")]
            OpenLinkOnDesktop(_) => false,
            // actions that are related to updating user settings or
            // managing some ui elements (like closing/opening modals)
            // that don't reflect on actual workspace and don't need to
            // be preserved between restarts.
        }
    }
}

#[cfg(test)]
#[path = "action_tests.rs"]
mod tests;
