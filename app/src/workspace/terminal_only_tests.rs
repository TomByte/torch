use torchui::{App, TypedActionView};

use super::*;

#[test]
fn terminal_only_restoration_replaces_legacy_file_tabs_with_a_terminal() {
    App::test((), |mut app| async move {
        tests::initialize_app(&mut app);
        let workspace = tests::mock_workspace(&mut app);
        workspace.update(&mut app, |workspace, ctx| {
            let snapshot = PaneNodeSnapshot::Leaf(LeafSnapshot {
                is_focused: true,
                custom_vertical_tabs_title: None,
                contents: LeafContents::Notebook(NotebookPaneSnapshot::LocalFileNotebook {
                    path: Some(PathBuf::from("/old/project/README.md")),
                }),
            });
            workspace.add_tab_with_pane_layout(
                PanesLayout::Snapshot(Box::new(snapshot)),
                Arc::new(HashMap::new()),
                None,
                ctx,
            );
            assert!(
                workspace
                    .active_tab_pane_group()
                    .as_ref(ctx)
                    .focused_session_view(ctx)
                    .is_some()
            );
        });
    });
}

#[test]
fn terminal_only_palette_has_sessions_and_projects_without_file_search() {
    App::test((), |mut app| async move {
        tests::initialize_app(&mut app);
        let workspace = tests::mock_workspace(&mut app);
        workspace.update(&mut app, |workspace, ctx| {
            workspace.palette.update(ctx, |palette, ctx| {
                let mixer = palette.search_bar.as_ref(ctx).mixer().clone();
                palette.data_source_store.update(ctx, |store, ctx| {
                    store.reset_search_mixer(mixer.clone(), ctx);
                });
                let filters = mixer
                    .as_ref(ctx)
                    .registered_filters()
                    .collect::<HashSet<_>>();
                assert!(filters.contains(&QueryFilter::Sessions));
                assert!(filters.contains(&QueryFilter::Repos));
                assert!(filters.contains(&QueryFilter::Actions));
                assert!(!filters.contains(&QueryFilter::Files));
                assert!(!filters.contains(&QueryFilter::Code));
            });
        });
    });
}

#[test]
fn terminal_only_bindings_and_actions_cannot_open_editor_surfaces() {
    App::test((), |mut app| async move {
        tests::initialize_app(&mut app);
        let workspace = tests::mock_workspace(&mut app);
        workspace.update(&mut app, |workspace, ctx| {
            let names = ctx
                .get_key_bindings()
                .map(|binding| binding.name.to_owned())
                .collect::<HashSet<_>>();
            for name in [
                NEW_FILE_BINDING_NAME,
                TOGGLE_RIGHT_PANEL_BINDING_NAME,
                LEFT_PANEL_PROJECT_EXPLORER_BINDING_NAME,
                "workspace:toggle_files_palette",
                "workspace:show_settings_code_page",
                "file_tree:toggle_hidden_files",
            ] {
                assert!(!names.contains(name), "{name} must not be exposed");
            }
            let tabs = workspace.tabs.len();
            for action in [
                WorkspaceAction::NewCodeFile,
                WorkspaceAction::ToggleLeftPanel,
                WorkspaceAction::ToggleRightPanel,
                WorkspaceAction::OpenProjectExplorer,
                WorkspaceAction::OpenGlobalSearch,
            ] {
                workspace.handle_action(&action, ctx);
            }
            assert_eq!(workspace.tabs.len(), tabs);
            let panes = workspace.active_tab_pane_group().as_ref(ctx);
            assert!(!panes.left_panel_open);
            assert!(!panes.right_panel_open);
            assert!(Workspace::compute_left_panel_views(ctx).is_empty());
            assert!(
                !crate::workspace::header_toolbar_item::HeaderToolbarItemKind::CodeReview
                    .is_supported(ctx)
            );
        });
    });
}
