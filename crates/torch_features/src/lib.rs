use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use enum_iterator::{Sequence, cardinality};
#[cfg(feature = "test-util")]
pub use overrides::{get_overrides, set_overrides};

#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug, Sequence)]
pub enum FeatureFlag {
    Changelog,
    DebugMode,
    Autoupdate,

    RuntimeFeatureFlags,

    /// Does grid storage go forwards or backwards
    SequentialStorage,

    /// If set, generators are executed in-band in all SSH sessions.
    InBandGeneratorsForSSH,

    /// If set, generators are executed using cmd.exe on Windows.
    RunGeneratorsWithCmdExe,

    /// Gates a bindable keyboard action for accepting command corrections.
    CommandCorrectionKey,

    /// If `true`, the "Show Initialization Block" menu item is added to the Blocks menu in the Mac
    /// menu bar.
    ToggleBootstrapBlock,

    /// Ligature Support in the Editor and Grid
    Ligatures,

    /// When enabled, the `History` rule from the command_corrections crate
    /// will be enabled. When the `History` rule is enabled, the command_corrections
    /// lib will use the user's history as a last-ditch effort to find a reasonable correction.
    CommandCorrectionsHistoryRule,

    /// Torch Agent Mode.
    AgentMode,

    /// A setting to enable a traditional completions experience.
    ClassicCompletions,

    /// Force enable classic completions.
    ForceClassicCompletions,

    /// If enabled, autosuggestions are hidden when the tab completions
    /// menu is open (except when using completions-as-you-type).
    RemoveAutosuggestionDuringTabCompletions,

    /// Feature flag for cursor reflow fix (fixes part of the Alacritty resizing logic).
    ResizeFix,

    /// Enable multiselect in Notebooks and Torch Text.
    RichTextMultiselect,

    /// Makes the input editor's prompt selectable.
    SelectablePrompt,

    /// Enables the settings file feature.
    SettingsFile,

    /// Enables rect selection.
    RectSelection,

    /// Adds Alacritty as a supported terminal to import settings from.
    AlacrittySettingsImport,

    /// Enables next action prediction within Torch, powered by AI.
    AgentPredict,

    /// Enables the shell selector, allowing us to open a new tab in
    /// a shell other than the default shell.
    ShellSelector,

    /// Enables the full-screen "zen mode" setting, where we hide the tab bar if there's only one
    /// tab.
    FullScreenZenMode,

    /// Playground for reducing Torch UI clutter.
    MinimalistUI,

    /// Enables support for using native shell completions to supplement our
    /// completion specs.
    NativeShellCompletions,

    /// Adds avatar to the tab bar.
    AvatarInTabBar,

    SshDragAndDrop,
    DragTabsToWindows,

    /// Maximizes data in flat storage to reduce memory usage.
    MaximizeFlatStorage,

    /// Recognizes the OSC 8 hyperlink escape sequence and makes the
    /// linked text Cmd+click-able.
    OscHyperlinks,

    ImeMarkedText,

    /// Enables iTerm image rendering
    ITermImages,

    /// Enables validation of autosuggestions.
    ValidateAutosuggestions,

    /// Enables using `esc` to clear autosuggestions.
    ClearAutosuggestionOnEscape,

    /// New, less intrusive autoupdate UI.
    AutoupdateUIRevamp,

    /// Enables Kitty image rendering
    KittyImages,

    /// If enabled, command palette searches will use Tantivy search instead of the default fuzzy search.
    UseTantivySearch,

    /// Enables image as context for AM.
    ImageAsContext,

    /// UNIX shells running "natively" on Windows via MSYS2.
    MSYS2Shells,

    /// Enables inline review comments on specific lines of code.
    ContextLineReviewComments,

    /// Enables the find/replace in code editor
    CodeFindReplace,

    /// Enables file search functionality in command palette
    CommandPaletteFileSearch,

    /// Enables close button on left side of tabs
    TabCloseButtonOnLeft,

    /// Enables the tabbed file viewer
    TabbedEditorView,

    /// Enables the prompt chip that displays the GitHub PR for the current branch.
    GithubPrPromptChip,

    /// Enables vim keybindings in the code editor.
    VimCodeEditor,

    /// Enables the ability to undo closed panes.
    UndoClosedPanes,

    /// Enables revert button for diff hunks in the gutter.
    RevertDiffHunk,

    /// Enables saving code review pane changes
    CodeReviewSaveChanges,

    /// Enables the file tree (with an entrypoint through code mode).
    FileTree,

    /// Enables ignoring input suggestions.
    AllowIgnoringInputSuggestions,

    /// Enables the one-time modal on app startup for existing users for the Code launch.
    CodeLaunchModal,

    /// Enables attaching diff sets (multiple hunks from multiple files) as context in Agent Mode.
    DiffSetAsContext,

    /// Enables file- and diff set-level comments in the code review header.
    FileAndDiffSetComments,

    /// Enables discarding per-file and discarding all changes
    DiscardPerFileAndAllChanges,

    /// Enables UI zoom support (scaling the entire UI by a given percentage).
    UIZoom,

    /// Enables find/search in code review pane
    CodeReviewFind,

    /// Enables asynchronous find in terminal, running search on a background thread.
    AsyncFind,

    /// Enables auto-opening code review pane on first agent change and its setting UI.
    AutoOpenCodeReviewPane,

    /// Enables inline code review functionality
    InlineCodeReview,

    /// Enables rendering Mermaid diagrams in markdown notebooks.
    MarkdownMermaid,
    /// Enables editable Mermaid diagrams to behave atomically in notebook and plan editors.
    EditableMarkdownMermaid,

    /// Enables rendering markdown tables in notebooks.
    MarkdownTables,

    /// Renders `.ipynb` (Jupyter) files as a formatted, read-only notebook in
    /// Torch's notebook viewer instead of showing the raw JSON in the code editor.
    JupyterNotebookRendering,

    /// Enables v2 of the context window usage UI.
    ContextWindowUsageV2,

    /// Enables global search
    GlobalSearch,

    /// Enables embedded code review comments.
    EmbeddedCodeReviewComments,

    AgentView,

    /// Enables the inline history menu for quickly accessing previous commands and conversations.
    InlineHistoryMenu,

    /// Enables Torch Managed Secrets functionality.

    /// Enables cloud conversation loading via the CLI --conversation flag.
    CloudConversations,

    /// Enables the "New agent" prompt chip in terminal mode when AgentView is enabled.
    ///
    /// When disabled (the default), the terminal message bar is shown instead.
    AgentViewPromptChip,

    /// Enables editing the agent input footer layout from the prompt context menu.
    AgentToolbarEditor,

    /// Enables configuring header toolbar item order, side placement, and visibility.
    ConfigurableToolbar,

    // Enables a side panel conversation list view for AgentView mode.
    AgentViewConversationListView,

    /// Enables pluggable notifications via OSC 9 and OSC 777 escape sequences.
    /// External programs can trigger system and in-app notifications.
    PluggableNotifications,

    /// When enabled, we expose LSP as a tool to the agent
    LSPAsATool,

    /// Gates populating/reading oz updates from channel versions in the changelog model.
    OzChangelogUpdates,

    /// Updated tab styling (background colors, border, close button positioning, margins).
    NewTabStyling,

    /// Enables the rich input editor for CLI agents (e.g., Claude Code).
    /// Ctrl-G intercepts the keystroke and opens Torch's input editor instead of $EDITOR.
    CLIAgentRichInput,

    /// Enables incremental (diff-based) buffer updates for auto-reload instead of full replace.
    IncrementalAutoReload,

    /// Enables scroll position preservation in the code review pane when file
    /// content changes via auto-reload.
    CodeReviewScrollPreservation,

    /// Re-enables local Claude Code and Codex child harnesses in orchestration
    /// flows while the default behavior temporarily keeps them disabled.
    LocalClaudeCodexChildHarnesses,

    /// Enables Kitty keyboard protocol support (CSI u encoding, progressive enhancement).
    KittyKeyboardProtocol,

    /// Enables header rows on all inline menus (label, tabs, resize handle).
    InlineMenuHeaders,

    /// Enables associating a tab color with a directory so tabs automatically
    /// adopt the configured color when their working directory matches.
    DirectoryTabColors,

    /// Enables vertical tab layout as an alternative to the horizontal tab bar.
    VerticalTabs,

    /// Enables attaching code review comments, diff hunk, and attach as context
    /// from code review + code editor for House Of Agents work
    HoaCodeReview,

    /// Enables the upgraded CLI agent session tracking and notifications infrastructure.
    HOANotifications,

    /// Enables the Codex Torch plugin marketplace integration.
    /// When disabled, Codex uses native OSC9 notifications.
    CodexPlugin,

    /// Enables tab configs — user-definable TOML templates for launching custom tab layouts.
    TabConfigs,

    /// Enables Torch local control through the standalone warpctrl CLI.
    WarpControlCli,

    /// Replaces the in-block warpification banner with a warpify footer.
    WarpifyFooter,

    /// Enables commit, push, and create-PR actions in the code review panel.
    GitOperationsInCodeReview,

    /// Trims trailing blank rows from CLI agent block output so unused vertical
    /// space is not rendered while the agent is running.
    TrimTrailingBlankLines,

    /// Gates the new SSH remote server flow that installs and connects to a
    /// persistent binary on the remote machine instead of using ControlMaster
    /// for command execution.
    SshRemoteServer,

    /// Enables summary mode in vertical tabs, showing condensed tab summaries
    /// instead of individual pane rows.
    VerticalTabsSummaryMode,
    /// Enables on-demand Factory repository checkouts and their credential scope.
    FactoryDeferredRepositories,
    /// Enables the code review view for remote sessions.
    RemoteCodeReview,

    /// Gates the Grouped Tabs feature.
    GroupedTabs,

    /// Gates the Pinned Tabs feature, which lets users pin individual tabs
    /// and whole tab groups so they stay at the front of the tab list and
    /// are protected from reordering.
    PinnedTabs,

    /// Enables state-mutating recovery for abnormal terminal lifecycle sequences.
    TerminalLifecycleRecovery,

    /// Renders supported solid box-drawing characters (`U+2500..=U+257F`)
    /// procedurally as cell-filling rectangles instead of from the font,
    /// eliminating seams between adjacent box-drawing cells in the terminal.
    BoxDrawingGlyphs,

    /// Uses fzf or atuin for history search instead of Torch's command search.
    ShellWidgetHandoff,

    /// Gates Ctrl+R / Command Search history ranking on match quality and usage priors (recency,
    /// session, exit status) plus whitespace space-AND tokenization, instead of Skim's raw
    /// fuzzy-match score against the whole query as a single pattern. Disabling this is a full
    /// return to the pre-APP-5650 history search behavior, not an approximation of it.
    HistorySearchRankingV2,

}

static FLAG_STATES: [AtomicBool; cardinality::<FeatureFlag>()] =
    [const { AtomicBool::new(false) }; { cardinality::<FeatureFlag>() }];

/// This map is populated by UserPreferences, which take precedence
/// over the global feature flag state.
static USER_PREFERENCE_MAP: [AtomicTriState; cardinality::<FeatureFlag>()] =
    [const { AtomicTriState::new() }; { cardinality::<FeatureFlag>() }];

/// Flag for whether or not feature flags have been globally initialized. Outside
/// of tests, this ensures that feature flags are only used after they're set
/// up by the app's `run_internal` function.
#[cfg(debug_assertions)]
static FEATURES_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Features used in debugging.
pub const DEBUG_FLAGS: &[FeatureFlag] = &[FeatureFlag::DebugMode, FeatureFlag::RuntimeFeatureFlags];
/// Features enabled only for the WarpLocal developer build.
pub const LOCAL_FLAGS: &[FeatureFlag] = &[FeatureFlag::LocalClaudeCodexChildHarnesses];

/// Features enabled for the development team.  The expectation is that, over
/// time, these will move on to PREVIEW_FLAGS before being launched.
pub const DOGFOOD_FLAGS: &[FeatureFlag] = &[
    FeatureFlag::ToggleBootstrapBlock,
    FeatureFlag::RemoveAutosuggestionDuringTabCompletions,
    FeatureFlag::ResizeFix,
    FeatureFlag::SshDragAndDrop,
    FeatureFlag::ImeMarkedText,
    FeatureFlag::MSYS2Shells,
    FeatureFlag::ContextLineReviewComments,
    FeatureFlag::RunGeneratorsWithCmdExe,
    FeatureFlag::FileAndDiffSetComments,
    // End manually enabled Code features.
    FeatureFlag::EditableMarkdownMermaid,
    FeatureFlag::CodeReviewScrollPreservation,
    #[cfg(not(windows))]
    FeatureFlag::SshRemoteServer,
    FeatureFlag::WarpControlCli,
    FeatureFlag::TerminalLifecycleRecovery,
    FeatureFlag::JupyterNotebookRendering,
    FeatureFlag::BoxDrawingGlyphs,
    FeatureFlag::FactoryDeferredRepositories,
];

/// Features enabled for feature preview build users (e.g.: Friends of Torch).
/// All PREVIEW_FLAGS are also automatically added to dogfood builds (WarpDev).
pub const PREVIEW_FLAGS: &[FeatureFlag] = &[];

/// Features enabled for all release builds (i.e.: everything but WarpLocal).
/// NOTE: if you are promoting a feature from Preview to launch, you'll likely
/// want to enable the feature by default in app/Cargo.toml, rather than add it to RELEASE_FLAGS.
pub const RELEASE_FLAGS: &[FeatureFlag] = &[
    FeatureFlag::Autoupdate,
    FeatureFlag::Changelog,
    FeatureFlag::ImeMarkedText,
    // Remote server binary is not yet supported on Windows.
    #[cfg(not(windows))]
    FeatureFlag::SshRemoteServer,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    FeatureFlag::DragTabsToWindows,
];

/// Flags that we want to allow to switch at runtime (assuming RuntimeFeatureFlags is set)
pub const RUNTIME_FEATURE_FLAGS: &[FeatureFlag] = &[FeatureFlag::LocalClaudeCodexChildHarnesses];

impl FeatureFlag {
    pub fn is_enabled(&self) -> bool {
        if cfg!(feature = "local_only") {
            if matches!(self, Self::VerticalTabs | Self::GroupedTabs) {
                return true;
            }
            if matches!(
                self,
                Self::AgentMode
                    | Self::AgentPredict
                    | Self::AgentView
                    | Self::AgentViewPromptChip
                    | Self::AgentViewConversationListView
                    | Self::AvatarInTabBar
                    | Self::CloudConversations
                    | Self::Changelog
                    | Self::OzChangelogUpdates
                    | Self::HOANotifications
                    | Self::CodeFindReplace
                    | Self::CommandPaletteFileSearch
                    | Self::TabbedEditorView
                    | Self::VimCodeEditor
                    | Self::FileTree
                    | Self::GlobalSearch
                    | Self::CodeReviewSaveChanges
                    | Self::CodeReviewFind
                    | Self::AutoOpenCodeReviewPane
                    | Self::InlineCodeReview
                    | Self::EmbeddedCodeReviewComments
                    | Self::HoaCodeReview
                    | Self::GitOperationsInCodeReview
                    | Self::RemoteCodeReview
                    | Self::JupyterNotebookRendering
            ) {
                return false;
            }
        }

        #[cfg(all(debug_assertions, not(feature = "test-util")))]
        {
            use std::sync::atomic::Ordering;
            assert!(
                FEATURES_INITIALIZED.load(Ordering::Relaxed),
                "Tried to check FeatureFlag::{self:?} before feature flags were initialized"
            );
        }

        overrides::get_override(*self)
            .or(USER_PREFERENCE_MAP[*self as usize].get())
            .or(Some(FLAG_STATES[*self as usize].load(Ordering::Relaxed)))
            .unwrap_or(false)
    }

    #[allow(dead_code)]
    pub fn set_enabled(self, enabled: bool) {
        // Allow calling this in integration tests because we sometimes use it in the app
        // during flows that integration tests cover.
        if cfg!(test) && cfg!(not(feature = "integration_tests")) {
            panic!(
                "Tried to globally enable {self:?} in a test. Use FeatureFlag::{self:?}.override_enabled instead"
            );
        }
        FLAG_STATES[self as usize].store(enabled, Ordering::Relaxed);
    }

    /// Sets a user preference for this flag. User preferences take precedence
    /// over the global feature flag state, and can be used to allow explicit opt-in
    /// and explicit opt-out behavior.
    pub fn set_user_preference(self, enabled: bool) {
        USER_PREFERENCE_MAP[self as usize].set(enabled);
    }

    /// Sets a thread-local test override for this flag. The override lasts
    /// until the returned guard is dropped.
    ///
    /// **Warning**: overrides do not work for tests of multi-threaded code. If
    /// you need to test multi-threaded code that's behind a feature flag, you'll
    /// need to set an override in _each_ thread.
    ///
    /// Tests should create overrides early on and allow them to be
    /// dropped automatically when they finish. This keeps overrides scoped to
    /// the duration of the test, since Rust doesn't have test lifecycle hooks.
    #[cfg(feature = "test-util")]
    pub fn override_enabled(self, enabled: bool) -> overrides::OverrideGuard {
        overrides::override_flag(self, enabled)
    }

    pub fn flag_description(&self) -> Option<&'static str> {
        use FeatureFlag::*;

        // Note: many feature flags are purposefully omitted from this list, in order to avoid blowing up
        // the Preview changelog. Features below which are enabled for Preview via PREVIEW_FLAGS, will be added to the changelog.
        // Features which are added to Stable should ideally have their feature flag removed entirely, but at the
        // very least, the feature flag should be removed from the Preview changelog by removing it from PREVIEW_FLAGS.
        // ** ONLY Preview-exclusive features should be added to this list! **
        match self {
            CodeReviewFind => Some("Enables the find bar in the code review pane."),
            GlobalSearch => Some("Enables global search in the left panel"),
            MarkdownTables => {
                Some("Enables rendering and interaction support for markdown tables in notebooks.")
            }
            SettingsFile => Some(
                "Enables configuring Torch via a user-editable `settings.toml` file, with hot reload and error reporting for invalid values.",
            ),
            GitOperationsInCodeReview => Some(
                "Enables commit, push, and create-PR actions directly from the code review panel.",
            ),
            _ => None,
        }
    }
}

/// Marks that feature flags have been globally initialized.
pub fn mark_initialized() {
    #[cfg(debug_assertions)]
    FEATURES_INITIALIZED.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(not(feature = "test-util"))]
mod overrides {
    #[inline(always)]
    pub fn get_override(_flag: super::FeatureFlag) -> Option<bool> {
        None
    }
}

/// Thread-local feature flag overrides for unit tests. For isolation, tests
/// should use overrides instead of globally modifying flags with [`super::FeatureFlag::set_enabled`].
#[cfg(feature = "test-util")]
mod overrides {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::FeatureFlag;

    thread_local! {
        static FLAG_OVERRIDES: RefCell<HashMap<FeatureFlag,bool>> = RefCell::new(HashMap::new());
    }

    /// RAII guard to set feature flag overrides in tests. When the guard is
    /// dropped, it reverts to the global flag state.
    #[must_use = "if unused the override will be immediately cleared"]
    pub struct OverrideGuard {
        flag: FeatureFlag,
    }

    /// Gets the overridden state for a flag, if set.
    pub fn get_override(flag: FeatureFlag) -> Option<bool> {
        FLAG_OVERRIDES.with(|overrides| overrides.borrow().get(&flag).copied())
    }

    /// Gets the set of overridden flags.
    pub fn get_overrides() -> HashMap<FeatureFlag, bool> {
        FLAG_OVERRIDES.with(|overrides| overrides.borrow().clone())
    }

    /// Applies a set of overrides.
    ///
    /// This is intended to be used with [`get_overrides`] to apply a set of
    /// existing overrides to a newly-spawned thread.  If you are trying to
    /// override a single feature flag, use [`FeatureFlag::override_enabled`]
    /// instead.
    pub fn set_overrides(new_overrides: HashMap<FeatureFlag, bool>) {
        FLAG_OVERRIDES.with(|overrides| *overrides.borrow_mut() = new_overrides);
    }

    /// Set a thread-local override for a flag.
    pub fn override_flag(flag: FeatureFlag, enabled: bool) -> OverrideGuard {
        set_override(flag, enabled);
        OverrideGuard { flag }
    }

    fn set_override(flag: FeatureFlag, enabled: bool) {
        FLAG_OVERRIDES.with(|overrides| {
            let previous = overrides.borrow_mut().insert(flag, enabled);
            // We could support nested overrides, but it requires some care around
            // out-of-order drops - if overrides are set and then cleared out of
            // order, what should the state after each drop be?
            if previous.is_some() {
                panic!("Multiple overrides set for {flag:?}");
            }
        });
    }

    fn clear_override(flag: FeatureFlag) {
        FLAG_OVERRIDES.with(|overrides| {
            let previous = overrides.borrow_mut().remove(&flag);
            if previous.is_none() {
                panic!("Cleared override for {flag:?}, but none was set");
            }
        });
    }

    impl Drop for OverrideGuard {
        fn drop(&mut self) {
            clear_override(self.flag);
        }
    }
}

/// An atomic tri-state value.
///
/// This is initially unset, and can be set to a true or false value.
///
/// Writes and reads use [`Ordering::Relaxed`], so should not be used for
/// synchronization.
struct AtomicTriState(AtomicU8);

impl AtomicTriState {
    const fn new() -> Self {
        Self(AtomicU8::new(TriState::Unset as u8))
    }

    fn get(&self) -> Option<bool> {
        TriState::from(self.0.load(Ordering::Relaxed)).into()
    }

    fn set(&self, value: bool) {
        self.0.store(TriState::from(value) as u8, Ordering::Relaxed);
    }
}

/// A simple enum representing a tristate, to be used as the backing type
/// for [`AtomicTriState`].
enum TriState {
    Unset = 0,
    False = 1,
    True = 2,
}

impl From<bool> for TriState {
    fn from(value: bool) -> Self {
        if value {
            TriState::True
        } else {
            TriState::False
        }
    }
}

impl From<u8> for TriState {
    fn from(value: u8) -> Self {
        match value {
            0 => TriState::Unset,
            1 => TriState::False,
            2 => TriState::True,
            _ => unreachable!(),
        }
    }
}

impl From<TriState> for Option<bool> {
    fn from(value: TriState) -> Self {
        match value {
            TriState::Unset => None,
            TriState::False => Some(false),
            TriState::True => Some(true),
        }
    }
}

#[cfg(test)]
#[path = "features_tests.rs"]
mod tests;
