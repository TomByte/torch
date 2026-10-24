use std::fmt;
use std::ops::Range;
use std::path::PathBuf;

use command_corrections::Correction;
use pathfinder_geometry::vector::Vector2F;
use torch_util::user_input::UserInput;
use torchui::elements::HyperlinkUrl;
use torchui::event::ModifiersState;
use torchui::units::Lines;

use super::inline_banner::{OpenInWarpBannerAction, VimModeBannerAction};
use super::{
    AliasExpansionBannerAction, ContextMenuAction, GridHighlightedLink, InputContextMenuAction,
    NotificationsDiscoveryBannerAction, NotificationsErrorBannerAction, TerminalEditor,
};
use crate::palette::PaletteSource;
use crate::terminal::available_shells::AvailableShell;
use crate::terminal::block_list_element::{
    BlockHoverAction, BlockListMenuSource, BlockSelectAction, BlockTextSelectAction,
};
use crate::terminal::block_list_viewport::OverhangingBlock;
use crate::terminal::model::SecretHandle;
use crate::terminal::model::completions::ShellCompletion;
use crate::terminal::model::index::Point;
use crate::terminal::model::mouse::MouseState;
use crate::terminal::model::selection::{SelectAction, SelectionDirection};
use crate::terminal::model::terminal_model::{BlockIndex, WithinModel};

/// This represents whether entering a subshell for a particular command should become automatic in
/// the future, or to ask again.
#[derive(Clone, Debug)]
pub enum RememberForWarpification {
    /// If yes, need to transmit the command itself so it can be persisted to user-defaults
    RememberSubshellCommand(String),
    RememberSSHHost(String),
    DoNotRememberSubshellCommand,
    DoNotRememberSSHHost,
}

impl RememberForWarpification {
    pub fn as_bool(&self) -> bool {
        match self {
            RememberForWarpification::RememberSubshellCommand(_) => true,
            RememberForWarpification::RememberSSHHost(_) => true,
            RememberForWarpification::DoNotRememberSubshellCommand => false,
            RememberForWarpification::DoNotRememberSSHHost => false,
        }
    }

    pub fn is_ssh(&self) -> bool {
        match self {
            RememberForWarpification::RememberSSHHost(_) => true,
            RememberForWarpification::DoNotRememberSSHHost => true,
            RememberForWarpification::RememberSubshellCommand(_) => false,
            RememberForWarpification::DoNotRememberSubshellCommand => false,
        }
    }
}

#[derive(Clone)]
pub enum TerminalAction {
    Scroll {
        delta: Lines,
    },
    AltScroll {
        delta: i32,
        point: Point,
    },
    ScrollToTopOfBlock {
        topmost_block: BlockIndex,
    },
    BlockTextSelect(BlockTextSelectAction),
    BlockSelect {
        action: BlockSelectAction,
        should_redetermine_focus: bool,
    },
    BlockHover(BlockHoverAction),
    BlockSnackbarHover {
        is_hovered: bool,
    },
    BlockNearSnackbarHover {
        is_hovered: bool,
    },

    // TODO: we should eventually use a Modifiers struct here instead of using
    // an aggregated is_selecting_blocks when we need better granularity.
    // This refactor will need to start from the Events themselves.
    ClickOnGrid {
        position: WithinModel<Point>,
        modifiers: ModifiersState,
    },
    MiddleClickOnGrid {
        /// `None` here means that the click was on the Block List but not on a particular blockgrid.
        position: Option<WithinModel<Point>>,
    },
    MiddleClickOnInput,
    MaybeLinkHover {
        position: Option<WithinModel<Point>>,
        from_editor: TerminalEditor,
    },
    MaybeHoverSecret {
        secret_handle: Option<SecretHandle>,
    },
    MaybeDismissToolTip {
        from_keybinding: bool,
    },
    AltScreenContextMenu {
        position: Vector2F,
    },
    AltSelect(SelectAction<Point>),
    MaybeClearAltSelect,
    AltMouseAction(MouseState),
    InsertCommandCorrection {
        correction: Correction,
    },
    BlockListContextMenu(BlockListMenuSource),
    CloseContextMenu,
    Paste,
    Copy,
    CopyOutputs,
    CopyCommands,
    CopyGitBranch,
    ReinputCommands,
    ReinputCommandsWithSudo,
    ClearBuffer,
    Focus,
    FocusInputAndClearSelection,
    ShowFindBar,
    SelectPriorBlock,
    SelectBookmarkDown,
    SelectBookmarkUp,
    BookmarkSelectedBlock,
    ScrollToBottomOfSelectedBlocks,
    ScrollToTopOfSelectedBlocks,
    ScrollToBottomOfOverhangingBlock(OverhangingBlock),
    SelectNextBlock,
    Up,
    OpenBlockListContextMenu,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    KeyboardSelectText(SelectionDirection),
    UserInputSequence(Vec<u8>),
    ControlSequence(Vec<u8>),
    RunNativeShellCompletions {
        buffer_text: String,
        results_tx:
            async_channel::Sender<(Vec<ShellCompletion>, Option<torch_completer::meta::Span>)>,
    },
    KeyDown(String),
    TypedCharacters(String),
    ContextMenu(ContextMenuAction),
    // IMPORTANT: Do not add a binding for ctrl_d, as we don't want this behavior to leak out to
    // parts of the terminal unrelated to the block list
    CtrlD,
    CtrlC,
    ClearSelectionsWhenShellMode,
    Close,
    ToggleMaximizePane,
    SplitRight(Option<AvailableShell>),
    SplitLeft(Option<AvailableShell>),
    SplitDown(Option<AvailableShell>),
    SplitUp(Option<AvailableShell>),
    /// The context menu that's used for the prompt directly above input editor
    PromptContextMenu {
        position_offset_from_prompt: Vector2F,
    },
    OpenInputContextMenu {
        position: Vector2F,
    },
    InputContextMenuItem(InputContextMenuAction),
    SelectAllBlocks,
    ExpandBlockSelectionAbove,
    ExpandBlockSelectionBelow,
    NotificationsDiscoveryBanner(NotificationsDiscoveryBannerAction),
    BookmarkBlock(BlockIndex),
    NotificationsErrorBanner(NotificationsErrorBannerAction),
    JumpToBookmark(BlockIndex),
    OpenGridLink(GridHighlightedLink),
    ToggleGridSecret {
        handle: WithinModel<SecretHandle>,
        show_secret: bool,
    },
    CopyGridSecret(WithinModel<SecretHandle>),
    ShowInFileExplorer(PathBuf),
    OpenFileInWarp(PathBuf),
    #[cfg(feature = "local_fs")]
    OpenCodeInWarp {
        path: PathBuf,
        layout: crate::util::file::external_editor::settings::EditorLayout,
        line_col: Option<torch_util::path::LineAndColumnArg>,
    },
    /// Starts a subshell in the active session.
    TriggerSubshellBootstrap,
    /// If the user says "no" to Warpification, possibly requesting not to be asked again
    DismissWarpifyBanner(RememberForWarpification),
    /// Triggers the banner asking to turn the running block into a subshell. The String is the
    /// command that the user entered.
    ShowSubshellBanner(String),
    InsertMostRecentCommandCorrection,
    AliasExpansionBanner(AliasExpansionBannerAction),
    OpenInWarpBanner(OpenInWarpBannerAction),
    OpenBlockFilterEditor(BlockIndex),
    ImportSettings,
    ToggleBlockFilterOnSelectedOrLastBlock,
    VimModeBanner(VimModeBannerAction),
    ToggleSnackbarInActivePane,
    DragAndDropFiles(Vec<String>),

    HyperlinkClick(HyperlinkUrl),
    StartFileDropTarget,
    StopFileDropTarget,
    SetMarkedText {
        marked_text: UserInput<String>,
        selected_range: Range<usize>,
    },
    ClearMarkedText,
    ShowInitializationBlock,
    ShowWarpifySettings,
    /// Removes a pending attachment (image or file) by index in the unified list.
    DeleteAttachment {
        index: usize,
    },
    /// Opens a pending input attachment image in the workspace lightbox before
    /// the attachment has been submitted with a user query.
    OpenAttachmentLightbox {
        index: usize,
    },
    AttachFile,
    ToggleCodeReviewPane,
    AddProjectAtCurrentDirectory,
    PickRepoToOpen,
    OpenFilesPalette {
        source: PaletteSource,
    },
    DismissCodeToolbeltTooltip,
    /// Start a Language Server for the current working directory (if supported)
    StartLspServer,
    /// Toggle the cloud mode conversation details panel
    OpenInlineHistoryMenu,
    /// Toggle PTY recording for this session.
    ToggleSessionRecording,
    /// Toggle the rich input editor for composing a prompt to send to a CLI agent.
    /// Triggered by Ctrl-G when a CLI agent is detected, or from the footer button.
    ToggleCLIAgentRichInput,

    /// Allow the blocked clipboard operation by adjusting the OSC 52 clipboard access setting.
    Osc52AllowBlockedClipboardOperation,
}

// Manually implementing Debug to avoid leaking sensitive information in logs
impl fmt::Debug for TerminalAction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use TerminalAction::*;

        match self {
            Scroll { delta } => write!(f, "Scroll {{ delta: {delta} }}"),
            AltScroll { delta, .. } => write!(f, "AltScroll {{ delta: {delta} }}"),
            ScrollToTopOfBlock { topmost_block } => write!(
                f,
                "JumpToPreviousCommand {{ topmost_block: {topmost_block} }}"
            ),
            ScrollToTopOfSelectedBlocks => f.write_str("ScrollToTopOfSelectedBlocks"),
            ScrollToBottomOfSelectedBlocks => f.write_str("ScrollToBottomOfSelectedBlocks"),
            ScrollToBottomOfOverhangingBlock(overhanging_block) => {
                write!(f, "ScrollToBottomOfOverhangingBlock {overhanging_block:?}")
            }
            BlockTextSelect(action) => write!(f, "BlockTextSelect({action:?})"),
            BlockSelect { action, .. } => write!(f, "BlockSelect({action:?})"),
            BlockHover(action) => write!(f, "BlockHover({action:?})"),
            BlockSnackbarHover { is_hovered } => {
                write!(f, "BlockSnackbarHover{{ is_hovered {is_hovered} }}")
            }
            BlockNearSnackbarHover { is_hovered } => {
                write!(f, "BlockNearSnackbarHover{{ is_hovered {is_hovered} }}")
            }
            ClickOnGrid {
                position,
                modifiers,
            } => write!(
                f,
                "ClickOnGrid {{ position: {position:?}, modifiers: {modifiers:?} }}"
            ),
            MaybeLinkHover {
                position,
                from_editor,
            } => write!(
                f,
                "MaybeLinkHover {{ position: {position:?}, from_editor: {from_editor:?} }}"
            ),
            MaybeHoverSecret { secret_handle } => {
                write!(f, "MaybeHoverSecret {{ secret_handle: {secret_handle:?} }}")
            }
            MaybeDismissToolTip { from_keybinding } => write!(
                f,
                "MaybeDismissToolTip {{ from_keybinding: {from_keybinding:?}}}"
            ),
            AltSelect(action) => write!(f, "AltSelect({action:?})"),
            MaybeClearAltSelect => f.write_str("MaybeClearAltSelect"),
            AltMouseAction(action) => write!(f, "AltMouseAction({action:?})"),
            AltScreenContextMenu { position } => {
                write!(f, "AltScreenContextMenu {{ position: {position:?} }}")
            }
            BlockListContextMenu(menu) => write!(f, "BlockListContextMenu({menu:?})"),
            CloseContextMenu => f.write_str("CloseContextMenu"),
            Paste => f.write_str("Paste"),
            Copy => f.write_str("Copy"),
            CopyOutputs => f.write_str("CopyOutputs"),
            CopyCommands => f.write_str("CopyCommands"),
            CopyGitBranch => f.write_str("CopyGitBranch"),
            ReinputCommands => f.write_str("ReinputCommands"),
            ReinputCommandsWithSudo => f.write_str("ReinputCommandsWithSudo"),
            ClearBuffer => f.write_str("ClearBuffer"),
            SelectBookmarkUp => f.write_str("SelectBookmarkUp"),
            SelectBookmarkDown => f.write_str("SelectBookmarkDown"),
            Focus => f.write_str("Focus"),
            FocusInputAndClearSelection => f.write_str("FocusInputAndClearSelection"),
            ShowFindBar => f.write_str("ShowFindBar"),
            SelectPriorBlock => f.write_str("SelectPriorBlock"),
            SelectNextBlock => f.write_str("SelectNextBlock"),
            BookmarkSelectedBlock => f.write_str("BookmarkSelectedBlock"),
            Up => f.write_str("Up"),
            Down => f.write_str("Down"),
            PageUp => f.write_str("PageUp"),
            PageDown => f.write_str("PageDown"),
            Home => f.write_str("Home"),
            End => f.write_str("End"),
            KeyboardSelectText(direction) => write!(f, "KeyboardSelectText({direction:?})"),
            ContextMenu(action) => write!(f, "ContextMenu({action:?})"),
            CtrlD => f.write_str("CtrlD"),
            CtrlC => f.write_str("CtrlC"),
            ClearSelectionsWhenShellMode => {
                f.write_str("ClearSelectionsWhenShellMode(TerminalAction)")
            }
            Close => f.write_str("Close"),
            SplitRight(_) => f.write_str("SplitRight"),
            SplitLeft(_) => f.write_str("SplitLeft"),
            SplitDown(_) => f.write_str("SplitDown"),
            SplitUp(_) => f.write_str("SplitUp"),
            ToggleMaximizePane => f.write_str("ToggleMaximizeActivePane"),
            PromptContextMenu {
                position_offset_from_prompt,
            } => write!(
                f,
                "PromptContextMenu {{ position_offset_from_prompt: {position_offset_from_prompt:?} }}"
            ),
            OpenInputContextMenu { position } => {
                write!(f, "OpenInputContextMenu {{ position: {position:?} }}")
            }
            InputContextMenuItem(action) => write!(f, "InputContextMenuItem({action:?})"),
            SelectAllBlocks => f.write_str("SelectAllBlocks"),
            ExpandBlockSelectionAbove => f.write_str("ExpandBlockSelectionAbove"),
            ExpandBlockSelectionBelow => f.write_str("ExpandBlockSelectionBelow"),
            UserInputSequence(_) => f.write_str("UserInputSequence"),
            ControlSequence(_) => f.write_str("ControlSequence"),
            KeyDown(_) => f.write_str("KeyDown"),
            TypedCharacters(_) => f.write_str("TypedCharacters"),
            NotificationsDiscoveryBanner(action) => {
                write!(f, "NotificationsDiscoveryBanner({action:?})")
            }
            BookmarkBlock(index) => {
                write!(f, "BookmarkBlock({index:?})")
            }
            NotificationsErrorBanner(action) => write!(f, "NotificationsErrorBanner({action:?})"),
            JumpToBookmark(index) => write!(f, "JumpToBookmark({index:?})"),
            InsertCommandCorrection { .. } => {
                write!(f, "InsertCommandCorrection",)
            }
            OpenGridLink(_) => f.write_str("OpenGridLink"),
            ToggleGridSecret { show_secret, .. } => write!(f, "ToggleGridSecret {show_secret:?}"),
            CopyGridSecret(_) => f.write_str("CopyGridSecret"),
            ShowInFileExplorer(_) => f.write_str("ShowInFileExplorer"),
            OpenFileInWarp(_) => f.write_str("OpenFileInWarp"),
            #[cfg(feature = "local_fs")]
            OpenCodeInWarp { .. } => f.write_str("OpenCodeInWarp"),
            OpenBlockListContextMenu => f.write_str("OpenBlockListContextMenu"),
            TriggerSubshellBootstrap => f.write_str("TriggerSubshellBootstrap"),
            DismissWarpifyBanner(remember) => write!(f, "DismissWarpifyBanner({remember:?})"),
            ShowSubshellBanner(_) => f.write_str("ShowSubshellBanner"),
            InsertMostRecentCommandCorrection => f.write_str("InsertMostRecentCommandCorrection"),
            AliasExpansionBanner(action) => write!(f, "AliasExpansionBanner({action:?}"),
            OpenInWarpBanner(action) => write!(f, "OpenInWarpBanner({action:?})"),
            OpenBlockFilterEditor(block_index) => {
                write!(f, "OpenBlockFilterEditor({block_index:?})")
            }
            ImportSettings => write!(f, "ImportSettings"),
            ToggleBlockFilterOnSelectedOrLastBlock => {
                f.write_str("ToggleBlockFilterOnSelectedOrLastBlock")
            }
            VimModeBanner(action) => write!(f, "VimModeBanner({action:?})"),
            ToggleSnackbarInActivePane => write!(f, "ToggleSnackbarInActivePane"),
            MiddleClickOnGrid { position } => {
                write!(f, "MiddleClickonGrid {{ position: {position:?} }}")
            }
            MiddleClickOnInput => write!(f, "MiddleClickOnInput"),
            DragAndDropFiles(_) => write!(f, "DragAndDropFiles"),
            HyperlinkClick(hyperlink_url) => write!(f, "HyperlinkClick({hyperlink_url:?})"),
            StartFileDropTarget => write!(f, "StartFileDropTarget"),
            StopFileDropTarget => write!(f, "StopFileDropTarget"),
            RunNativeShellCompletions { buffer_text, .. } => {
                write!(f, "RunNativeShellCompletions({buffer_text:?})")
            }
            SetMarkedText {
                marked_text,
                selected_range,
            } => write!(f, "SetMarkedText {{{marked_text:?}, {selected_range:?}}}"),
            ClearMarkedText => write!(f, "ClearMarkedText"),
            ShowInitializationBlock => write!(f, "ShowInitializationBlock"),
            ShowWarpifySettings => write!(f, "ShowWarpifySettings"),
            DeleteAttachment { index } => write!(f, "DeleteAttachment({index:?})"),
            OpenAttachmentLightbox { index } => {
                write!(f, "OpenAttachmentLightbox({index:?})")
            }
            AttachFile => write!(f, "AttachFile"),
            ToggleCodeReviewPane => write!(f, "ToggleCodeReviewPane"),
            AddProjectAtCurrentDirectory => write!(f, "AddProjectAtCurrentDirectory"),
            PickRepoToOpen => write!(f, "PickRepoToOpen"),
            OpenFilesPalette { .. } => write!(f, "OpenFilesPalette"),
            DismissCodeToolbeltTooltip => write!(f, "DismissCodeToolbeltTooltip"),
            StartLspServer => write!(f, "StartLspServer"),
            OpenInlineHistoryMenu => write!(f, "OpenInlineHistoryMenu"),
            ToggleSessionRecording => write!(f, "ToggleSessionRecording"),
            ToggleCLIAgentRichInput => write!(f, "ToggleCLIAgentRichInput"),
            Osc52AllowBlockedClipboardOperation => {
                write!(f, "Osc52AllowBlockedClipboardOperation")
            }
        }
    }
}
