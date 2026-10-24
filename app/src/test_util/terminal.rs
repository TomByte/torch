#[cfg(feature = "local_fs")]
#[cfg(feature = "local_fs")]
use repo_metadata::RepoMetadataModel;
use repo_metadata::repositories::DetectedRepositories;
use repo_metadata::watcher::DirectoryWatcher;
use std::sync::Arc;

use torch_core::ui::appearance::Appearance;
use torchui::platform::WindowStyle;
use torchui::{App, ViewHandle, WindowId};
use watcher::HomeDirectoryWatcher;

use super::settings::initialize_history_persistence_for_tests;
use crate::changelog_model::ChangelogModel;
use crate::code_review::git_repo_model::GitRepoModels;
use crate::context_chips::prompt::Prompt;
use crate::network::NetworkStatus;
use crate::persisted_workspace::PersistedWorkspace;
use crate::search::files::model::FileSearchModel;
use crate::settings::PrivacySettings;
use crate::settings_view::keybindings::KeybindingChangedNotifier;
use crate::suggestions::ignored_suggestions_model::IgnoredSuggestionsModel;
use crate::system::{SystemInfo, SystemStats};
use crate::terminal::alt_screen_reporting::AltScreenReporting;
use crate::terminal::cli_agent_sessions::CLIAgentSessionsModel;
use crate::terminal::model::block::SerializedBlock;
use crate::terminal::resizable_data::ResizableData;
use crate::terminal::{History, TerminalView};
use crate::undo_close::UndoCloseStack;
use crate::warp_managed_paths_watcher::WarpManagedPathsWatcher;
use crate::workflows::local_workflows::LocalWorkflows;
use crate::workspace::sync_inputs::SyncedInputState;
use crate::workspace::{ActiveSession, WorkspaceRegistry};
use crate::AgentNotificationsModel;

/// Initializes all of the necessary models to use a terminal view.
pub fn initialize_app_for_terminal_view(app: &mut App) {
    initialize_history_persistence_for_tests(app);

    app.add_singleton_model(|_| ChangelogModel::new(Arc::new(http_client::Client::new_for_test())));
    app.add_singleton_model(|_| NetworkStatus::new());
    app.add_singleton_model(|_| SystemStats::new());
    app.add_singleton_model(|_| Prompt::mock());
    app.add_singleton_model(|_| Appearance::mock());
    app.add_singleton_model(PrivacySettings::mock);
    app.add_singleton_model(|_ctx| SyncedInputState::mock());
    app.add_singleton_model(|_| ResizableData::default());
    app.add_singleton_model(LocalWorkflows::new);
    app.add_singleton_model(|_| History::default());
    app.add_singleton_model(|_| CLIAgentSessionsModel::new());
    app.add_singleton_model(AgentNotificationsModel::new);
    app.add_singleton_model(UndoCloseStack::new);

    app.add_singleton_model(|_| KeybindingChangedNotifier::new());
    app.add_singleton_model(|_| ActiveSession::default());
    app.add_singleton_model(DirectoryWatcher::new);
    app.add_singleton_model(|_| DetectedRepositories::default());
    #[cfg(feature = "local_fs")]
    app.add_singleton_model(|ctx| {
        RepoMetadataModel::new(ctx)
    });
    app.add_singleton_model(FileSearchModel::new);
    app.add_singleton_model(|_| GitRepoModels::new());
    app.add_singleton_model(HomeDirectoryWatcher::new_for_test);
    app.add_singleton_model(WarpManagedPathsWatcher::new_for_testing);

    #[cfg(not(target_family = "wasm"))]
    app.add_singleton_model(SystemInfo::new);

    app.add_singleton_model(|_| WorkspaceRegistry::new());
    app.add_singleton_model(|_| IgnoredSuggestionsModel::new(vec![]));
    app.add_singleton_model(PersistedWorkspace::new_for_test);

    AltScreenReporting::register(app);
}

/// Creates a window in `app` with a [`TerminalView`] as the root view.
/// Returns the handle to that terminal view.
pub fn add_window_with_terminal(
    app: &mut App,
    restored_blocks: Option<&[SerializedBlock]>,
) -> ViewHandle<TerminalView> {
    add_window_with_id_and_terminal(app, restored_blocks).1
}

/// Creates a window in `app` with a [`TerminalView`] as the root view.
/// Returns the WindowID and the handle to that terminal view.
pub fn add_window_with_id_and_terminal(
    app: &mut App,
    restored_blocks: Option<&[SerializedBlock]>,
) -> (WindowId, ViewHandle<TerminalView>) {
    let tips_model = app.add_model(|_| Default::default());
    app.add_window(WindowStyle::NotStealFocus, |ctx| {
        TerminalView::new_for_test(tips_model, restored_blocks, ctx)
    })
}
