#[path = "torch_motion.rs"]
mod motion;

use std::time::Duration;

use futures::StreamExt;
use instant::Instant;
use motion::{Motion, SessionMotion};
use warp_core::ui::theme::ColorScheme;
use warpui::ViewContext;

use super::*;
use crate::session_management::CommandContext;
use crate::ui_components::buttons::icon_button;
use crate::workspace::header_toolbar_item::HeaderToolbarItemKind;
use crate::workspace::view::PanelPosition;

pub(super) const SIDEBAR_WIDTH: f32 = 272.;
const HEADER_HEIGHT: f32 = 48.;
const ROW_HEIGHT: f32 = 28.;
const SCROLL_FOLLOW_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MotionPreference {
    Full,
    Reduced,
}

/// The system motion preference, or `None` where the sidebar places rows immediately.
fn motion_preference() -> Option<MotionPreference> {
    #[cfg(target_os = "macos")]
    {
        Some(
            if objc2_app_kit::NSWorkspace::sharedWorkspace()
                .accessibilityDisplayShouldReduceMotion()
            {
                MotionPreference::Reduced
            } else {
                MotionPreference::Full
            },
        )
    }
    // Keep placement immediate until the other backends expose their system motion preference.
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

fn arrival_motion(started: Option<Instant>) -> Option<(Motion, Instant)> {
    let started = started?;
    let motion = match motion_preference()? {
        MotionPreference::Full => Motion::Arrival,
        MotionPreference::Reduced => Motion::ReducedArrival,
    };
    (started.elapsed() < motion.duration()).then_some((motion, started))
}

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
        } else if matches!(theme.name().as_deref(), Some("Kanagawa" | "Ayu")) {
            let background = theme.background();
            let foreground = theme.foreground();
            Self {
                sidebar_top: background.blend(&foreground.with_opacity(4)).into_solid(),
                sidebar_bottom: background.blend(&foreground.with_opacity(2)).into_solid(),
                text: foreground.into_solid(),
                secondary: background.blend(&foreground.with_opacity(85)).into_solid(),
                muted: background.blend(&foreground.with_opacity(80)).into_solid(),
                selection: background.blend(&foreground.with_opacity(10)).into_solid(),
                hover: foreground.with_opacity(5).into_solid(),
                rule: background.blend(&foreground.with_opacity(18)).into_solid(),
                attention: theme.terminal_colors().bright.yellow.into(),
                header_rule: foreground.with_opacity(7).into_solid(),
                shadow: ColorU::from_u32(0x00000030),
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

#[derive(Clone)]
struct Session {
    index: usize,
    title: String,
    directory: String,
    group_key: String,
    group_label: String,
    project_directory: Option<PathBuf>,
    status: Option<ConversationStatus>,
    agent_status_unavailable: bool,
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

/// A closed session that stays drawn in the sidebar while it animates out.
pub(super) struct DepartingSession {
    /// The pane group of the closed tab.
    id: EntityId,
    /// The row drawn below this one when it closed, or `None` if it was last in its group.
    anchor: Option<EntityId>,
    /// Position of its group among the sidebar groups when it closed.
    group_position: usize,
    session: Session,
    selected: bool,
    active: bool,
    hovered: bool,
    started: Instant,
}

impl DepartingSession {
    fn is_animating(&self) -> bool {
        self.started.elapsed() < Motion::Departure.duration()
    }
}

/// Orders live rows and the rows departing from among them. `departures` are in the order they
/// closed; each returns to just above its anchor, which may itself be departing.
fn place_departures<Id: PartialEq + Copy, T>(
    live: Vec<(Id, T)>,
    departures: impl DoubleEndedIterator<Item = (Id, Option<Id>, T)>,
) -> Vec<T> {
    let mut rows = live;
    for (id, anchor, row) in departures.rev() {
        let position = anchor
            .and_then(|anchor| rows.iter().position(|(existing, _)| *existing == anchor))
            .unwrap_or(rows.len());
        rows.insert(position, (id, row));
    }
    rows.into_iter().map(|(_, row)| row).collect()
}

pub(in crate::workspace::view) fn session_order(
    workspace: &Workspace,
    app: &AppContext,
) -> Vec<usize> {
    let keys = sessions(workspace, app)
        .into_iter()
        .map(|session| session.group_key)
        .collect::<Vec<_>>();
    grouped_indices(&keys)
        .into_iter()
        .flatten()
        .filter(|index| {
            !workspace
                .vertical_tabs_panel
                .collapsed_torch_groups
                .contains(&keys[*index])
        })
        .collect()
}

impl Workspace {
    pub(in crate::workspace::view) fn animate_sidebar_arrival(
        &mut self,
        index: usize,
        ctx: &mut ViewContext<Self>,
    ) {
        if !warp_core::channel::ChannelState::is_local_only() || !self.vertical_tabs_panel_open {
            return;
        }
        let started = Instant::now();
        self.tabs[index].sidebar_arrival_started = Some(started);
        let Some((motion, _)) = arrival_motion(Some(started)) else {
            return;
        };
        let id = self.tabs[index].pane_group.id();
        let frames = motion.duration().as_millis() / SCROLL_FOLLOW_INTERVAL.as_millis() + 1;
        // The row repaints itself while it grows; keep re-arming the scroll target so a row added
        // at the bottom of a long list stays fully in view.
        ctx.spawn_stream_local(
            async_io::Timer::interval(SCROLL_FOLLOW_INTERVAL).take(frames as usize),
            move |workspace, _, _| {
                if workspace.vertical_tabs_panel_open
                    && workspace.active_tab_pane_group().id() == id
                {
                    workspace
                        .vertical_tabs_panel
                        .scroll_to_tab(workspace.active_tab_index);
                }
            },
            |_, _| {},
        );
    }

    pub(in crate::workspace::view) fn capture_sidebar_departure(
        &mut self,
        index: usize,
        ctx: &mut ViewContext<Self>,
    ) {
        self.vertical_tabs_panel
            .departing_sessions
            .retain(DepartingSession::is_animating);
        if !warp_core::channel::ChannelState::is_local_only()
            || !self.vertical_tabs_panel_open
            || motion_preference() != Some(MotionPreference::Full)
        {
            return;
        }
        let mut sessions = sessions(self, ctx);
        if index >= sessions.len() || group_collapsed(self, &sessions[index]) {
            return;
        }
        let keys = sessions
            .iter()
            .map(|session| session.group_key.clone())
            .collect::<Vec<_>>();
        let groups = grouped_indices(&keys);
        let Some((group_position, members)) = groups
            .iter()
            .enumerate()
            .find(|(_, members)| members.contains(&index))
        else {
            return;
        };
        let position = members.iter().position(|member| *member == index).unwrap();
        let anchor = members
            .get(position + 1)
            .map(|below| self.tabs[*below].pane_group.id());
        let tab = &self.tabs[index];
        let departing = DepartingSession {
            id: tab.pane_group.id(),
            anchor,
            group_position,
            selected: index == self.active_tab_index || tab.in_multi_selection,
            active: index == self.active_tab_index,
            hovered: tab
                .tab_mouse_state
                .lock()
                .is_ok_and(|state| state.is_hovered()),
            session: sessions.swap_remove(index),
            started: Instant::now(),
        };
        self.vertical_tabs_panel.departing_sessions.push(departing);
    }

    pub(in crate::workspace::view) fn reveal_sidebar_project(&mut self, directory: &Path) {
        self.vertical_tabs_panel
            .collapsed_torch_groups
            .remove(&format!("project:{}", directory.display()));
    }

    pub(in crate::workspace::view) fn toggle_sidebar_group_collapsed(
        &mut self,
        key: &str,
        ctx: &mut ViewContext<Self>,
    ) {
        let collapsed = &mut self.vertical_tabs_panel.collapsed_torch_groups;
        if !collapsed.remove(key) {
            collapsed.insert(key.to_owned());
        }
        ctx.notify();
    }

    pub(in crate::workspace::view) fn reveal_sidebar_session(
        &mut self,
        index: usize,
        ctx: &mut ViewContext<Self>,
    ) {
        if !self.vertical_tabs_panel.collapsed_torch_groups.is_empty()
            && let Some(session) = sessions(self, ctx).get(index)
        {
            self.vertical_tabs_panel
                .collapsed_torch_groups
                .remove(&session.group_key);
        }
        if let Some(group_id) = self.tabs.get(index).and_then(|tab| tab.group_id) {
            self.expand_tab_group(group_id, ctx);
        }
    }
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
            let startup_directory = tab
                .sidebar_startup_directory
                .as_ref()
                .filter(|_| terminal.is_some_and(|view| !view.is_login_shell_bootstrapped()));
            let directory = startup_directory
                .map(|path| {
                    let home = dirs::home_dir();
                    warp_util::path::user_friendly_path(
                        &path.to_string_lossy(),
                        home.as_ref().and_then(|path| path.to_str()),
                    )
                    .to_string()
                })
                .or_else(|| terminal.and_then(|view| view.display_working_directory(app)))
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
            let agent_status_unavailable = cli_session.is_some() && status.is_none();
            if status.is_none()
                && !agent_status_unavailable
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
            let live_directory = terminal.and_then(|view| view.canonical_session_pwd_if_local(app));
            let project = if tab.group_id.is_none() {
                startup_directory
                    .map(PathBuf::as_path)
                    .or_else(|| live_directory.as_ref().map(|path| path.as_path()))
                    .and_then(|path| {
                        crate::settings::project_for_directory(
                            &crate::settings::ProjectCatalog::as_ref(app).projects,
                            path,
                        )
                    })
            } else {
                None
            };
            let (group_key, group_label) = tab
                .group_id
                .and_then(|id| {
                    workspace
                        .tab_groups
                        .get(&id)
                        .map(|group| (format!("manual:{id:?}"), group_display_name(group)))
                })
                .unwrap_or_else(|| {
                    project
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
                project_directory: project.map(|project| project.path.clone()),
                status,
                agent_status_unavailable,
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

#[derive(Debug, PartialEq, Eq)]
enum SessionIndicator {
    Working,
    NeedsInput,
    UnreadComplete,
    Complete,
    Failed,
    Idle,
    Unknown,
}

fn session_indicator(
    status: Option<&ConversationStatus>,
    unread: bool,
    agent_status_unavailable: bool,
) -> SessionIndicator {
    match status {
        Some(ConversationStatus::InProgress) => SessionIndicator::Working,
        Some(ConversationStatus::Blocked { .. }) => SessionIndicator::NeedsInput,
        Some(ConversationStatus::Success) if unread => SessionIndicator::UnreadComplete,
        Some(ConversationStatus::Success) => SessionIndicator::Complete,
        Some(ConversationStatus::Error | ConversationStatus::TransientError) => {
            SessionIndicator::Failed
        }
        Some(ConversationStatus::Cancelled | ConversationStatus::WaitingForEvents) => {
            SessionIndicator::Idle
        }
        None if agent_status_unavailable => SessionIndicator::Unknown,
        None => SessionIndicator::Idle,
    }
}

fn status_icon(session: &Session, palette: &Palette) -> Box<dyn Element> {
    let (asset, color) = match session_indicator(
        session.status.as_ref(),
        session.unread,
        session.agent_status_unavailable,
    ) {
        SessionIndicator::Working => ("bundled/svg/torch-session-running.svg", palette.text),
        SessionIndicator::NeedsInput => {
            ("bundled/svg/torch-session-attention.svg", palette.attention)
        }
        SessionIndicator::UnreadComplete => {
            ("bundled/svg/torch-session-complete.svg", palette.text)
        }
        SessionIndicator::Complete => ("bundled/svg/torch-session-check.svg", palette.muted),
        SessionIndicator::Failed => ("bundled/svg/torch-session-error.svg", palette.attention),
        SessionIndicator::Idle | SessionIndicator::Unknown => {
            ("bundled/svg/torch-session-idle.svg", palette.muted)
        }
    };
    ConstrainedBox::new(warpui::elements::Icon::new(asset, color).finish())
        .with_width(16.)
        .with_height(16.)
        .finish()
}

fn group_collapsed(workspace: &Workspace, session: &Session) -> bool {
    workspace.tabs[session.index]
        .group_id
        .and_then(|id| workspace.tab_groups.get(&id))
        .map_or_else(
            || {
                workspace
                    .vertical_tabs_panel
                    .collapsed_torch_groups
                    .contains(&session.group_key)
            },
            |group| group.collapsed,
        )
}

fn session_title(
    session: &Session,
    palette: &Palette,
    appearance: &Appearance,
) -> Box<dyn Element> {
    label(
        &session.title,
        13.,
        palette.text,
        if session.unread {
            Weight::Semibold
        } else {
            Weight::Normal
        },
        appearance,
    )
}

fn session_age(session: &Session, palette: &Palette, appearance: &Appearance) -> Box<dyn Element> {
    label(
        &session.age,
        12.,
        palette.secondary,
        Weight::Normal,
        appearance,
    )
}

fn session_status(session: &Session, palette: &Palette) -> Box<dyn Element> {
    if session.status.is_none() && session.age.is_empty() && !session.agent_status_unavailable {
        Empty::new().finish()
    } else {
        status_icon(session, palette)
    }
}

struct RowParts {
    title: Box<dyn Element>,
    age_or_close: Box<dyn Element>,
    status: Box<dyn Element>,
}

fn row_surface(parts: RowParts, background: ElementFill, border: ElementFill) -> Box<dyn Element> {
    let trailing = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(10.)
        .with_child(
            ConstrainedBox::new(Align::new(parts.age_or_close).finish())
                .with_width(28.)
                .with_height(20.)
                .finish(),
        )
        .with_child(
            ConstrainedBox::new(Align::new(parts.status).finish())
                .with_width(20.)
                .with_height(20.)
                .finish(),
        )
        .finish();
    let contents = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(10.)
        .with_child(Expanded::new(1., parts.title).finish())
        .with_child(trailing)
        .finish();
    ConstrainedBox::new(
        Container::new(contents)
            .with_padding(Padding::uniform(3.).with_horizontal(12.))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
            .with_background(background)
            .with_border(Border::all(1.).with_border_fill(border))
            .finish(),
    )
    .with_height(ROW_HEIGHT)
    .finish()
}

/// A non-interactive copy of a closed session's row, as it looked when it closed.
fn departing_row(
    departing: &DepartingSession,
    palette: &Palette,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let session = &departing.session;
    let parts = RowParts {
        title: session_title(session, palette, appearance),
        age_or_close: if departing.hovered {
            Empty::new().finish()
        } else {
            session_age(session, palette, appearance)
        },
        status: session_status(session, palette),
    };
    let background = if departing.selected {
        ElementFill::Solid(palette.selection)
    } else if departing.hovered {
        ElementFill::Solid(palette.hover)
    } else {
        ElementFill::None
    };
    let border = if departing.active {
        ElementFill::Solid(palette.rule)
    } else {
        ElementFill::None
    };
    row_surface(parts, background, border)
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
        session_title(session, palette, appearance)
    };
    let age = session_age(session, palette, appearance);
    let status = session_status(session, palette);
    let tooltip = appearance
        .ui_builder()
        .tool_tip("Close session".to_string())
        .build()
        .finish();
    let close_button = icon_button(
        appearance,
        UiIcon::X,
        false,
        workspace.tabs[index].close_mouse_state.clone(),
    )
    .with_style(UiComponentStyles::default().set_width(20.).set_height(20.))
    .with_tooltip(move || tooltip)
    .build()
    .on_click(move |ctx, _, _| ctx.dispatch_typed_action(WorkspaceAction::CloseTab(index)))
    .finish();
    let row = Hoverable::new(
        workspace.tabs[index].tab_mouse_state.clone(),
        move |state| {
            let parts = RowParts {
                title,
                age_or_close: if state.is_hovered() {
                    close_button
                } else {
                    age
                },
                status,
            };
            let background = if active || in_multi_selection {
                ElementFill::Solid(selected)
            } else if state.is_hovered() {
                ElementFill::Solid(hover)
            } else {
                ElementFill::None
            };
            let border = if active {
                ElementFill::Solid(rule)
            } else {
                ElementFill::None
            };
            row_surface(parts, background, border)
        },
    )
    .with_defer_events_to_children()
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
    Draggable::new(workspace.tabs[index].draggable_state.clone(), row)
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
        .finish()
}

enum SidebarRow<'a> {
    Live(&'a Session),
    Departing(&'a DepartingSession),
}

fn group_title(
    label_text: &str,
    count: usize,
    collapsed: bool,
    palette: &Palette,
    appearance: &Appearance,
) -> Box<dyn Element> {
    Flex::row()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.)
        .with_child(icon(
            if collapsed {
                WarpIcon::ChevronRight
            } else {
                WarpIcon::ChevronDown
            },
            12.,
            palette.secondary,
        ))
        .with_child(
            Shrinkable::new(
                1.,
                label(label_text, 12., palette.muted, Weight::Semibold, appearance),
            )
            .finish(),
        )
        .with_child(label(
            count.to_string(),
            12.,
            palette.muted,
            Weight::Normal,
            appearance,
        ))
        .finish()
}

fn padded_heading(content: Box<dyn Element>) -> Box<dyn Element> {
    Container::new(content)
        .with_padding(
            Padding::uniform(0.)
                .with_left(12.)
                .with_right(10.)
                .with_top(12.)
                .with_bottom(7.),
        )
        .finish()
}

fn group_spacer() -> Box<dyn Element> {
    ConstrainedBox::new(Empty::new().finish())
        .with_height(15.)
        .finish()
}

fn live_group(
    members: &[&Session],
    workspace: &Workspace,
    palette: &Palette,
    app: &AppContext,
) -> Box<dyn Element> {
    let appearance = Appearance::as_ref(app);
    let state = &workspace.vertical_tabs_panel;
    let mut group_content = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    let needs = members
        .iter()
        .filter(|session| matches!(session.status, Some(ConversationStatus::Blocked { .. })))
        .count();
    let group = workspace.tabs[members[0].index]
        .group_id
        .and_then(|id| workspace.tab_groups.get(&id));
    let collapsed = group_collapsed(workspace, members[0]);
    let title = group_title(
        &members[0].group_label,
        members.len(),
        collapsed,
        palette,
        appearance,
    );
    let mut heading = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.)
        .with_child(Shrinkable::new(1., title).finish());
    if needs > 0 {
        heading.add_child(label(
            format!("{needs} needs you"),
            12.,
            palette.attention,
            Weight::Medium,
            appearance,
        ));
    }
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
        Hoverable::new(mouse, |_| padded_heading(content))
            .with_cursor(Cursor::PointingHand)
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
        let key = members[0].group_key.clone();
        let mouse_states = state
            .torch_group_mouse_states
            .borrow_mut()
            .entry(key.clone())
            .or_default()
            .clone();
        let new_session = members[0].project_directory.as_ref().map(|directory| {
            let directory = directory.clone();
            let tooltip = appearance
                .ui_builder()
                .tool_tip(format!("New session in {}", members[0].group_label))
                .build()
                .finish();
            icon_button(
                appearance,
                UiIcon::Plus,
                false,
                mouse_states.new_session.clone(),
            )
            .with_style(UiComponentStyles::default().set_width(20.).set_height(20.))
            .with_tooltip(move || tooltip)
            .build()
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(WorkspaceAction::NewSessionInDirectory(directory.clone()))
            })
            .finish()
        });
        Hoverable::new(mouse_states.header, move |mouse| {
            if let Some(button) = new_session {
                heading.add_child(if mouse.is_hovered() {
                    button
                } else {
                    ConstrainedBox::new(Empty::new().finish())
                        .with_width(20.)
                        .with_height(20.)
                        .finish()
                });
            }
            padded_heading(heading.finish())
        })
        .with_defer_events_to_children()
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(WorkspaceAction::ToggleSidebarGroupCollapsed(key.clone()))
        })
        .finish()
    };
    group_content.add_child(heading);
    let departing = state
        .departing_sessions
        .iter()
        .filter(|departing| {
            departing.is_animating() && departing.session.group_key == members[0].group_key
        })
        .collect::<Vec<_>>();
    // A group created by its only session arrives as a whole, heading included.
    let group_arrival = (members.len() == 1 && departing.is_empty())
        .then(|| arrival_motion(workspace.tabs[members[0].index].sidebar_arrival_started))
        .flatten();
    if !collapsed {
        let mut rows = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.);
        let live = members
            .iter()
            .map(|session| {
                (
                    workspace.tabs[session.index].pane_group.id(),
                    SidebarRow::Live(session),
                )
            })
            .collect();
        let departures = departing.iter().map(|departing| {
            (
                departing.id,
                departing.anchor,
                SidebarRow::Departing(departing),
            )
        });
        for entry in place_departures(live, departures) {
            rows.add_child(match entry {
                SidebarRow::Live(session) => {
                    let element = row(session, workspace, palette, app);
                    let element =
                        match arrival_motion(workspace.tabs[session.index].sidebar_arrival_started)
                        {
                            Some((motion, started)) if group_arrival.is_none() => {
                                SessionMotion::new(element, motion, started).finish()
                            }
                            Some(_) | None => element,
                        };
                    SavePosition::new(element, &tab_position_id(session.index)).finish()
                }
                SidebarRow::Departing(departing) => SessionMotion::new(
                    departing_row(departing, palette, appearance),
                    Motion::Departure,
                    departing.started,
                )
                .finish(),
            });
        }
        group_content.add_child(rows.finish());
    }
    group_content.add_child(group_spacer());
    let group_content = match group_arrival {
        Some((motion, started)) => {
            SessionMotion::new(group_content.finish(), motion, started).finish()
        }
        None => group_content.finish(),
    };
    if let Some(group) = group {
        SavePosition::new(group_content, &vtab_group_position_id(group.id)).finish()
    } else {
        group_content
    }
}

/// A group whose last sessions closed, leaving with them.
fn vanished_group(
    members: &[&DepartingSession],
    palette: &Palette,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let heading = padded_heading(group_title(
        &members[0].session.group_label,
        members.len(),
        false,
        palette,
        appearance,
    ));
    let mut rows = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(2.);
    for departing in members {
        rows.add_child(departing_row(departing, palette, appearance));
    }
    let content = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(heading)
        .with_child(rows.finish())
        .with_child(group_spacer())
        .finish();
    let started = members
        .iter()
        .map(|departing| departing.started)
        .max()
        .expect("vanished groups have members");
    SessionMotion::new(content, Motion::Departure, started).finish()
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
    let mut groups = grouped_indices(&keys)
        .into_iter()
        .map(|indices| {
            let members = indices
                .into_iter()
                .map(|index| &sessions[index])
                .collect::<Vec<_>>();
            live_group(&members, workspace, palette, app)
        })
        .collect::<Vec<_>>();
    let mut vanished: Vec<(usize, Vec<&DepartingSession>)> = Vec::new();
    for departing in state.departing_sessions.iter().filter(|departing| {
        departing.is_animating() && !keys.contains(&departing.session.group_key)
    }) {
        match vanished
            .iter_mut()
            .find(|(_, members)| members[0].session.group_key == departing.session.group_key)
        {
            Some((_, members)) => members.push(departing),
            None => vanished.push((departing.group_position, vec![departing])),
        }
    }
    vanished.sort_by_key(|(position, _)| *position);
    for (position, members) in vanished {
        groups.insert(
            position.min(groups.len()),
            vanished_group(&members, palette, appearance),
        );
    }
    let mut list = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    for group in groups {
        list.add_child(group);
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
