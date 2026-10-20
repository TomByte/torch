use super::*;
use crate::session_management::CommandContext;
use crate::workspace::header_toolbar_item::HeaderToolbarItemKind;
use crate::workspace::view::PanelPosition;
use warp_core::ui::theme::ColorScheme;

pub(super) const SIDEBAR_WIDTH: f32 = 272.;
const HEADER_HEIGHT: f32 = 48.;
const ROW_HEIGHT: f32 = 31.;

struct Palette {
    sidebar_top: ColorU,
    sidebar_bottom: ColorU,
    text: ColorU,
    secondary: ColorU,
    muted: ColorU,
    selection: ColorU,
    hover: ColorU,
    rule: ColorU,
    attention: ColorU,
    header_rule: ColorU,
    shadow: ColorU,
}

impl Palette {
    fn new(theme: &WarpTheme) -> Self {
        if theme.inferred_color_scheme() == ColorScheme::DarkOnLight {
            Self {
                sidebar_top: ColorU::from_u32(0xE6ECF4FF),
                sidebar_bottom: ColorU::from_u32(0xEEE9F4FF),
                text: ColorU::from_u32(0x202124FF),
                secondary: ColorU::from_u32(0x616268FF),
                muted: ColorU::from_u32(0x85868BFF),
                selection: ColorU::from_u32(0xFFFFFFF5),
                hover: ColorU::from_u32(0xFFFFFF70),
                rule: ColorU::from_u32(0xD5D6DBFF),
                attention: ColorU::from_u32(0x98651FFF),
                header_rule: ColorU::from_u32(0x88888818),
                shadow: ColorU::from_u32(0x22223312),
            }
        } else {
            Self {
                sidebar_top: ColorU::from_u32(0x2B2B2BFF),
                sidebar_bottom: ColorU::from_u32(0x282828FF),
                text: ColorU::from_u32(0xE6E6E6FF),
                secondary: ColorU::from_u32(0xB5B5B5FF),
                muted: ColorU::from_u32(0xA8A8A8FF),
                selection: ColorU::from_u32(0x3B3B3BFF),
                hover: ColorU::from_u32(0xFFFFFF12),
                rule: ColorU::from_u32(0x424242FF),
                attention: ColorU::from_u32(0xD9B96EFF),
                header_rule: ColorU::from_u32(0xFFFFFF12),
                shadow: ColorU::from_u32(0x00000030),
            }
        }
    }

    fn background(&self) -> ElementFill {
        ElementFill::Gradient {
            start: vec2f(0., 0.),
            end: vec2f(0., 1.),
            start_color: self.sidebar_top,
            end_color: self.sidebar_bottom,
        }
    }
}

struct Session {
    index: usize,
    title: String,
    directory: String,
    group_key: String,
    group_label: String,
    status: Option<ConversationStatus>,
    age: String,
    unread: bool,
}

fn folder_label(directory: &str) -> String {
    let trimmed = directory.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() || trimmed == "~" {
        return "Other".to_owned();
    }
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(trimmed)
        .to_owned()
}

fn folder_key(directory: &str) -> String {
    match directory.trim_end_matches(['/', '\\']) {
        "" | "~" => "other".to_owned(),
        path => format!("folder:{path}"),
    }
}

fn age_label(minutes: i64) -> String {
    match minutes {
        ..=0 => "now".to_owned(),
        1..60 => format!("{minutes}m"),
        60..1440 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1440),
    }
}

fn grouped_indices(keys: &[String]) -> Vec<Vec<usize>> {
    let mut groups: Vec<(&str, Vec<usize>)> = Vec::new();
    for (index, key) in keys.iter().enumerate() {
        if let Some((_, members)) = groups.iter_mut().find(|(existing, _)| *existing == key) {
            members.push(index);
        } else {
            groups.push((key, vec![index]));
        }
    }
    groups.into_iter().map(|(_, members)| members).collect()
}

pub(in crate::workspace::view) fn session_order(
    workspace: &Workspace,
    app: &AppContext,
) -> Vec<usize> {
    let keys = sessions(workspace, app)
        .into_iter()
        .map(|session| session.group_key)
        .collect::<Vec<_>>();
    grouped_indices(&keys).into_iter().flatten().collect()
}

pub(in crate::workspace::view) fn range_in_order(
    order: &[usize],
    anchor: usize,
    clicked: usize,
) -> Vec<usize> {
    let Some(start) = order.iter().position(|index| *index == anchor) else {
        return Vec::new();
    };
    let Some(end) = order.iter().position(|index| *index == clicked) else {
        return Vec::new();
    };
    order[start.min(end)..=start.max(end)].to_vec()
}

pub(in crate::workspace::view) fn drag_neighbors(
    workspace: &Workspace,
    index: usize,
    app: &AppContext,
) -> (Option<usize>, Option<usize>) {
    let keys = sessions(workspace, app)
        .into_iter()
        .map(|session| session.group_key)
        .collect::<Vec<_>>();
    let groups = grouped_indices(&keys);
    let Some(members) = groups.iter().find(|members| members.contains(&index)) else {
        return (None, None);
    };
    let position = members.iter().position(|member| *member == index).unwrap();
    (
        position.checked_sub(1).map(|i| members[i]),
        members.get(position + 1).copied(),
    )
}

fn sessions(workspace: &Workspace, app: &AppContext) -> Vec<Session> {
    workspace
        .tabs
        .iter()
        .enumerate()
        .map(|(index, tab)| {
            let pane_group = tab.pane_group.as_ref(app);
            let terminal = pane_group.focused_session_view(app);
            let terminal = terminal.as_ref().map(|view| view.as_ref(app));
            let directory = terminal
                .and_then(|view| view.display_working_directory(app))
                .unwrap_or_default();
            let cli_session =
                terminal.and_then(|view| CLIAgentSessionsModel::as_ref(app).session(view.id()));
            let title = pane_group
                .custom_title(app)
                .or_else(|| cli_session.and_then(|session| session.session_context.display_title()))
                .unwrap_or_else(|| {
                    let title = pane_group.display_title(app);
                    if title.is_empty() || title == directory {
                        "Terminal".to_owned()
                    } else {
                        title
                    }
                });
            let mut status =
                terminal.and_then(|view| summary_conversation_status_for_terminal(view, app));
            if status.is_none()
                && terminal.is_some_and(|view| {
                    matches!(
                        view.session_command_context(app),
                        CommandContext::RunningCommand { .. }
                    )
                })
            {
                status = Some(ConversationStatus::InProgress);
            }
            let age = terminal
                .and_then(TerminalView::session_activity_age_minutes)
                .map(age_label)
                .unwrap_or_default();
            let (group_key, group_label) = tab
                .group_id
                .and_then(|id| {
                    workspace
                        .tab_groups
                        .get(&id)
                        .map(|group| (format!("manual:{id:?}"), group_display_name(group)))
                })
                .unwrap_or_else(|| {
                    terminal
                        .and_then(|view| view.canonical_session_pwd_if_local(app))
                        .and_then(|path| {
                            crate::settings::project_for_directory(
                                &crate::settings::ProjectCatalog::as_ref(app).projects,
                                path.as_path(),
                            )
                        })
                        .map(|project| {
                            (
                                format!("project:{}", project.path.display()),
                                project.name.clone(),
                            )
                        })
                        .unwrap_or_else(|| (folder_key(&directory), folder_label(&directory)))
                });
            let unread =
                terminal.is_some_and(|view| has_unread_activity_for_terminal_view(view.id(), app));
            Session {
                index,
                title,
                directory,
                group_key,
                group_label,
                status,
                age,
                unread,
            }
        })
        .collect()
}

fn label(
    text: impl Into<String>,
    size: f32,
    color: ColorU,
    weight: Weight,
    appearance: &Appearance,
) -> Box<dyn Element> {
    Text::new_inline(text.into(), appearance.ui_font_family(), size)
        .with_color(color)
        .with_style(Properties::default().weight(weight))
        .with_clip(ClipConfig::ellipsis())
        .finish()
}

fn icon(icon: WarpIcon, size: f32, color: ColorU) -> Box<dyn Element> {
    ConstrainedBox::new(icon.to_warpui_icon(color.into()).finish())
        .with_width(size)
        .with_height(size)
        .finish()
}

fn control(
    icon_kind: WarpIcon,
    state: MouseStateHandle,
    action: WorkspaceAction,
    palette: &Palette,
) -> Box<dyn Element> {
    let color = palette.secondary;
    let hover = palette.hover;
    Hoverable::new(state, move |state| {
        ConstrainedBox::new(
            Container::new(icon(icon_kind, 16., color))
                .with_uniform_padding(6.)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
                .with_background(if state.is_hovered() {
                    ElementFill::Solid(hover)
                } else {
                    ElementFill::None
                })
                .finish(),
        )
        .with_width(28.)
        .with_height(28.)
        .finish()
    })
    .with_cursor(Cursor::PointingHand)
    .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
    .finish()
}

fn status_icon(status: Option<&ConversationStatus>, palette: &Palette) -> Box<dyn Element> {
    if matches!(status, Some(ConversationStatus::InProgress)) {
        return ConstrainedBox::new(
            warpui::elements::Icon::new("bundled/svg/torch-session-running.svg", palette.text)
                .finish(),
        )
        .with_width(14.)
        .with_height(14.)
        .finish();
    }
    if matches!(status, Some(ConversationStatus::Blocked { .. })) {
        return Stack::new()
            .with_child(icon(WarpIcon::Circle, 14., palette.attention))
            .with_child(Align::new(icon(WarpIcon::CircleFilled, 5., palette.attention)).finish())
            .finish();
    }
    if matches!(status, Some(ConversationStatus::Success)) {
        return Stack::new()
            .with_child(icon(WarpIcon::CircleFilled, 14., palette.text))
            .with_child(Align::new(icon(WarpIcon::Check, 9., palette.selection)).finish())
            .finish();
    }
    let (glyph, color) = match status {
        Some(ConversationStatus::InProgress) => (WarpIcon::Loading, palette.text),
        Some(ConversationStatus::Blocked { .. }) => (WarpIcon::Circle, palette.attention),
        Some(ConversationStatus::Success) => (WarpIcon::CheckCircleBroken, palette.text),
        Some(ConversationStatus::Error | ConversationStatus::TransientError) => {
            (WarpIcon::AlertCircle, palette.attention)
        }
        Some(ConversationStatus::Cancelled | ConversationStatus::WaitingForEvents) | None => {
            (WarpIcon::Check, palette.muted)
        }
    };
    icon(glyph, 14., color)
}

fn row(
    session: &Session,
    workspace: &Workspace,
    palette: &Palette,
    app: &AppContext,
) -> Box<dyn Element> {
    let appearance = Appearance::as_ref(app);
    let index = session.index;
    let active = index == workspace.active_tab_index;
    let selected = palette.selection;
    let hover = palette.hover;
    let rule = palette.rule;
    let pane_group = workspace.tabs[index].pane_group.as_ref(app);
    let locator = PaneViewLocator {
        pane_group_id: workspace.tabs[index].pane_group.id(),
        pane_id: pane_group.focused_pane_id(app),
    };
    let in_multi_selection = workspace.tabs[index].in_multi_selection;
    let title = if workspace.current_workspace_state.tab_being_renamed() == Some(index) {
        render_inline_tab_rename_editor(&workspace.tab_rename_editor, appearance, app)
    } else {
        label(
            &session.title,
            14.,
            palette.text,
            if session.unread {
                Weight::Semibold
            } else {
                Weight::Normal
            },
            appearance,
        )
    };
    let contents = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(10.)
        .with_child(Expanded::new(1., title).finish())
        .with_child(label(
            &session.age,
            12.,
            palette.secondary,
            Weight::Normal,
            appearance,
        ))
        .with_child(if session.status.is_none() && session.age.is_empty() {
            ConstrainedBox::new(Empty::new().finish())
                .with_width(14.)
                .finish()
        } else {
            status_icon(session.status.as_ref(), palette)
        })
        .finish();
    let row = Hoverable::new(
        workspace.tabs[index].tab_mouse_state.clone(),
        move |state| {
            ConstrainedBox::new(
                Container::new(contents)
                    .with_padding(Padding::uniform(4.).with_horizontal(12.))
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(9.)))
                    .with_background(if active || in_multi_selection {
                        ElementFill::Solid(selected)
                    } else if state.is_hovered() {
                        ElementFill::Solid(hover)
                    } else {
                        ElementFill::None
                    })
                    .with_border(Border::all(1.).with_border_fill(if active {
                        ElementFill::Solid(rule)
                    } else {
                        ElementFill::None
                    }))
                    .finish(),
            )
            .with_height(ROW_HEIGHT)
            .finish()
        },
    )
    .on_click_with_modifiers(move |ctx, _, _, modifiers| {
        if modifiers.shift {
            ctx.dispatch_typed_action(WorkspaceAction::ShiftSelectTabRange { locator });
        } else if modifiers.cmd {
            ctx.dispatch_typed_action(WorkspaceAction::ToggleTabMultiSelection { locator });
        } else {
            ctx.dispatch_typed_action(WorkspaceAction::ActivateTab(index));
        }
    })
    .on_double_click(move |ctx, _, _| ctx.dispatch_typed_action(WorkspaceAction::RenameTab(index)))
    .on_middle_click(move |ctx, _, _| ctx.dispatch_typed_action(WorkspaceAction::CloseTab(index)))
    .on_right_click(move |ctx, _, position| {
        let anchor = TabContextMenuAnchor::Pointer(position);
        ctx.dispatch_typed_action(if in_multi_selection {
            WorkspaceAction::ToggleTabSelectionRightClickMenu {
                tab_index: index,
                anchor,
            }
        } else {
            WorkspaceAction::ToggleTabRightClickMenu {
                tab_index: index,
                anchor,
            }
        });
    })
    .with_cursor(Cursor::PointingHand)
    .finish();
    let row = Draggable::new(workspace.tabs[index].draggable_state.clone(), row)
        .with_drag_axis(DragAxis::VerticalOnly)
        .on_drag_start(|ctx, _, _| ctx.dispatch_typed_action(WorkspaceAction::StartTabDrag))
        .on_drag(move |ctx, _, rect, _| {
            ctx.dispatch_typed_action(WorkspaceAction::DragTab {
                tab_index: index,
                tab_position: rect,
            })
        })
        .on_drop(move |ctx, _, rect, _| {
            ctx.dispatch_typed_action(WorkspaceAction::CommitTabDrag {
                pane_group_id: locator.pane_group_id,
                tab_position: rect,
            });
            ctx.dispatch_typed_action(WorkspaceAction::DropTab);
        })
        .finish();
    SavePosition::new(row, &tab_position_id(index)).finish()
}

fn sidebar(
    workspace: &Workspace,
    sessions: &[Session],
    palette: &Palette,
    app: &AppContext,
) -> Box<dyn Element> {
    let appearance = Appearance::as_ref(app);
    let state = &workspace.vertical_tabs_panel;
    let header = Container::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Expanded::new(1., Empty::new().finish()).finish())
            .with_child(control(
                WarpIcon::LayoutAlt01,
                state.settings_button_mouse_state.clone(),
                WorkspaceAction::ToggleVerticalTabsPanel,
                palette,
            ))
            .with_child(control(
                WarpIcon::Plus,
                state.new_tab_button_state.clone(),
                WorkspaceAction::AddDefaultTab,
                palette,
            ))
            .finish(),
    )
    .with_padding_left(80.)
    .with_padding_right(14.)
    .finish();
    let header = workspace.render_tab_bar_hoverable(
        ConstrainedBox::new(header)
            .with_height(HEADER_HEIGHT + 8.)
            .finish(),
    );
    let keys = sessions
        .iter()
        .map(|session| session.group_key.clone())
        .collect::<Vec<_>>();
    let groups = grouped_indices(&keys);
    let mut list = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    for indices in groups {
        let members = indices
            .into_iter()
            .map(|index| &sessions[index])
            .collect::<Vec<_>>();
        let mut group_content =
            Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        let needs = members
            .iter()
            .filter(|session| matches!(session.status, Some(ConversationStatus::Blocked { .. })))
            .count();
        let mut heading = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.)
            .with_child(label(
                &members[0].group_label,
                12.,
                palette.muted,
                Weight::Semibold,
                appearance,
            ))
            .with_child(label(
                members.len().to_string(),
                12.,
                palette.muted,
                Weight::Normal,
                appearance,
            ))
            .with_child(Expanded::new(1., Empty::new().finish()).finish());
        if needs > 0 {
            heading.add_child(label(
                format!("{needs} needs you"),
                12.,
                palette.attention,
                Weight::Medium,
                appearance,
            ));
        }
        let group = workspace.tabs[members[0].index]
            .group_id
            .and_then(|id| workspace.tab_groups.get(&id));
        let heading = if let Some(group) = group {
            let group_id = group.id;
            let mouse = state
                .tab_group_mouse_states
                .borrow_mut()
                .entry(group_id)
                .or_default()
                .header
                .clone();
            let content = if workspace
                .current_workspace_state
                .is_tab_group_being_renamed(group_id)
            {
                render_inline_tab_rename_editor(&workspace.tab_group_rename_editor, appearance, app)
            } else {
                heading.finish()
            };
            Hoverable::new(mouse, |_| content)
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(WorkspaceAction::ToggleTabGroupCollapsed(group_id))
                })
                .on_double_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(WorkspaceAction::RenameTabGroup(group_id))
                })
                .on_right_click(move |ctx, _, position| {
                    ctx.dispatch_typed_action(WorkspaceAction::ToggleTabGroupRightClickMenu {
                        group_id,
                        anchor: TabContextMenuAnchor::Pointer(position),
                    })
                })
                .finish()
        } else {
            heading.finish()
        };
        group_content.add_child(
            Container::new(heading)
                .with_padding(
                    Padding::uniform(0.)
                        .with_left(12.)
                        .with_right(10.)
                        .with_top(12.)
                        .with_bottom(7.),
                )
                .finish(),
        );
        if !group.is_some_and(|group| group.collapsed) {
            for session in members {
                group_content.add_child(row(session, workspace, palette, app));
            }
        }
        group_content.add_child(
            ConstrainedBox::new(Empty::new().finish())
                .with_height(15.)
                .finish(),
        );
        list.add_child(if let Some(group) = group {
            SavePosition::new(group_content.finish(), &vtab_group_position_id(group.id)).finish()
        } else {
            group_content.finish()
        });
    }
    let scroller = ClippedScrollable::vertical(
        state.scroll_state.clone(),
        list.finish(),
        ScrollbarWidth::Custom(3.),
        ElementFill::None,
        ElementFill::Solid(palette.muted),
        ElementFill::None,
    )
    .with_overlayed_scrollbar()
    .finish();
    let content = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(header)
        .with_child(
            Expanded::new(
                1.,
                Container::new(scroller)
                    .with_padding_left(9.)
                    .with_padding_right(9.)
                    .finish(),
            )
            .finish(),
        )
        .finish();
    let panel = Resizable::new(state.resizable_state.clone(), content)
        .with_dragbar_side(DragBarSide::Right)
        .with_bounds_callback(Box::new(|size| (220., (size.x() * 0.45).max(220.))))
        .on_resize(|ctx, _| ctx.notify())
        .finish();
    SavePosition::new(panel, super::super::VERTICAL_TABS_PANEL_POSITION_ID).finish()
}

pub(in crate::workspace::view) fn render(
    workspace: &Workspace,
    app: &AppContext,
) -> Box<dyn Element> {
    let appearance = Appearance::as_ref(app);
    let palette = Palette::new(appearance.theme());
    let sessions = sessions(workspace, app);
    let active = &sessions[workspace.active_tab_index];
    let mut header = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(12.);
    if !workspace.vertical_tabs_panel_open {
        header.add_child(control(
            WarpIcon::LayoutAlt01,
            workspace
                .vertical_tabs_panel
                .settings_button_mouse_state
                .clone(),
            WorkspaceAction::ToggleVerticalTabsPanel,
            &palette,
        ));
    }
    header.add_child(
        Shrinkable::new(
            1.,
            ConstrainedBox::new(label(
                &active.title,
                14.,
                palette.text,
                Weight::Semibold,
                appearance,
            ))
            .with_max_width(280.)
            .finish(),
        )
        .finish(),
    );
    header.add_child(
        Expanded::new(
            1.,
            label(
                &active.directory,
                14.,
                palette.secondary,
                Weight::Normal,
                appearance,
            ),
        )
        .finish(),
    );
    let header = SavePosition::new(
        ConstrainedBox::new(
            Container::new(header.finish())
                .with_padding_left(if workspace.vertical_tabs_panel_open {
                    32.
                } else {
                    80.
                })
                .with_padding_right(24.)
                .with_border(Border::bottom(1.).with_border_fill(palette.header_rule))
                .finish(),
        )
        .with_height(40.)
        .finish(),
        super::super::TAB_BAR_POSITION_ID,
    )
    .finish();
    let config = &TabSettings::as_ref(app).header_toolbar_chip_selection;
    let pane_group = workspace.active_tab_pane_group().as_ref(app);
    let mut body = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    for item in config
        .left_items()
        .into_iter()
        .filter(|item| *item != HeaderToolbarItemKind::TabsPanel)
    {
        if let Some(panel) = workspace.render_config_panel(&item, pane_group, config, app) {
            body.add_child(panel);
        }
    }
    if workspace.current_workspace_state.is_theme_chooser_open {
        body.add_child(workspace.render_panel(
            app,
            workspace.render_theme_chooser(),
            &PanelPosition::Left,
        ));
    }
    let is_right_maximized = pane_group.right_panel_open && pane_group.is_right_panel_maximized;
    if !is_right_maximized {
        body.add_child(
            Expanded::new(1., workspace.render_banner_and_active_tab(app, appearance)).finish(),
        );
    }
    for item in config
        .right_items()
        .into_iter()
        .filter(|item| *item != HeaderToolbarItemKind::TabsPanel)
    {
        if let Some(panel) = workspace.render_config_panel(&item, pane_group, config, app) {
            body.add_child(panel);
        }
    }
    if is_right_maximized {
        if let Some(panel) = workspace.render_config_panel_maximized(pane_group, config, app) {
            body.add_child(Expanded::new(1., panel).finish());
        }
    } else if !config.contains_item(&HeaderToolbarItemKind::CodeReview)
        && let Some(panel) = workspace.render_config_panel(
            &HeaderToolbarItemKind::CodeReview,
            pane_group,
            config,
            app,
        )
    {
        body.add_child(panel);
    }
    let main = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(header)
        .with_child(Expanded::new(1., body.finish()).finish())
        .finish();
    let main = Container::new(Clipped::new(main).finish())
        .with_background(appearance.theme().background())
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(12.)))
        .with_border(Border::all(1.).with_border_fill(palette.rule))
        .with_drop_shadow(DropShadow {
            color: palette.shadow,
            offset: vec2f(0., 2.),
            blur_radius: 12.,
            spread_radius: 0.,
        })
        .finish();
    let mut layout = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    if workspace.vertical_tabs_panel_open {
        layout.add_child(sidebar(workspace, &sessions, &palette, app));
    }
    layout.add_child(
        Expanded::new(
            1.,
            Container::new(main)
                .with_padding_top(8.)
                .with_padding_left(if workspace.vertical_tabs_panel_open {
                    0.
                } else {
                    8.
                })
                .with_padding_right(8.)
                .with_padding_bottom(8.)
                .finish(),
        )
        .finish(),
    );
    Container::new(layout.finish())
        .with_background(palette.background())
        .finish()
}

#[cfg(test)]
#[path = "torch_tests.rs"]
mod tests;
