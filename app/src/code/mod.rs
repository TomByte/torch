use std::any::Any;
use std::fmt::Debug;

use pathfinder_geometry::rect::RectF;
use torch_errors::{ErrorExt, register_error};
use torch_util::file::FileSaveError;
use torchui::AppContext;
use torchui::elements::DropTargetData;

#[cfg(not(target_family = "wasm"))]
pub mod find_references_view;
#[cfg(not(target_family = "wasm"))]
pub mod language_server_extension;
#[cfg_attr(not(target_family = "wasm"), path = "local_code_editor.rs")]
#[cfg_attr(target_family = "wasm", path = "local_code_editor_wasm.rs")]
pub mod local_code_editor;
#[cfg(not(target_family = "wasm"))]
pub use local_code_editor::ShowFindReferencesCard;
pub mod buffer_location;
pub mod editor;
pub mod editor_management;
pub mod global_buffer_model;
#[cfg(feature = "local_fs")]
pub mod language_server_shutdown_manager;
#[cfg(not(target_family = "wasm"))]
pub mod lsp_logs;
pub mod lsp_telemetry;

#[derive(Debug, thiserror::Error)]
#[cfg_attr(target_family = "wasm", allow(dead_code))]
pub enum ImmediateSaveError {
    #[error("No FileId")]
    NoFileId,
    #[error("failed to save file: {0:#}")]
    FailedToSave(#[from] FileSaveError),
    #[error("There is no file tab currently selected")]
    NoActiveFileTab,
    #[error("Remote session disconnected")]
    RemoteDisconnected,
}

impl ErrorExt for ImmediateSaveError {
    fn is_actionable(&self) -> bool {
        match self {
            ImmediateSaveError::NoFileId | ImmediateSaveError::NoActiveFileTab => true,
            ImmediateSaveError::FailedToSave(err) => err.is_actionable(),
            ImmediateSaveError::RemoteDisconnected => false,
        }
    }
}
register_error!(ImmediateSaveError);

/// Trait to determine whether we should show the comment editor based on state held
/// by the parent of the [`CodeEditorView`].
pub trait ShowCommentEditorProvider: Debug + 'static {
    /// Returns whether the comment editor should be shown given the location of the line where
    /// the editor would be shown.
    #[cfg_attr(not(feature = "local_fs"), allow(dead_code))]
    fn should_show_comment_editor(&self, editor_line_location: RectF, app: &AppContext) -> bool;
}

#[derive(Debug)]
struct NoopCommentEditorProvider;

impl ShowCommentEditorProvider for NoopCommentEditorProvider {
    fn should_show_comment_editor(&self, _editor_line_location: RectF, _app: &AppContext) -> bool {
        false
    }
}

/// Trait to determine whether we should show the find references card based on state held
/// by the parent of the [`CodeEditorView`].
pub trait ShowFindReferencesCardProvider: Debug + 'static {
    /// Returns whether the find references card should be shown given the location of the anchor
    /// point where the card would be positioned.
    #[cfg_attr(not(feature = "local_fs"), allow(dead_code))]
    fn should_show_find_references_card(
        &self,
        card_anchor_location: RectF,
        app: &AppContext,
    ) -> bool;
}

#[derive(Debug)]
pub struct NoopFindReferencesCardProvider;

impl ShowFindReferencesCardProvider for NoopFindReferencesCardProvider {
    fn should_show_find_references_card(
        &self,
        _card_anchor_location: RectF,
        _app: &AppContext,
    ) -> bool {
        false
    }
}

#[cfg_attr(target_family = "wasm", expect(dead_code))]
#[derive(Debug)]
pub enum SaveStatus {
    /// Save completed immediately and successfully.
    SavedImmediately,
    /// Save operation is in progress asynchronously (e.g., save-as dialog).
    AsyncSaveInProgress,
    /// Save failed with an error.
    Failed(#[allow(unused)] ImmediateSaveError),
}

#[derive(Debug, Eq, PartialEq)]
#[cfg_attr(target_family = "wasm", expect(dead_code))]
pub enum SaveOutcome {
    Canceled,
    Failed,
    Succeeded,
}

pub mod file_tree;
pub mod footer;
mod icon;

pub mod active_file;
pub mod opened_files;
pub use icon::icon_from_file_path;

#[cfg_attr(not(target_family = "wasm"), path = "view.rs")]
#[cfg_attr(target_family = "wasm", path = "wasm.rs")]
pub mod view;

/// Torch provides terminal sessions; file editing is handled by external applications.
pub fn editor_available() -> bool {
    !torch_core::channel::ChannelState::is_local_only()
}

pub fn init(app: &mut AppContext) {
    if !editor_available() {
        return;
    }
    self::view::init(app);
    self::file_tree::init(app);
    #[cfg(not(target_family = "wasm"))]
    self::find_references_view::init(app);
}

#[derive(Debug)]
#[cfg_attr(target_family = "wasm", expect(dead_code))]
pub struct EditorTabBarDropTargetData {
    index: usize,
}

impl DropTargetData for EditorTabBarDropTargetData {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
