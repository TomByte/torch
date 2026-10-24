use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use torch_util::path::LineAndColumnArg;
use torchui::{AppContext, Entity, EntityId, SingletonEntity, ViewHandle, WindowId};

use super::buffer_location::LocalOrRemotePath;
use super::view::CodeView;
use crate::code_review::code_review_view::CodeReviewView;
use crate::pane_group::{PaneGroup, PaneId};
use crate::workspace::PaneViewLocator;

pub struct CodeEditorSummary<'a> {
    pub unsaved_changes: Vec<&'a CodeEditorStatus>,
}

impl<'a> CodeEditorSummary<'a> {
    /// Create a summary from the currently open Code Editors.
    pub fn new(editors: &'a [CodeEditorStatus]) -> Self {
        let unsaved_changes = editors
            .iter()
            .filter(|editor| editor.unsaved_changes)
            .collect();

        Self { unsaved_changes }
    }
}

#[derive(Copy, Clone)]
pub struct CodeEditorStatus {
    unsaved_changes: bool,
}

impl CodeEditorStatus {
    pub fn new(unsaved_changes: bool) -> Self {
        Self { unsaved_changes }
    }

    /// Fetches all code editors open in the App.
    pub fn all_editors(app: &AppContext) -> impl Iterator<Item = Self> + '_ {
        app.window_ids()
            .flat_map(move |window_id| Self::editors_in_window(window_id, app))
    }

    /// Fetches all code editors in a given window.
    pub fn editors_in_window(
        window_id: WindowId,
        app: &AppContext,
    ) -> impl Iterator<Item = Self> + '_ {
        app.views_of_type::<CodeView>(window_id)
            .into_iter()
            .flat_map(move |editors| {
                editors
                    .into_iter()
                    .map(move |editor| Self::editor_status(&editor, app))
            })
    }

    /// Fetches all code editors in a given tab.
    pub fn editors_in_tab<'a>(
        tab: &ViewHandle<PaneGroup>,
        app: &'a AppContext,
    ) -> impl Iterator<Item = Self> + 'a + use<'a> {
        tab.as_ref(app)
            .code_panes(app)
            .map(move |(_, editor)| Self::editor_status(&editor, app))
    }

    pub fn editor_status(editor: &ViewHandle<CodeView>, app: &AppContext) -> Self {
        editor.read(app, |editor_view, ctx| Self {
            unsaved_changes: editor_view.contains_unsaved_changes(ctx),
        })
    }

    pub fn status_for_code_review(review: &ViewHandle<CodeReviewView>, app: &AppContext) -> Self {
        review.read(app, |review_view, ctx| Self {
            unsaved_changes: review_view.has_unsaved_changes(ctx),
        })
    }

    /// Fetches all code review views in a given window (including panel views).
    pub fn code_review_views_in_window(
        window_id: WindowId,
        app: &AppContext,
    ) -> impl Iterator<Item = Self> + '_ {
        app.views_of_type::<CodeReviewView>(window_id)
            .into_iter()
            .flat_map(move |editors| {
                editors
                    .into_iter()
                    .map(move |editor| Self::status_for_code_review(&editor, app))
            })
    }
}

#[derive(Debug, Hash, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub enum CodeSource {
    /// A new code pane not attached to an existing file.
    New {
        /// When the new file is saved, open the file picker to this directory.
        default_directory: Option<PathBuf>,
    },
    /// Opened from file links.
    Link {
        path: PathBuf,
        range_start: Option<LineAndColumnArg>,
        range_end: Option<LineAndColumnArg>,
    },
    /// Opened from project rules (WARP.md) file.
    ProjectRules { location: LocalOrRemotePath },
    /// Opened from file tree (local or remote).
    FileTree { location: LocalOrRemotePath },
    /// Opened from command palette file search (local or remote).
    CommandPalette { location: LocalOrRemotePath },
    /// Opened from macOS Finder via "Open With".
    Finder { path: PathBuf },
}

impl CodeSource {
    pub fn default_directory(&self) -> Option<&PathBuf> {
        match self {
            Self::New {
                default_directory, ..
            } => default_directory.as_ref(),
            Self::Link { .. }
            | Self::ProjectRules { .. }
            | Self::FileTree { .. }
            | Self::CommandPalette { .. }
            | Self::Finder { .. } => None,
        }
    }

    pub fn path(&self) -> Option<PathBuf> {
        match self {
            Self::New { .. } => None,
            Self::FileTree { location, .. } | Self::CommandPalette { location, .. } => {
                match location {
                    LocalOrRemotePath::Local(path) => Some(path.clone()),
                    LocalOrRemotePath::Remote(_) => None,
                }
            }
            Self::Link { path, .. } | Self::Finder { path } => Some(path.clone()),
            Self::ProjectRules { location } => location.to_local_path().map(Path::to_path_buf),
        }
    }

    /// Returns the `LocalOrRemotePath` for file tree sources.
    pub fn file_location(&self) -> Option<&LocalOrRemotePath> {
        match self {
            Self::FileTree { location } | Self::CommandPalette { location } => Some(location),
            _ => None,
        }
    }

    /// Returns the `LocalOrRemotePath` for any source that has a backing file.
    ///
    /// Unlike `path()` (which only returns local paths) and `file_location()`
    /// (which only covers `FileTree`), this covers every variant that maps to
    /// a file — local or remote.
    pub fn location(&self) -> Option<LocalOrRemotePath> {
        match self {
            Self::New { .. } => None,
            Self::FileTree { location } | Self::CommandPalette { location } => {
                Some(location.clone())
            }
            Self::Link { path, .. } | Self::Finder { path } => {
                Some(LocalOrRemotePath::Local(path.clone()))
            }
            Self::ProjectRules { location } => Some(location.clone()),
        }
    }

    pub fn omit_line_col(&self) -> CodeSource {
        if let CodeSource::Link { path, .. } = self {
            CodeSource::Link {
                path: path.clone(),
                range_start: None,
                range_end: None,
            }
        } else {
            self.clone()
        }
    }

    /// Returns `true` if this source should be restored across app restarts.
    ///
    pub fn is_restorable(&self) -> bool {
        !matches!(
            self,
            Self::FileTree {
                location: LocalOrRemotePath::Remote(_),
            } | Self::CommandPalette {
                location: LocalOrRemotePath::Remote(_),
            } | Self::ProjectRules {
                location: LocalOrRemotePath::Remote(_),
            }
        )
    }
}

struct CodePaneData {
    #[allow(unused)]
    window_id: WindowId,
    #[allow(unused)]
    locator: PaneViewLocator,
}

/// Singleton model for managing the state of open code panes. It is responsible for
/// 1) Allow caller to find an open code pane if exists.
/// 2) Allow other sources to listen for events emitted when code pane is closed.
#[derive(Default)]
pub struct CodeManager {
    source_to_pane_data: HashMap<CodeSource, CodePaneData>,
}

impl CodeManager {
    /// Register a new pane in the code manager.
    pub fn register_pane(
        &mut self,
        pane_group_id: EntityId,
        window_id: WindowId,
        pane_id: PaneId,
        source: CodeSource,
    ) {
        let entry = self.source_to_pane_data.entry(source.omit_line_col());
        if let Entry::Vacant(entry) = entry {
            entry.insert(CodePaneData {
                window_id,
                locator: PaneViewLocator {
                    pane_group_id,
                    pane_id,
                },
            });
        } else {
            log::warn!("Ignoring duplicate code pane registration");
        }
    }

    /// De-register an open code pane when it's removed from a pane group.
    pub fn deregister_pane(&mut self, source: &CodeSource) {
        self.source_to_pane_data.remove(&source.omit_line_col());
    }

    /// Returns the locator for a code pane that already has the given `LocalOrRemotePath`
    /// open in the given pane group. Works for both local and remote files.
    pub fn get_locator_for_location_in_tab(
        &self,
        pane_group_id: EntityId,
        location: &LocalOrRemotePath,
    ) -> Option<PaneViewLocator> {
        self.source_to_pane_data
            .iter()
            .find(|(source, data)| {
                data.locator.pane_group_id == pane_group_id
                    && source.location().as_ref() == Some(location)
            })
            .map(|(_, data)| data.locator)
    }
}

impl Entity for CodeManager {
    type Event = ();
}

impl SingletonEntity for CodeManager {}
