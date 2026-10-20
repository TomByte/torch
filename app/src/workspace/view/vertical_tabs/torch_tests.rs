use super::*;
use warp_core::ui::color::contrast::{MinimumAllowedContrast, high_enough_contrast};
use warp_core::ui::theme::{Details, Fill};
use warpui::TypedActionView;

#[test]
fn completed_sessions_only_have_a_filled_check_until_read() {
    assert_eq!(
        session_indicator(Some(&ConversationStatus::Success), true, false),
        SessionIndicator::UnreadComplete,
    );
    assert_eq!(
        session_indicator(Some(&ConversationStatus::Success), false, false),
        SessionIndicator::Complete,
    );
}

#[test]
fn idle_and_untracked_sessions_do_not_claim_completed_work() {
    assert_eq!(
        session_indicator(None, false, true),
        SessionIndicator::Unknown
    );
    assert_eq!(
        session_indicator(None, false, false),
        SessionIndicator::Idle
    );
    assert_eq!(
        session_indicator(Some(&ConversationStatus::Cancelled), true, false),
        SessionIndicator::Idle,
    );
}

#[test]
fn permission_requests_keep_attention_even_after_being_read() {
    let status = ConversationStatus::Blocked {
        blocked_action: "Permission required".into(),
    };
    assert_eq!(
        session_indicator(Some(&status), false, false),
        SessionIndicator::NeedsInput
    );
    assert_eq!(
        session_indicator(Some(&status), true, false),
        SessionIndicator::NeedsInput
    );
}

#[test]
fn unavailable_project_does_not_create_a_session_or_expand_its_group() {
    use crate::workspace::view::tests::{initialize_app, mock_workspace};
    warpui::App::test((), |mut app| async move {
        initialize_app(&mut app);
        app.add_singleton_model(|_| crate::settings::ProjectsSettings {
            directories: crate::settings::ProjectDirectories::new(None),
            parent_directories: crate::settings::ProjectParentDirectories::new(None),
        });
        app.add_singleton_model(|_| crate::settings::ProjectCatalog::default());
        let workspace = mock_workspace(&mut app);
        let temporary = tempfile::tempdir().unwrap();
        let missing = temporary.path().join("removed project");
        let key = format!("project:{}", missing.display());
        workspace.update(&mut app, |workspace, ctx| {
            let original_tabs = workspace.tabs.len();
            let original_active = workspace.active_tab_index;
            workspace
                .vertical_tabs_panel
                .collapsed_torch_groups
                .insert(key.clone());

            workspace.handle_action(&WorkspaceAction::NewSessionInDirectory(missing), ctx);

            assert_eq!(workspace.tabs.len(), original_tabs);
            assert_eq!(workspace.active_tab_index, original_active);
            assert!(
                workspace
                    .vertical_tabs_panel
                    .collapsed_torch_groups
                    .contains(&key)
            );
        });
    });
}

#[test]
fn collapsing_sidebar_group_keeps_sessions_alive_and_activation_reveals_them() {
    use crate::workspace::view::tests::{initialize_app, mock_workspace};
    warpui::App::test((), |mut app| async move {
        initialize_app(&mut app);
        app.add_singleton_model(|_| crate::settings::ProjectsSettings {
            directories: crate::settings::ProjectDirectories::new(None),
            parent_directories: crate::settings::ProjectParentDirectories::new(None),
        });
        app.add_singleton_model(|_| crate::settings::ProjectCatalog::default());
        let workspace = mock_workspace(&mut app);
        workspace.update(&mut app, |workspace, ctx| {
            let key = sessions(workspace, ctx)[0].group_key.clone();
            let pane_group_id = workspace.tabs[0].pane_group.id();
            let tab_count = workspace.tabs.len();
            let original_order = session_order(workspace, ctx);

            workspace.handle_action(
                &WorkspaceAction::ToggleSidebarGroupCollapsed(key.clone()),
                ctx,
            );
            assert!(session_order(workspace, ctx).is_empty());
            assert_eq!(workspace.tabs.len(), tab_count);
            assert_eq!(workspace.tabs[0].pane_group.id(), pane_group_id);

            workspace.handle_action(
                &WorkspaceAction::ToggleSidebarGroupCollapsed(key.clone()),
                ctx,
            );
            assert_eq!(session_order(workspace, ctx), original_order);

            workspace.handle_action(&WorkspaceAction::ToggleSidebarGroupCollapsed(key), ctx);
            workspace.activate_tab(0, ctx);
            assert_eq!(session_order(workspace, ctx), original_order);
            assert_eq!(workspace.tabs[0].pane_group.id(), pane_group_id);
        });
    });
}

#[test]
fn dark_sidebar_text_remains_readable_in_selected_and_idle_rows() {
    let theme = crate::themes::default_themes::dark_theme();
    let palette = Palette::new(&theme);
    for background in [
        palette.sidebar_top,
        palette.sidebar_bottom,
        palette.selection,
    ] {
        for foreground in [
            palette.text,
            palette.secondary,
            palette.muted,
            palette.attention,
        ] {
            assert!(high_enough_contrast(
                foreground,
                background,
                MinimumAllowedContrast::Text,
            ));
        }
    }
    assert!(high_enough_contrast(
        theme.foreground().into_solid(),
        theme.background().into_solid(),
        MinimumAllowedContrast::Text,
    ));
}

#[test]
fn sidebar_follows_custom_theme_color_scheme_instead_of_red_channel() {
    // A bright cyan theme has a low red channel but still needs the light shell.
    let theme = WarpTheme::new(
        Fill::Solid(ColorU::from_u32(0x00FFFFFF)),
        ColorU::black(),
        Fill::Solid(ColorU::from_u32(0x007799FF)),
        None,
        Some(Details::Lighter),
        crate::themes::default_themes::light_theme()
            .terminal_colors()
            .clone(),
        None,
        None,
    );
    let palette = Palette::new(&theme);
    assert!(high_enough_contrast(
        ColorU::black(),
        palette.sidebar_top,
        MinimumAllowedContrast::Text,
    ));
}

#[test]
fn ranges_follow_folder_order_instead_of_underlying_tab_indices() {
    let keys = [
        "folder:A",
        "folder:B",
        "folder:A",
        "manual:review",
        "folder:B",
    ]
    .map(str::to_owned);
    let groups = grouped_indices(&keys);
    assert_eq!(groups, vec![vec![0, 2], vec![1, 4], vec![3]]);
    let order = groups.into_iter().flatten().collect::<Vec<_>>();
    assert_eq!(range_in_order(&order, 0, 2), vec![0, 2]);
    assert_eq!(range_in_order(&order, 3, 1), vec![1, 4, 3]);
    assert!(range_in_order(&order, 99, 1).is_empty());
}

#[test]
fn folder_groups_handle_home_roots_and_both_path_separators() {
    assert_eq!(folder_label("~/code/launchpad/"), "launchpad");
    assert_eq!(folder_label(r"C:\code\backend\"), "backend");
    assert_eq!(folder_label("~"), "Other");
    assert_eq!(folder_label("/"), "Other");
    assert_eq!(folder_label(""), "Other");
}

#[test]
fn grouping_merges_home_and_nonterminal_sessions_but_not_equal_folder_names() {
    assert_eq!(folder_key("~"), folder_key(""));
    assert_eq!(folder_key("~/code/torch/"), folder_key("~/code/torch"));
    assert_ne!(folder_key("~/work/torch"), folder_key("~/personal/torch"));
}

#[test]
fn activity_labels_remain_compact_across_time_boundaries() {
    for (minutes, expected) in [
        (-1, "now"),
        (0, "now"),
        (1, "1m"),
        (59, "59m"),
        (60, "1h"),
        (1439, "23h"),
        (1440, "1d"),
    ] {
        assert_eq!(age_label(minutes), expected);
    }
}

#[test]
fn quiet_command_surfaces_preserve_text_contrast_in_both_themes() {
    use warp_core::ui::color::blend::Blend;
    for theme in [
        crate::themes::default_themes::light_theme(),
        crate::themes::default_themes::dark_theme(),
    ] {
        for surface in [
            crate::themes::command_surface(&theme),
            theme
                .background()
                .blend(&crate::themes::restored_command_overlay(&theme)),
        ] {
            assert!(high_enough_contrast(
                theme.foreground().into_solid(),
                surface.into_solid(),
                MinimumAllowedContrast::Text
            ));
        }
    }
}

#[test]
fn kanagawa_and_ayu_keep_sidebar_and_command_text_readable() {
    for theme in [
        crate::themes::default_themes::kanagawa(),
        crate::themes::default_themes::ayu(),
    ] {
        let palette = Palette::new(&theme);
        for background in [
            palette.sidebar_top,
            palette.sidebar_bottom,
            palette.selection,
        ] {
            for foreground in [
                palette.text,
                palette.secondary,
                palette.muted,
                palette.attention,
            ] {
                assert!(
                    high_enough_contrast(foreground, background, MinimumAllowedContrast::Text),
                    "{:?}: {foreground:?} against {background:?}",
                    theme.name()
                );
            }
        }
        for background in [theme.background(), crate::themes::command_surface(&theme)] {
            assert!(high_enough_contrast(
                theme.foreground().into_solid(),
                background.into_solid(),
                MinimumAllowedContrast::Text
            ));
        }
    }
}
