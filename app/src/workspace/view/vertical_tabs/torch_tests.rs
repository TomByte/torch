use warp_core::ui::color::contrast::{MinimumAllowedContrast, high_enough_contrast};
use warp_core::ui::theme::{Details, Fill};
use warpui::TypedActionView;

use super::*;

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

#[test]
fn new_session_has_its_project_before_the_shell_reports_a_directory() {
    use crate::workspace::view::tests::{initialize_app, mock_workspace};
    warpui::App::test((), |mut app| async move {
        initialize_app(&mut app);
        let temporary = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temporary.path()).unwrap();
        let child = root.join("nested");
        std::fs::create_dir(&child).unwrap();
        app.add_singleton_model(|_| crate::settings::ProjectsSettings {
            directories: crate::settings::ProjectDirectories::new(None),
            parent_directories: crate::settings::ProjectParentDirectories::new(None),
        });
        app.add_singleton_model(|_| {
            let mut catalog = crate::settings::ProjectCatalog::default();
            catalog.projects.push(crate::settings::LocalProject {
                path: root.clone(),
                name: "Launch".into(),
            });
            catalog
        });
        let workspace = mock_workspace(&mut app);
        workspace.update(&mut app, |workspace, ctx| {
            workspace
                .vertical_tabs_panel
                .collapsed_torch_groups
                .insert(format!("project:{}", root.display()));
            workspace.add_tab_with_pane_layout(
                crate::pane_group::PanesLayout::SingleTerminal(Box::new(
                    crate::pane_group::NewTerminalOptions {
                        initial_directory: Some(child.clone()),
                        ..Default::default()
                    },
                )),
                Arc::new(HashMap::new()),
                None,
                ctx,
            );
            let index = workspace.active_tab_index;
            let session = &sessions(workspace, ctx)[index];
            assert_eq!(session.group_key, format!("project:{}", root.display()));
            assert_eq!(session.group_label, "Launch");
            assert!(!session.directory.is_empty());
            assert!(session_order(workspace, ctx).contains(&index));
            let inherited = workspace.get_new_tab_startup_directory(
                crate::terminal::session_settings::NewSessionSource::Tab,
                Some(ctx.window_id()),
                None,
                ctx,
            );
            assert_eq!(inherited, Some(child));
        });
    });
}

fn frames(motion: Motion) -> Vec<motion::MotionFrame> {
    let total = motion.duration().as_millis() as u64;
    (0..=total)
        .map(|ms| motion.frame(Duration::from_millis(ms)))
        .collect()
}

#[test]
fn arrival_only_slides_in_from_the_left_and_settles_exactly() {
    let first = Motion::Arrival.frame(Duration::ZERO);
    assert_eq!(first.opacity, 0.);
    assert!(first.offset < 0. && first.blur > 0.);
    assert_eq!(
        Motion::Arrival.frame(Motion::Arrival.duration()),
        motion::MotionFrame::SETTLED
    );
    let frames = frames(Motion::Arrival);
    assert!(frames.iter().all(|frame| frame.height == 1.));
    assert!(frames.windows(2).all(|pair| {
        pair[0].opacity <= pair[1].opacity
            && pair[0].offset <= pair[1].offset
            && pair[0].blur >= pair[1].blur
    }));
    assert!(frames.iter().all(|frame| {
        (0. ..=1.).contains(&frame.opacity)
            && (0. ..=1.).contains(&frame.blur)
            && frame.offset <= 0.
    }));
}

#[test]
fn departure_slides_out_before_its_empty_space_closes() {
    assert_eq!(
        Motion::Departure.frame(Duration::ZERO),
        motion::MotionFrame::SETTLED
    );
    let frames = frames(Motion::Departure);
    assert!(frames.windows(2).all(|pair| {
        pair[0].height >= pair[1].height
            && pair[0].opacity >= pair[1].opacity
            && pair[0].offset >= pair[1].offset
    }));
    assert!(
        frames
            .iter()
            .filter(|frame| frame.opacity > 0.)
            .all(|frame| frame.height == 1.)
    );
    let last = frames.last().unwrap();
    assert_eq!((last.height, last.opacity), (0., 0.));
}

#[test]
fn reduced_arrival_fades_without_moving() {
    for frame in frames(Motion::ReducedArrival) {
        assert_eq!((frame.height, frame.offset, frame.blur), (1., 0., 0.));
    }
    assert_eq!(Motion::ReducedArrival.frame(Duration::ZERO).opacity, 0.);
}

#[test]
fn departing_rows_return_to_their_original_order() {
    let live = |ids: &[usize]| ids.iter().map(|id| (*id, *id)).collect::<Vec<_>>();
    // Bulk closes remove from the bottom up, so every departure anchors to the same survivor.
    assert_eq!(
        place_departures(
            live(&[0, 4]),
            [(3, Some(4), 3), (2, Some(4), 2), (1, Some(4), 1)].into_iter()
        ),
        [0, 1, 2, 3, 4]
    );
    // Closing top-down anchors each departure to the next one.
    assert_eq!(
        place_departures(
            live(&[0, 4]),
            [(1, Some(2), 1), (2, Some(3), 2), (3, Some(4), 3)].into_iter()
        ),
        [0, 1, 2, 3, 4]
    );
    assert_eq!(
        place_departures(live(&[0]), [(2, None, 2), (1, None, 1)].into_iter()),
        [0, 1, 2]
    );
    assert_eq!(
        place_departures(live(&[0, 1]), [(5, Some(9), 5)].into_iter()),
        [0, 1, 5]
    );
}

struct MotionLayoutTestView {
    motion: Motion,
    started: Instant,
}
impl warpui::Entity for MotionLayoutTestView {
    type Event = ();
}
impl warpui::TypedActionView for MotionLayoutTestView {
    type Action = ();
}
impl warpui::View for MotionLayoutTestView {
    fn ui_name() -> &'static str {
        "MotionLayoutTest"
    }
    fn render(&self, _: &AppContext) -> Box<dyn Element> {
        let full_row = SavePosition::new(
            ConstrainedBox::new(Empty::new().finish())
                .with_width(120.)
                .with_height(ROW_HEIGHT)
                .finish(),
            "full-row",
        )
        .finish();
        let rows = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_child(
                SavePosition::new(
                    SessionMotion::new(full_row, self.motion, self.started).finish(),
                    "moving",
                )
                .finish(),
            )
            .with_child(
                SavePosition::new(
                    ConstrainedBox::new(Empty::new().finish())
                        .with_width(120.)
                        .with_height(ROW_HEIGHT)
                        .finish(),
                    "following-row",
                )
                .finish(),
            )
            .finish();
        Stack::new()
            .with_child(Align::new(rows).top_left().finish())
            .finish()
    }
}

/// Lays out a [`SessionMotion`] row `elapsed` into `motion` and returns the height of its painted
/// contents, the height of its slot, and where the next row starts.
fn motion_layout(motion: Motion, elapsed: Duration) -> (Option<f32>, f32, f32) {
    let result = std::rc::Rc::new(std::cell::Cell::new((None, 0., 0.)));
    let output = result.clone();
    warpui::App::test((), |mut app| async move {
        let started = Instant::now() - elapsed;
        let (window_id, _view) = app
            .add_window(warpui::platform::WindowStyle::NotStealFocus, |_| {
                MotionLayoutTestView { motion, started }
            });
        let root = app.root_view_id(window_id).unwrap();
        let mut presenter = warpui::Presenter::new(window_id);
        app.update(|ctx| {
            presenter.invalidate(
                warpui::WindowInvalidation {
                    updated: [root].into_iter().collect(),
                    ..Default::default()
                },
                ctx,
            );
            presenter.build_scene(vec2f(272., 600.), 1., None, ctx);
            let positions = presenter.position_cache();
            output.set((
                positions.get_position("full-row").map(|row| row.height()),
                positions.get_position("moving").unwrap().height(),
                positions
                    .get_position("following-row")
                    .unwrap()
                    .origin()
                    .y(),
            ));
        });
    });
    result.get()
}

#[test]
fn moving_rows_keep_their_height_until_a_departure_has_faded() {
    let settled = motion_layout(Motion::Arrival, Duration::from_secs(5));
    assert_eq!(settled, (Some(ROW_HEIGHT), ROW_HEIGHT, ROW_HEIGHT));

    let arriving = motion_layout(Motion::Arrival, Duration::from_millis(100));
    assert_eq!(arriving, (Some(ROW_HEIGHT), ROW_HEIGHT, ROW_HEIGHT));

    let sliding_out = motion_layout(Motion::Departure, Duration::from_millis(100));
    assert_eq!(sliding_out, (Some(ROW_HEIGHT), ROW_HEIGHT, ROW_HEIGHT));

    let (painted, slot, following) = motion_layout(Motion::Departure, Duration::from_millis(250));
    assert_eq!(painted, None);
    assert!(slot > 0. && slot < ROW_HEIGHT);
    assert_eq!(following, slot);

    let gone = motion_layout(Motion::Departure, Duration::from_secs(5));
    assert_eq!(gone, (None, 0., 0.));
}

#[test]
fn attention_jump_prefers_blocked_sessions_and_wraps_in_sidebar_order() {
    // Sidebar order differs from tab order; 4 and 1 are blocked, 2 finished unread.
    let order = [3, 4, 0, 2, 1];
    let rank = |index| match index {
        4 | 1 => Some(0),
        2 => Some(1),
        _ => None,
    };
    assert_eq!(next_in_attention_order(&order, rank, 3), Some(4));
    assert_eq!(next_in_attention_order(&order, rank, 4), Some(1));
    assert_eq!(next_in_attention_order(&order, rank, 1), Some(4));
}

#[test]
fn attention_jump_falls_back_to_unread_and_skips_the_active_session() {
    let order = [0, 1, 2];
    let only_active_blocked = |index| match index {
        1 => Some(0),
        2 => Some(1),
        _ => None,
    };
    assert_eq!(
        next_in_attention_order(&order, only_active_blocked, 1),
        Some(2)
    );
    assert_eq!(next_in_attention_order(&order, |_| None, 0), None);
    assert_eq!(
        next_in_attention_order(&order, |index| (index == 0).then_some(0), 0),
        None
    );
}

fn pane(status: Option<ConversationStatus>, unread: bool) -> PaneStatus {
    PaneStatus {
        status,
        unread,
        agent_status_unavailable: false,
    }
}

#[test]
fn a_tab_surfaces_its_most_urgent_pane() {
    let blocked = ConversationStatus::Blocked {
        blocked_action: "Permission required".into(),
    };
    let surfaced = |panes: Vec<PaneStatus>| most_urgent(panes).and_then(|pane| pane.status);
    assert_eq!(
        surfaced(vec![
            pane(Some(ConversationStatus::InProgress), false),
            pane(Some(blocked.clone()), false),
        ]),
        Some(blocked),
    );
    assert_eq!(
        surfaced(vec![
            pane(Some(ConversationStatus::Success), true),
            pane(Some(ConversationStatus::InProgress), false),
        ]),
        Some(ConversationStatus::InProgress),
    );
    assert_eq!(
        surfaced(vec![
            pane(Some(ConversationStatus::Error), false),
            pane(Some(ConversationStatus::Success), true),
        ]),
        Some(ConversationStatus::Success),
    );
    assert_eq!(
        surfaced(vec![
            pane(Some(ConversationStatus::Success), false),
            pane(Some(ConversationStatus::Error), false),
        ]),
        Some(ConversationStatus::Error),
    );
    assert_eq!(
        surfaced(vec![
            pane(None, false),
            pane(Some(ConversationStatus::Success), false),
        ]),
        None,
        "equally quiet panes keep the focused pane's status",
    );
    assert!(surfaced(Vec::new()).is_none());
}

#[test]
fn keyboard_navigation_follows_sidebar_rows_and_skips_collapsed_groups() {
    // Tabs 0 and 2 share a group shown first; tab 3 is in a collapsed group.
    let rows = [(0, true), (2, true), (1, true), (3, false)];
    assert_eq!(expanded_row_from(&rows, 0, 1), Some(2));
    assert_eq!(expanded_row_from(&rows, 2, 1), Some(1));
    assert_eq!(
        expanded_row_from(&rows, 1, 1),
        Some(0),
        "wraps past collapsed rows"
    );
    assert_eq!(expanded_row_from(&rows, 0, -1), Some(1));
    assert_eq!(
        expanded_row_from(&rows, 3, 1),
        Some(0),
        "leaves a collapsed active row"
    );
    assert_eq!(expanded_row_from(&[(4, true)], 4, 1), Some(4));
    assert_eq!(expanded_row_from(&rows, 9, 1), None);
}

#[test]
fn moving_a_session_keeps_it_in_its_group_and_groups_in_order() {
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
            while workspace.tabs.len() < 3 {
                workspace.add_terminal_tab(false, ctx);
            }
            let ids = workspace
                .tabs
                .iter()
                .map(|tab| tab.pane_group.id())
                .collect::<Vec<_>>();
            let order = session_order(workspace, ctx);
            assert_eq!(order, vec![0, 1, 2], "mock sessions share one group");

            workspace.move_sidebar_session(0, TabMovement::Right, ctx);
            assert_eq!(workspace.tabs[1].pane_group.id(), ids[0]);
            assert_eq!(workspace.tabs[0].pane_group.id(), ids[1]);

            workspace.move_sidebar_session(1, TabMovement::Left, ctx);
            assert_eq!(workspace.tabs[0].pane_group.id(), ids[0]);

            workspace.move_sidebar_session(0, TabMovement::Left, ctx);
            assert_eq!(
                workspace.tabs[0].pane_group.id(),
                ids[0],
                "first row stays put"
            );
        });
    });
}

#[test]
fn changing_directory_keeps_a_session_in_its_launch_group() {
    let no_projects = |_: &Path| false;
    let launched = next_anchor(None, None, "~/code/scratch", no_projects);
    assert_eq!(launched, Some(GroupAnchor::Folder("~/code/scratch".into())));
    assert_eq!(
        next_anchor(launched.as_ref(), None, "~/elsewhere", no_projects),
        launched,
    );
    assert_eq!(next_anchor(None, None, "", no_projects), None);
}

#[test]
fn sessions_join_their_first_project_and_stay_there() {
    let alpha = PathBuf::from("/projects/alpha");
    let beta = PathBuf::from("/projects/beta");
    let known = |path: &Path| path == alpha || path == beta;
    let home = Some(GroupAnchor::Folder("~".into()));
    let joined = next_anchor(home.as_ref(), Some(&alpha), "~/projects/alpha", known);
    assert_eq!(joined, Some(GroupAnchor::Project(alpha.clone())));
    assert_eq!(
        next_anchor(joined.as_ref(), Some(&beta), "~/projects/beta", known),
        joined,
    );
    assert_eq!(
        next_anchor(joined.as_ref(), None, "/tmp", |path| path == beta),
        Some(GroupAnchor::Folder("/tmp".into())),
        "a removed project releases its sessions",
    );
}
