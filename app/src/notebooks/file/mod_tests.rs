use std::sync::Arc;

use pathfinder_geometry::vector::vec2f;
#[cfg(feature = "local_fs")]
use repo_metadata::RepoMetadataModel;
use repo_metadata::repositories::DetectedRepositories;
use repo_metadata::watcher::DirectoryWatcher;
use string_offset::CharOffset;
use torch_core::features::FeatureFlag;
use torch_core::ui::appearance::Appearance;
use torch_editor::render::model::BlockItem;
#[cfg(feature = "local_fs")]
use torch_files::FileModel;
use torchui::keymap::Keystroke;
use torchui::platform::WindowStyle;
use torchui::{App, SingletonEntity, View};

use super::{
    FileNotebookAction, FileNotebookView, FileState, MarkdownDisplayMode,
    TOGGLE_MARKDOWN_DISPLAY_MODE_KEY_BINDING,
};
use crate::auth::AuthStateProvider;
use crate::auth::auth_manager::AuthManager;
use crate::cloud_object::model::persistence::CloudModel;
use crate::notebooks::context_menu::MenuSource;
use crate::notebooks::editor::keys::NotebookKeybindings;
use crate::notebooks::file::is_markdown_file;
use crate::search::files::model::FileSearchModel;
use crate::server::server_api::ServerApiProvider;
use crate::server::server_api::team::MockTeamClient;
use crate::server::server_api::workspace::MockWorkspaceClient;
use crate::server::telemetry::context_provider::AppTelemetryContextProvider;
use crate::settings_view::keybindings::KeybindingChangedNotifier;
use crate::terminal::model::session::Session;
use crate::test_util::settings::initialize_settings_for_tests;
use crate::workspace::ActiveSession;
use crate::workspaces::user_workspaces::UserWorkspaces;
use crate::{GlobalResourceHandles, GlobalResourceHandlesProvider};

fn init_app(app: &mut App) {
    initialize_settings_for_tests(app);

    let global_resource_handles = GlobalResourceHandles::mock(app);
    app.add_singleton_model(|_| GlobalResourceHandlesProvider::new(global_resource_handles));
    app.add_singleton_model(|_| Appearance::mock());
    app.add_singleton_model(|_| ActiveSession::default());
    app.add_singleton_model(|_| KeybindingChangedNotifier::new());
    app.add_singleton_model(DirectoryWatcher::new);
    app.add_singleton_model(|_| DetectedRepositories::default());
    #[cfg(feature = "local_fs")]
    app.add_singleton_model(RepoMetadataModel::new);
    app.add_singleton_model(FileSearchModel::new);
    app.add_singleton_model(FileModel::new);
    app.add_singleton_model(NotebookKeybindings::new);
    app.add_singleton_model(CloudModel::mock);
    app.add_singleton_model(|_| ServerApiProvider::new_for_test());
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());
    app.add_singleton_model(AppTelemetryContextProvider::new_context_provider);
    app.add_singleton_model(AuthManager::new_for_test);
    let team_client_mock = Arc::new(MockTeamClient::new());
    let workspace_client_mock = Arc::new(MockWorkspaceClient::new());
    app.add_singleton_model(|ctx| {
        UserWorkspaces::mock(
            team_client_mock.clone(),
            workspace_client_mock.clone(),
            vec![],
            ctx,
        )
    });
}

#[test]
fn test_load_static() {
    App::test((), |mut app| async move {
        init_app(&mut app);
        let (_, handle) = app.add_window(WindowStyle::NotStealFocus, FileNotebookView::new);

        handle.update(&mut app, |file_notebook, ctx| {
            file_notebook.open_static("Test Title", "Test Content", ctx);
            assert!(file_notebook.file_id.is_none());

            assert!(matches!(file_notebook.file_state, FileState::Loaded(_)));
            assert_eq!(file_notebook.title(), "Test Title");
            assert!(file_notebook.location.is_none());

            let editor = file_notebook.editor.as_ref(ctx);
            assert!(!editor.is_editable(ctx));
            // We don't want to check the actual README contents, but it should be clearly non-empty.
            assert!(editor.markdown(ctx).len() > 4);

            // Rendering should not panic.
            file_notebook.render(ctx);
        });
    });
}

#[test]
fn test_file_notebook_mermaid_blocks_default_to_rendered() {
    App::test((), |mut app| async move {
        init_app(&mut app);
        let _flag = FeatureFlag::MarkdownMermaid.override_enabled(true);
        let _editable_flag = FeatureFlag::EditableMarkdownMermaid.override_enabled(true);
        let (_, handle) = app.add_window(WindowStyle::NotStealFocus, FileNotebookView::new);

        handle.update(&mut app, |file_notebook, ctx| {
            file_notebook.open_static("Test Title", "```mermaid\ngraph TD\nA --> B\n```", ctx);
        });
        let render_state = handle.read(&app, |view, ctx| {
            view.editor
                .as_ref(ctx)
                .model()
                .as_ref(ctx)
                .render_state()
                .clone()
        });
        app.read(|ctx| render_state.as_ref(ctx).layout_complete())
            .await;
        app.read(|ctx| render_state.as_ref(ctx).layout_complete())
            .await;

        handle.read(&app, |view, ctx| {
            let editor = view.editor.as_ref(ctx);
            let model = editor.model().as_ref(ctx);
            let command = model
                .notebook_command_for_block(CharOffset::zero())
                .expect("Mermaid command should exist");
            assert_eq!(
                command.as_ref(ctx).mermaid_display_mode,
                MarkdownDisplayMode::Rendered
            );
            assert!(matches!(
                model
                    .render_state()
                    .as_ref(ctx)
                    .content()
                    .block_at_height(0.)
                    .map(|item| item.item),
                Some(BlockItem::MermaidDiagram { .. })
            ));
        });
    });
}

/// APP-5243: retrying and then discarding a failed open must not panic, and each attempt must
/// release the file state it opened rather than stacking it on the shared [`FileModel`].
#[cfg(feature = "local_fs")]
#[test]
fn test_reload_and_discard_after_failed_open() {
    use torchui::TypedActionView;

    /// Opens the notebook's current file and waits for the read to settle.
    async fn await_open(
        app: &mut torchui::App,
        handle: &torchui::ViewHandle<FileNotebookView>,
        open: impl FnOnce(&mut FileNotebookView, &mut torchui::ViewContext<FileNotebookView>),
    ) -> torch_util::file::FileId {
        let (file_id, future) = handle.update(app, |file_notebook, ctx| {
            open(file_notebook, ctx);
            let file_id = file_notebook.file_id.expect("File should have a file_id");
            let future_handle = FileModel::as_ref(ctx)
                .get_future_handle(file_id)
                .expect("Loading future should be present");
            (file_id, ctx.await_spawned_future(future_handle.future_id()))
        });
        future.await;
        file_id
    }

    App::test((), |mut app| async move {
        init_app(&mut app);
        let (_, handle) = app.add_window(WindowStyle::NotStealFocus, FileNotebookView::new);

        // A path that cannot be read, mirroring the `Could not read ...` treatment the reporter hit.
        let first_id = await_open(&mut app, &handle, |view, ctx| {
            view.open_local("app-5243-does-not-exist.md", None, ctx)
        })
        .await;

        handle.read(&app, |view, _| {
            assert!(
                matches!(view.file_state, FileState::Error(_)),
                "expected an error state, got {:?}",
                view.file_state
            );
        });

        // "Try again" in the error treatment.
        let second_id = await_open(&mut app, &handle, |view, ctx| {
            view.handle_action(&FileNotebookAction::ReloadFile, ctx)
        })
        .await;

        assert_ne!(first_id, second_id, "reload should open a fresh file id");
        app.read(|ctx| {
            assert!(
                FileModel::as_ref(ctx).file_path(first_id).is_none(),
                "reload should release the previous file id"
            );
        });
        handle.read(&app, |view, _| {
            assert!(
                matches!(view.file_state, FileState::Error(_)),
                "expected an error state after reload, got {:?}",
                view.file_state
            );
        });

        // Discarding the pane for good. Releasing is idempotent, so every teardown path can run it.
        handle.update(&mut app, |file_notebook, ctx| {
            file_notebook.release_file_model(ctx);
            file_notebook.release_file_model(ctx);
            assert!(file_notebook.file_id.is_none());
        });
        app.read(|ctx| {
            assert!(
                FileModel::as_ref(ctx).file_path(second_id).is_none(),
                "discarding the pane should release the open file id"
            );
        });
    });
}

#[test]
fn test_markdown_file_detection() {
    assert!(is_markdown_file("README.md"));
    assert!(is_markdown_file("DATABASE.MD"));
    assert!(is_markdown_file("notes.markdown"));
    assert!(is_markdown_file("README"));
    assert!(is_markdown_file("license"));
    assert!(is_markdown_file("CHANGELOG"));
    assert!(is_markdown_file("ReadMe"));

    assert!(!is_markdown_file("README.txt"));
    assert!(!is_markdown_file("main.rs"));
    assert!(!is_markdown_file("notes"));
}

#[test]
fn test_file_notebook_mermaid_context_menu_does_not_show_copy_image() {
    App::test((), |mut app| async move {
        init_app(&mut app);
        let (_, handle) = app.add_window(WindowStyle::NotStealFocus, FileNotebookView::new);

        handle.update(&mut app, |file_notebook, ctx| {
            file_notebook.open_static("Test Title", "```mermaid\ngraph TD\nA --> B\n```", ctx);

            let source = MenuSource::RichTextEditor {
                parent_offset: vec2f(0., 0.),
                editor: file_notebook.editor.clone(),
            };
            file_notebook.context_menu.show_context_menu(source, ctx);

            let item_names = file_notebook.context_menu.item_names(ctx);
            assert!(!item_names.contains(&"Copy image"));
        });
    });
}

#[test]
fn test_markdown_toggle_shortcut_only_handled_for_toggleable_files() {
    App::test((), |mut app| async move {
        init_app(&mut app);
        app.update(super::init);
        let (window_id, handle) = app.add_window(WindowStyle::NotStealFocus, FileNotebookView::new);
        let session = Arc::new(Session::test());
        let keystroke = Keystroke::parse(TOGGLE_MARKDOWN_DISPLAY_MODE_KEY_BINDING).unwrap();
        let press_shortcut = |app: &App| {
            app.dispatch_keystroke(window_id, &[handle.id()], &keystroke, false)
                .unwrap()
        };

        assert!(
            !press_shortcut(&app),
            "a pane with no file has no raw view to switch to"
        );

        handle.update(&mut app, |file_notebook, ctx| {
            file_notebook.open_local("../README.md", Some(session.clone()), ctx);
        });
        assert!(
            press_shortcut(&app),
            "a rendered Markdown file should be toggleable from the keyboard"
        );

        handle.update(&mut app, |file_notebook, ctx| {
            file_notebook.open_local("../Cargo.toml", Some(session), ctx);
        });
        assert!(
            !press_shortcut(&app),
            "a file without a Rendered/Raw toggle should not respond to the shortcut"
        );
    });
}
