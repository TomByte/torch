#[path = "torch_motion.rs"]
mod motion;

use std::cell::Cell;
use std::time::Duration;

use futures::StreamExt;
use instant::Instant;
use motion::{
    CardEntrance, Fade, HOVER_IN, HOVER_OUT, Motion, RowFills, RowSurface, SessionMotion,
    WorkingPulse,
};
use warp_core::ui::theme::{AnsiColors, ColorScheme};
use warp_errors::report_if_error;
use warpui::ViewContext;
use warpui::elements::DraggableState;

use super::*;
use crate::menu::{MenuItem, MenuItemFields};
use crate::palette::PaletteMode;
use crate::server::telemetry::PaletteSource;
use crate::tab::{ColorPickerTarget, color_picker_menu_items};
use crate::terminal::CLIAgent;
use crate::terminal::cli_agent_sessions::{
    BlockedSource, CLIAgentSessionStatus, CLIAgentSessionsModelEvent,
};
use crate::themes::theme::AnsiColorIdentifier;
use crate::ui_components::buttons::icon_button;
use crate::ui_components::elevation::soft_shadow;
use crate::ui_components::motion::{MotionPreference, Reveal, Revealing, motion_preference};
use crate::workspace::TabMovement;
use crate::workspace::header_toolbar_item::HeaderToolbarItemKind;
use crate::workspace::tab_settings::DirectoryTabColor;
use crate::workspace::view::PanelPosition;

pub(super) const SIDEBAR_WIDTH: f32 = 272.;
/// Built-in themes whose sidebar is drawn from their own colours rather than Torch's Light or Dark.
const THEMED_SIDEBARS: [&str; 6] = [
    "Kanagawa",
    "Ayu",
    "Catppuccin Latte",
    "Catppuccin Frappé",
    "Catppuccin Macchiato",
    "Catppuccin Mocha",
];
const HEADER_HEIGHT: f32 = 48.;
const ROW_HEIGHT: f32 = 28.;
const SCROLL_FOLLOW_INTERVAL: Duration = Duration::from_millis(16);

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
    working: ColorU,
    header_rule: ColorU,
    /// The broad, soft shadow that lifts the terminal pane off the sidebar.
    shadow: ColorU,
    /// The tight shadow that seats the terminal pane's edge.
    contact_shadow: ColorU,
    /// The hairline that crisps the active session's card.
    card_edge: ColorU,
    card_shadow: ColorU,
}

impl Palette {
    fn new(theme: &WarpTheme) -> Self {
        let follows_theme = theme
            .name()
            .is_some_and(|name| THEMED_SIDEBARS.contains(&name.as_str()));
        match (theme.inferred_color_scheme(), follows_theme) {
            (ColorScheme::DarkOnLight, true) => Self::themed_light(theme),
            (ColorScheme::DarkOnLight, false) => Self {
                sidebar_top: ColorU::from_u32(0xE6ECF4FF),
                sidebar_bottom: ColorU::from_u32(0xEEE9F4FF),
                text: ColorU::from_u32(0x202124FF),
                secondary: ColorU::from_u32(0x616268FF),
                muted: ColorU::from_u32(0x85868BFF),
                selection: ColorU::from_u32(0xFFFFFFF5),
                hover: ColorU::from_u32(0xFFFFFF70),
                rule: ColorU::from_u32(0xD5D6DBFF),
                attention: ColorU::from_u32(0x98651FFF),
                working: ColorU::from_u32(0xC15F3CFF),
                header_rule: ColorU::from_u32(0x88888818),
                shadow: ColorU::from_u32(0x1A1F361C),
                contact_shadow: ColorU::from_u32(0x1A1F3612),
                card_edge: ColorU::from_u32(0x0000000A),
                card_shadow: ColorU::from_u32(0x1A1F3626),
            },
            (ColorScheme::LightOnDark, true) => Self::themed_dark(theme),
            (ColorScheme::LightOnDark, false) => Self {
                sidebar_top: ColorU::from_u32(0x2B2B2BFF),
                sidebar_bottom: ColorU::from_u32(0x282828FF),
                text: ColorU::from_u32(0xE6E6E6FF),
                secondary: ColorU::from_u32(0xB5B5B5FF),
                muted: ColorU::from_u32(0xA8A8A8FF),
                selection: ColorU::from_u32(0x3B3B3BFF),
                hover: ColorU::from_u32(0xFFFFFF12),
                rule: ColorU::from_u32(0x424242FF),
                attention: ColorU::from_u32(0xD9B96EFF),
                working: ColorU::from_u32(0xD97757FF),
                header_rule: ColorU::from_u32(0xFFFFFF12),
                shadow: ColorU::from_u32(0x00000059),
                contact_shadow: ColorU::from_u32(0x00000047),
                card_edge: ColorU::from_u32(0xFFFFFF0B),
                card_shadow: ColorU::from_u32(0x00000033),
            },
        }
    }

    fn themed_dark(theme: &WarpTheme) -> Self {
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
            working: ColorU::from_u32(0xD97757FF),
            header_rule: foreground.with_opacity(7).into_solid(),
            shadow: ColorU::from_u32(0x00000059),
            contact_shadow: ColorU::from_u32(0x00000047),
            card_edge: foreground.with_opacity(5).into_solid(),
            card_shadow: ColorU::from_u32(0x00000033),
        }
    }

    /// Light themes need deeper text blends than dark ones to stay readable, and a darkened yellow
    /// for attention, since a light theme's own yellow is drawn for a light-on-dark terminal cell.
    fn themed_light(theme: &WarpTheme) -> Self {
        let background = theme.background();
        let foreground = theme.foreground();
        let yellow: ColorU = theme.terminal_colors().normal.yellow.into();
        Self {
            sidebar_top: background.blend(&foreground.with_opacity(5)).into_solid(),
            sidebar_bottom: background.blend(&foreground.with_opacity(3)).into_solid(),
            text: foreground.into_solid(),
            secondary: background.blend(&foreground.with_opacity(90)).into_solid(),
            muted: background.blend(&foreground.with_opacity(85)).into_solid(),
            selection: background
                .into_solid()
                .blend(&coloru_with_opacity(ColorU::white(), 70)),
            hover: ColorU::from_u32(0xFFFFFF70),
            rule: background.blend(&foreground.with_opacity(15)).into_solid(),
            attention: yellow.blend(&coloru_with_opacity(ColorU::black(), 40)),
            working: ColorU::from_u32(0xC15F3CFF),
            header_rule: foreground.with_opacity(8).into_solid(),
            shadow: ColorU::from_u32(0x1A1F361C),
            contact_shadow: ColorU::from_u32(0x1A1F3612),
            card_edge: ColorU::from_u32(0x0000000A),
            card_shadow: ColorU::from_u32(0x1A1F3626),
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

    fn row_fills(&self) -> RowFills {
        RowFills {
            hover: self.hover,
            selection: self.selection,
            card_edge: self.card_edge,
            card_shadow: self.card_shadow,
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

/// Every tab in sidebar order, paired with whether its group is expanded.
fn sidebar_rows(workspace: &Workspace, app: &AppContext) -> Vec<(usize, bool)> {
    let sessions = sessions(workspace, app);
    let keys = sessions
        .iter()
        .map(|session| session.group_key.clone())
        .collect::<Vec<_>>();
    grouped_indices(&keys)
        .into_iter()
        .flatten()
        .map(|index| (index, !group_collapsed(workspace, &sessions[index])))
        .collect()
}

/// The first expanded row `step` rows at a time away from `current`, wrapping around the list.
fn expanded_row_from(rows: &[(usize, bool)], current: usize, step: isize) -> Option<usize> {
    let start = rows.iter().position(|(index, _)| *index == current)? as isize;
    let len = rows.len() as isize;
    (1..=len)
        .map(|offset| rows[(start + step * offset).rem_euclid(len) as usize])
        .find(|(_, expanded)| *expanded)
        .map(|(index, _)| index)
}

impl Workspace {
    /// The session `step` visible rows from the active one in the sidebar, wrapping around.
    pub(in crate::workspace::view) fn sidebar_session_from_active(
        &self,
        step: isize,
        ctx: &AppContext,
    ) -> Option<usize> {
        expanded_row_from(&sidebar_rows(self, ctx), self.active_tab_index, step)
    }

    /// The session at a zero-based position among the visible sidebar rows.
    pub(in crate::workspace::view) fn sidebar_session_at(
        &self,
        position: usize,
        ctx: &AppContext,
    ) -> Option<usize> {
        sidebar_rows(self, ctx)
            .into_iter()
            .filter(|(_, expanded)| *expanded)
            .nth(position)
            .map(|(index, _)| index)
    }

    pub(in crate::workspace::view) fn last_sidebar_session(
        &self,
        ctx: &AppContext,
    ) -> Option<usize> {
        sidebar_rows(self, ctx)
            .into_iter()
            .filter(|(_, expanded)| *expanded)
            .last()
            .map(|(index, _)| index)
    }

    /// Moves a session one row up or down within its sidebar group.
    pub(in crate::workspace::view) fn move_sidebar_session(
        &mut self,
        index: usize,
        direction: TabMovement,
        ctx: &mut ViewContext<Self>,
    ) {
        let (above, below) = drag_neighbors(self, index, ctx);
        // The later tab always moves to the earlier one's position, so no group's first
        // occurrence moves and the groups keep their sidebar order.
        match direction {
            TabMovement::Left => {
                if let Some(above) = above {
                    self.hop_tab_to_index(index, above, ctx);
                }
            }
            TabMovement::Right => {
                if let Some(below) = below {
                    self.hop_tab_to_index(below, index, ctx);
                }
            }
        }
    }

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

    /// Refreshes sidebar activity ages at each wall-clock minute while the sidebar shows them.
    pub(in crate::workspace::view) fn schedule_sidebar_age_refresh(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        let into_minute =
            Duration::from_millis(chrono::Local::now().timestamp_millis() as u64 % 60_000);
        ctx.spawn(
            warpui::r#async::Timer::after(Duration::from_secs(60) - into_minute),
            |workspace, _, ctx| {
                if workspace.vertical_tabs_panel_open {
                    workspace
                        .vertical_tabs_panel
                        .torch_sessions
                        .invalidate_all();
                    ctx.notify();
                }
                workspace.schedule_sidebar_age_refresh(ctx);
            },
        );
    }

    /// Swaps the dragged group with the neighbor whose middle the cursor has crossed. Comparing
    /// the cursor rather than the group's edges means a swap back needs the cursor to travel the
    /// dragged group's full height, so the groups never flicker between orders.
    pub(in crate::workspace::view) fn drag_sidebar_group(
        &mut self,
        key: &str,
        cursor_position: Vector2F,
        ctx: &mut ViewContext<Self>,
    ) {
        let keys = sessions(self, ctx)
            .into_iter()
            .map(|session| session.group_key)
            .collect::<Vec<_>>();
        let groups = grouped_indices(&keys);
        let Some(position) = groups.iter().position(|members| keys[members[0]] == key) else {
            return;
        };
        let middle = |group: usize| {
            ctx.element_position_by_id(group_position_id(&keys[groups[group][0]]))
                .map(|rect| rect.center().y())
        };
        let upper = if position > 0 && middle(position - 1).is_some_and(|y| cursor_position.y() < y)
        {
            position - 1
        } else if position + 1 < groups.len()
            && middle(position + 1).is_some_and(|y| cursor_position.y() > y)
        {
            position
        } else {
            return;
        };
        let order = swap_adjacent_groups(groups, upper);
        self.reorder_tabs(&order, ctx);
    }

    /// Rearranges the tabs into `order`, a permutation of their indices, keeping the same tab
    /// active.
    fn reorder_tabs(&mut self, order: &[usize], ctx: &mut ViewContext<Self>) {
        let active = self.tabs[self.active_tab_index].pane_group.id();
        let mut tabs = std::mem::take(&mut self.tabs)
            .into_iter()
            .map(Some)
            .collect::<Vec<_>>();
        self.tabs = order
            .iter()
            .map(|index| {
                tabs[*index]
                    .take()
                    .expect("order is a permutation of the tabs")
            })
            .collect();
        self.active_tab_index = self
            .tabs
            .iter()
            .position(|tab| tab.pane_group.id() == active)
            .unwrap_or_default();
        ctx.notify();
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

/// What a project heading's new-session menu offers. A plain shell comes first so the menu
/// never hides the previous one-click default.
pub(in crate::workspace::view) fn project_session_menu_items(
    directory: &Path,
) -> Vec<MenuItem<WorkspaceAction>> {
    let mut items = vec![
        MenuItemFields::new("Shell")
            .with_on_select_action(WorkspaceAction::NewSessionInDirectory(directory.to_owned()))
            .into_item(),
    ];
    for agent in [CLIAgent::Claude, CLIAgent::Codex] {
        items.push(
            MenuItemFields::new(agent.display_name())
                .with_on_select_action(WorkspaceAction::NewAgentSessionInDirectory {
                    directory: directory.to_owned(),
                    agent,
                })
                .into_item(),
        );
    }
    if crate::tab_configs::session_config::is_git_repo(directory) {
        items.push(
            MenuItemFields::new("Claude Code in new worktree")
                .with_on_select_action(WorkspaceAction::NewAgentWorktreeSession {
                    directory: directory.to_owned(),
                    agent: CLIAgent::Claude,
                })
                .into_item(),
        );
    }
    items
}

/// A project heading's context menu: its color, then the session kinds it can start.
pub(in crate::workspace::view) fn project_group_menu_items(
    directory: &Path,
    terminal_colors: AnsiColors,
    app: &AppContext,
) -> Vec<MenuItem<WorkspaceAction>> {
    let mut items = color_picker_menu_items(
        directory_color(directory, app),
        terminal_colors,
        ColorPickerTarget::Directory {
            directory: directory.to_owned(),
        },
    );
    items.push(MenuItem::Separator);
    items.extend(project_session_menu_items(directory));
    items
}

fn directory_color(directory: &Path, app: &AppContext) -> Option<AnsiColorIdentifier> {
    TabSettings::as_ref(app)
        .directory_tab_colors
        .value()
        .color_for_directory(directory)
        .and_then(DirectoryTabColor::ansi_color)
}

pub(in crate::workspace::view) fn toggle_directory_color(
    directory: &Path,
    color: AnsiColorIdentifier,
    ctx: &mut AppContext,
) {
    let next = if directory_color(directory, ctx) == Some(color) {
        DirectoryTabColor::Unassigned
    } else {
        DirectoryTabColor::Color(color)
    };
    TabSettings::handle(ctx).update(ctx, |settings, ctx| {
        let value = settings
            .directory_tab_colors
            .value()
            .with_color(directory, next);
        report_if_error!(settings.directory_tab_colors.set_value(value, ctx));
    });
}

fn swap_adjacent_groups(mut groups: Vec<Vec<usize>>, upper: usize) -> Vec<usize> {
    groups.swap(upper, upper + 1);
    groups.into_iter().flatten().collect()
}

fn group_position_id(key: &str) -> String {
    format!("torch_sidebar:group:{key}")
}

/// Lower ranks need attention sooner.
fn attention_rank(session: &Session) -> Option<u8> {
    match session_indicator(
        session.status.as_ref(),
        session.unread,
        session.agent_status_unavailable,
    ) {
        SessionIndicator::NeedsInput => Some(0),
        SessionIndicator::UnreadComplete => Some(1),
        SessionIndicator::Failed if session.unread => Some(1),
        SessionIndicator::Working
        | SessionIndicator::Complete
        | SessionIndicator::Failed
        | SessionIndicator::Idle
        | SessionIndicator::Unknown => None,
    }
}

/// The first session after `active` in sidebar `order`, wrapping around, among those with the
/// lowest attention rank. The active session itself is never chosen.
fn next_in_attention_order(
    order: &[usize],
    rank: impl Fn(usize) -> Option<u8>,
    active: usize,
) -> Option<usize> {
    let best = order
        .iter()
        .filter(|index| **index != active)
        .filter_map(|index| rank(*index))
        .min()?;
    let start = order
        .iter()
        .position(|index| *index == active)
        .map_or(0, |position| position + 1);
    order[start..]
        .iter()
        .chain(&order[..start])
        .copied()
        .find(|index| *index != active && rank(*index) == Some(best))
}

pub(in crate::workspace::view) fn next_attention_session(
    workspace: &Workspace,
    app: &AppContext,
) -> Option<usize> {
    let sessions = sessions(workspace, app);
    let keys = sessions
        .iter()
        .map(|session| session.group_key.clone())
        .collect::<Vec<_>>();
    let order = grouped_indices(&keys)
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    next_in_attention_order(
        &order,
        |index| attention_rank(&sessions[index]),
        workspace.active_tab_index,
    )
}

/// Shows the number of agent sessions waiting on the user on the Dock icon.
pub(in crate::workspace::view) fn update_attention_badge(
    event: &CLIAgentSessionsModelEvent,
    app: &AppContext,
) {
    match event {
        CLIAgentSessionsModelEvent::Started { .. }
        | CLIAgentSessionsModelEvent::StatusChanged { .. }
        | CLIAgentSessionsModelEvent::Ended { .. } => {
            let count = CLIAgentSessionsModel::as_ref(app).blocked_session_count();
            app.set_dock_badge((count > 0).then(|| count.to_string()));
        }
        CLIAgentSessionsModelEvent::InputSessionChanged { .. }
        | CLIAgentSessionsModelEvent::SessionUpdated { .. } => {}
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

/// How long terminal-derived row data may be reused when no invalidating event arrives.
const SNAPSHOT_LIFETIME: Duration = Duration::from_secs(1);

/// The directory or project that keeps a session in one sidebar group after it changes directory.
#[derive(Clone, Debug, PartialEq, Eq)]
enum GroupAnchor {
    Project(PathBuf),
    Folder(String),
}

/// Keeps a session's first project match, otherwise its first known directory. A session that
/// later enters a project joins it, so projects discovered after launch still group their sessions.
fn next_anchor(
    previous: Option<&GroupAnchor>,
    project: Option<&Path>,
    directory: &str,
    is_known_project: impl Fn(&Path) -> bool,
) -> Option<GroupAnchor> {
    match (previous, project) {
        (Some(GroupAnchor::Project(path)), _) if is_known_project(path) => previous.cloned(),
        (_, Some(project)) => Some(GroupAnchor::Project(project.to_owned())),
        (Some(GroupAnchor::Folder(_)), None) => previous.cloned(),
        (Some(GroupAnchor::Project(_)) | None, None) => {
            (!directory.trim_end_matches(['/', '\\']).is_empty())
                .then(|| GroupAnchor::Folder(directory.to_owned()))
        }
    }
}

/// Orders statuses by how urgently they need the user, so a tab surfaces its most urgent pane.
fn status_urgency(status: Option<&ConversationStatus>, unread: bool) -> u8 {
    match status {
        Some(ConversationStatus::Blocked { .. }) => 4,
        Some(ConversationStatus::InProgress) => 3,
        Some(ConversationStatus::Success) if unread => 2,
        Some(ConversationStatus::Error | ConversationStatus::TransientError) => 1,
        Some(
            ConversationStatus::Success
            | ConversationStatus::Cancelled
            | ConversationStatus::WaitingForEvents,
        )
        | None => 0,
    }
}

struct PaneStatus {
    status: Option<ConversationStatus>,
    unread: bool,
    agent_status_unavailable: bool,
}

/// The most urgent pane status; ties keep the earliest pane.
fn most_urgent(statuses: impl IntoIterator<Item = PaneStatus>) -> Option<PaneStatus> {
    statuses
        .into_iter()
        .fold(None, |best, candidate| match best {
            Some(best)
                if status_urgency(best.status.as_ref(), best.unread)
                    >= status_urgency(candidate.status.as_ref(), candidate.unread) =>
            {
                Some(best)
            }
            Some(_) | None => Some(candidate),
        })
}

fn pane_status(
    view: &TerminalView,
    is_running_command: impl FnOnce() -> bool,
    app: &AppContext,
) -> PaneStatus {
    let mut status = summary_conversation_status_for_terminal(view, app);
    let agent_status_unavailable = status.is_none()
        && CLIAgentSessionsModel::as_ref(app)
            .session(view.id())
            .is_some();
    if status.is_none() && !agent_status_unavailable && is_running_command() {
        status = Some(ConversationStatus::InProgress);
    }
    PaneStatus {
        status,
        unread: has_unread_activity_for_terminal_view(view.id(), app),
        agent_status_unavailable,
    }
}

/// Terminal-derived row data for one tab.
#[derive(Clone)]
struct TerminalSnapshot {
    captured: Instant,
    title: String,
    directory: String,
    /// The directory used for project matching: the launch directory until the shell reports one.
    project_lookup: Option<PathBuf>,
    status: Option<ConversationStatus>,
    agent_status_unavailable: bool,
    age: String,
    unread: bool,
}

/// Sidebar row data cached per pane group, so renders do not lock every terminal model.
#[derive(Default)]
pub(in crate::workspace::view) struct SessionCache {
    snapshots: RefCell<HashMap<EntityId, TerminalSnapshot>>,
    anchors: RefCell<HashMap<EntityId, GroupAnchor>>,
    /// Agent panes across every window, for the sidebar's Agents panel.
    agents: RefCell<Option<(Instant, Vec<AgentPane>)>>,
}

impl SessionCache {
    pub(in crate::workspace::view) fn invalidate(&mut self, pane_group: EntityId) {
        self.snapshots.get_mut().remove(&pane_group);
    }

    pub(in crate::workspace::view) fn invalidate_all(&mut self) {
        self.snapshots.get_mut().clear();
        *self.agents.get_mut() = None;
    }

    fn snapshot(&self, tab: &TabData, app: &AppContext) -> TerminalSnapshot {
        let id = tab.pane_group.id();
        if let Some(snapshot) = self.snapshots.borrow().get(&id)
            && snapshot.captured.elapsed() < SNAPSHOT_LIFETIME
        {
            return snapshot.clone();
        }
        let snapshot = capture_snapshot(tab, app);
        self.snapshots.borrow_mut().insert(id, snapshot.clone());
        snapshot
    }

    fn prune(&self, tabs: &[TabData]) {
        if self.snapshots.borrow().len() <= tabs.len() && self.anchors.borrow().len() <= tabs.len()
        {
            return;
        }
        let live = |id: &EntityId| tabs.iter().any(|tab| tab.pane_group.id() == *id);
        self.snapshots.borrow_mut().retain(|id, _| live(id));
        self.anchors.borrow_mut().retain(|id, _| live(id));
    }
}

/// The active session as of the last render, and how its card arrived there.
#[derive(Clone, Copy)]
struct Selection {
    pane_group: EntityId,
    /// Its tab index, which names its row's saved position.
    index: usize,
    card: CardEntrance,
}

/// Row hover fades and the active session's card entrance, carried between renders.
#[derive(Default)]
pub(in crate::workspace::view) struct RowTransitions {
    hovers: RefCell<HashMap<EntityId, Fade>>,
    selection: Cell<Option<Selection>>,
}

impl RowTransitions {
    /// Records the active session, starting its card's entrance when it differs from the one
    /// active at the last render.
    fn select(&self, workspace: &Workspace, app: &AppContext) {
        let index = workspace.active_tab_index;
        let Some(tab) = workspace.tabs.get(index) else {
            return;
        };
        let pane_group = tab.pane_group.id();
        let card = match self.selection.get() {
            Some(previous) if previous.pane_group == pane_group => previous.card,
            Some(previous) => {
                let from_y = workspace
                    .tabs
                    .iter()
                    .any(|tab| tab.pane_group.id() == previous.pane_group)
                    .then(|| {
                        app.element_position_by_id_at_last_frame(
                            workspace.window_id,
                            tab_position_id(previous.index),
                        )
                    })
                    .flatten()
                    .map(|rect| rect.origin_y());
                CardEntrance::after_switch(from_y, motion_preference(), Instant::now())
            }
            None => CardEntrance::Settled,
        };
        self.selection.set(Some(Selection {
            pane_group,
            index,
            card,
        }));
    }

    fn card(&self) -> CardEntrance {
        self.selection
            .get()
            .map_or(CardEntrance::Settled, |selection| selection.card)
    }

    /// The hover fade of a session's row, restarted whenever the pointer enters or leaves it.
    fn hover(&self, pane_group: EntityId, hovered: bool) -> Fade {
        let target = if hovered { 1. } else { 0. };
        let mut hovers = self.hovers.borrow_mut();
        let fade = hovers
            .entry(pane_group)
            .or_insert_with(|| Fade::settled(target));
        if fade.target() != target {
            let duration = if hovered { HOVER_IN } else { HOVER_OUT };
            *fade = fade.retarget(target, duration, Instant::now());
        }
        *fade
    }

    fn prune(&self, tabs: &[TabData]) {
        let mut hovers = self.hovers.borrow_mut();
        if hovers.len() > tabs.len() {
            hovers.retain(|id, _| tabs.iter().any(|tab| tab.pane_group.id() == *id));
        }
    }
}

fn capture_snapshot(tab: &TabData, app: &AppContext) -> TerminalSnapshot {
    let pane_group = tab.pane_group.as_ref(app);
    let focused = pane_group.focused_session_view(app);
    let terminal = focused.as_ref().map(|view| view.as_ref(app));
    let is_booting = terminal.is_some_and(|view| !view.is_login_shell_bootstrapped());
    let startup_directory = tab
        .sidebar_startup_directory
        .as_ref()
        .filter(|_| is_booting);
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
    let title = cli_session
        .and_then(|session| session.session_context.display_title())
        .unwrap_or_else(|| {
            // A booting session's title is still the shell-name placeholder.
            let title = pane_group.title(app);
            if is_booting || title.is_empty() || title == directory {
                "Terminal".to_owned()
            } else {
                title
            }
        });
    let activity = terminal.map(TerminalView::session_activity);
    let focused_id = terminal.map(TerminalView::id);
    let focused_status = terminal.map(|view| {
        pane_status(
            view,
            || {
                activity
                    .as_ref()
                    .is_some_and(|activity| activity.is_running_command)
            },
            app,
        )
    });
    let other_statuses = pane_group
        .terminal_views(app)
        .into_iter()
        .filter(|view| Some(view.id()) != focused_id)
        .map(|view| {
            let view = view.as_ref(app);
            pane_status(view, || view.session_activity().is_running_command, app)
        })
        .collect::<Vec<_>>();
    let unread = focused_status
        .iter()
        .chain(&other_statuses)
        .any(|pane| pane.unread);
    let status = most_urgent(focused_status.into_iter().chain(other_statuses));
    TerminalSnapshot {
        captured: Instant::now(),
        title,
        directory,
        project_lookup: startup_directory.cloned().or_else(|| {
            terminal
                .and_then(|view| view.canonical_session_pwd_if_local(app))
                .map(|path| path.as_path().to_owned())
        }),
        agent_status_unavailable: status
            .as_ref()
            .is_some_and(|pane| pane.agent_status_unavailable),
        status: status.and_then(|pane| pane.status),
        age: activity
            .and_then(|activity| activity.age_minutes)
            .map(age_label)
            .unwrap_or_default(),
        unread,
    }
}

fn session(workspace: &Workspace, index: usize, app: &AppContext) -> Session {
    let tab = &workspace.tabs[index];
    let cache = &workspace.vertical_tabs_panel.torch_sessions;
    let snapshot = cache.snapshot(tab, app);
    let title = tab
        .pane_group
        .as_ref(app)
        .custom_title(app)
        .unwrap_or(snapshot.title);
    let manual_group = tab.group_id.and_then(|id| {
        workspace
            .tab_groups
            .get(&id)
            .map(|group| (format!("manual:{id:?}"), group_display_name(group)))
    });
    let mut project = None;
    let (group_key, group_label) = manual_group.unwrap_or_else(|| {
        let id = tab.pane_group.id();
        let mut anchors = cache.anchors.borrow_mut();
        // Sessions with no directory and no project have nothing to look up in the catalog.
        let needs_catalog = snapshot.project_lookup.is_some()
            || matches!(anchors.get(&id), Some(GroupAnchor::Project(_)));
        let projects: &[crate::settings::LocalProject] = if needs_catalog {
            &crate::settings::ProjectCatalog::as_ref(app).projects
        } else {
            &[]
        };
        let find_project = |path: &Path| projects.iter().find(|project| project.path == path);
        let matched = snapshot
            .project_lookup
            .as_deref()
            .and_then(|path| crate::settings::project_for_directory(projects, path));
        let anchor = next_anchor(
            anchors.get(&id),
            matched.map(|project| project.path.as_path()),
            &snapshot.directory,
            |path| find_project(path).is_some(),
        );
        match &anchor {
            Some(anchor) => anchors.insert(id, anchor.clone()),
            None => anchors.remove(&id),
        };
        match anchor {
            Some(GroupAnchor::Project(path)) => {
                project = find_project(&path);
                (
                    format!("project:{}", path.display()),
                    project
                        .map(|project| project.name.clone())
                        .unwrap_or_default(),
                )
            }
            Some(GroupAnchor::Folder(folder)) => (folder_key(&folder), folder_label(&folder)),
            None => (folder_key(""), folder_label("")),
        }
    });
    Session {
        index,
        title,
        directory: snapshot.directory,
        group_key,
        group_label,
        project_directory: project.map(|project| project.path.clone()),
        status: snapshot.status,
        agent_status_unavailable: snapshot.agent_status_unavailable,
        age: snapshot.age,
        unread: snapshot.unread,
    }
}

/// A pane running a coding agent, as the Agents palette lists it.
#[derive(Clone)]
pub(crate) struct AgentPane {
    pub(crate) window_id: WindowId,
    pub(crate) locator: PaneViewLocator,
    terminal_view_id: EntityId,
    pub(crate) agent: &'static str,
    /// What the agent is working on, when it has said.
    pub(crate) task: Option<String>,
    /// The sidebar group the pane's session belongs to.
    pub(crate) project: String,
    branch: Option<String>,
    /// The tool a permission request is for.
    requested_tool: Option<String>,
    status_since: Option<Instant>,
    session: Session,
}

/// Where an agent stands, ordered from most to least in need of the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AgentState {
    NeedsYou,
    Finished,
    Failed,
    Working,
    Idle,
    /// The agent has not reported a status, so Torch cannot tell what it is doing.
    Running,
}

impl AgentState {
    pub(crate) fn label(self) -> &'static str {
        match self {
            AgentState::NeedsYou => "needs you",
            AgentState::Finished => "finished",
            AgentState::Failed => "failed",
            AgentState::Working => "working",
            AgentState::Idle => "idle",
            AgentState::Running => "running",
        }
    }
}

impl AgentPane {
    #[cfg(test)]
    pub(crate) fn for_test(
        status: Option<ConversationStatus>,
        requested_tool: Option<&str>,
        branch: Option<&str>,
    ) -> Self {
        Self {
            window_id: WindowId::from_usize(1),
            locator: PaneViewLocator {
                pane_group_id: EntityId::new(),
                pane_id: crate::pane_group::PaneId::dummy_pane_id(),
            },
            terminal_view_id: EntityId::new(),
            agent: "Claude Code",
            task: Some("Fix login redirect".to_owned()),
            project: "blipp-ai".to_owned(),
            branch: branch.map(str::to_owned),
            requested_tool: requested_tool.map(str::to_owned),
            status_since: None,
            session: Session {
                index: 0,
                title: String::new(),
                directory: String::new(),
                group_key: String::new(),
                group_label: String::new(),
                project_directory: None,
                status,
                agent_status_unavailable: false,
                age: String::new(),
                unread: false,
            },
        }
    }

    pub(crate) fn state(&self) -> AgentState {
        agent_state(session_indicator(
            self.session.status.as_ref(),
            self.session.unread,
            self.session.agent_status_unavailable,
        ))
    }

    /// The sidebar's indicator for this pane's state.
    pub(crate) fn render_status_icon(&self, appearance: &Appearance) -> Box<dyn Element> {
        status_icon(&self.session, &Palette::new(appearance.theme()))
    }

    /// How long the pane has been in its state, e.g. "4m".
    pub(crate) fn time_in_state(&self, now: Instant) -> Option<String> {
        self.status_since
            .map(|since| elapsed_label(now.duration_since(since)))
    }

    /// The color the sidebar uses for text about this pane's state.
    pub(crate) fn state_color(&self, appearance: &Appearance) -> ColorU {
        let palette = Palette::new(appearance.theme());
        match self.state() {
            AgentState::NeedsYou | AgentState::Failed => palette.attention,
            AgentState::Finished | AgentState::Working | AgentState::Idle | AgentState::Running => {
                palette.secondary
            }
        }
    }
}

fn elapsed_label(elapsed: Duration) -> String {
    let minutes = elapsed.as_secs() / 60;
    match minutes {
        0 => "now".to_owned(),
        1..60 => format!("{minutes}m"),
        60..1440 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1440),
    }
}

/// Agent panes in every window, this window's first, most in need of the user first.
pub(in crate::workspace::view) fn all_agent_panes(
    workspace: &Workspace,
    app: &AppContext,
) -> Vec<AgentPane> {
    let cache = &workspace.vertical_tabs_panel.torch_sessions.agents;
    if let Some((captured, panes)) = &*cache.borrow()
        && captured.elapsed() < SNAPSHOT_LIFETIME
    {
        return panes.clone();
    }
    let mut panes = agent_panes(workspace, workspace.window_id, app);
    for window_id in app.window_ids() {
        // The workspace being rendered cannot be read through its handle.
        if window_id == workspace.window_id {
            continue;
        }
        for other in app
            .views_of_type::<Workspace>(window_id)
            .unwrap_or_default()
        {
            panes.extend(agent_panes(other.as_ref(app), window_id, app));
        }
    }
    // Stable, so panes in the same state keep sidebar order.
    panes.sort_by_key(AgentPane::state);
    *cache.borrow_mut() = Some((Instant::now(), panes.clone()));
    panes
}

/// A finished agent stays "finished" until its session is seen, then it is idle.
fn agent_state(indicator: SessionIndicator) -> AgentState {
    match indicator {
        SessionIndicator::NeedsInput => AgentState::NeedsYou,
        SessionIndicator::UnreadComplete => AgentState::Finished,
        SessionIndicator::Failed => AgentState::Failed,
        SessionIndicator::Working => AgentState::Working,
        SessionIndicator::Complete | SessionIndicator::Idle => AgentState::Idle,
        SessionIndicator::Unknown => AgentState::Running,
    }
}

/// Every pane in `workspace` running a coding agent, in sidebar tab order.
fn agent_panes(workspace: &Workspace, window_id: WindowId, app: &AppContext) -> Vec<AgentPane> {
    let agents = CLIAgentSessionsModel::as_ref(app);
    let mut panes = Vec::new();
    for (index, tab) in workspace.tabs.iter().enumerate() {
        let pane_group = tab.pane_group.as_ref(app);
        let mut tab_session = None;
        for view in pane_group.terminal_views(app) {
            let view = view.as_ref(app);
            let Some(agent) = agents.session(view.id()) else {
                continue;
            };
            let Some(pane_id) = pane_group.find_pane_id_for_terminal_view(view.id(), app) else {
                continue;
            };
            let tab_session: &Session =
                tab_session.get_or_insert_with(|| session(workspace, index, app));
            // A shell command running beside the agent says nothing about the agent itself.
            let status = pane_status(view, || false, app);
            panes.push(AgentPane {
                window_id,
                locator: PaneViewLocator {
                    pane_group_id: tab.pane_group.id(),
                    pane_id,
                },
                terminal_view_id: view.id(),
                agent: agent.agent.display_name(),
                task: view.agent_task_title(&agent.session_context),
                project: tab_session.group_label.clone(),
                branch: view.current_git_branch(app),
                requested_tool: matches!(
                    agent.status,
                    CLIAgentSessionStatus::Blocked {
                        source: BlockedSource::PermissionRequest,
                        ..
                    }
                )
                .then(|| agent.session_context.tool_name.clone())
                .flatten()
                .filter(|tool| !tool.trim().is_empty()),
                status_since: agents.status_since(view.id()),
                session: Session {
                    status: status.status,
                    unread: status.unread,
                    agent_status_unavailable: status.agent_status_unavailable,
                    ..tab_session.clone()
                },
            });
        }
    }
    panes
}

fn sessions(workspace: &Workspace, app: &AppContext) -> Vec<Session> {
    workspace
        .vertical_tabs_panel
        .torch_sessions
        .prune(&workspace.tabs);
    workspace
        .vertical_tabs_panel
        .torch_rows
        .prune(&workspace.tabs);
    (0..workspace.tabs.len())
        .map(|index| session(workspace, index, app))
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
        SessionIndicator::Working => {
            let animated = motion_preference() != Some(MotionPreference::Reduced);
            return ConstrainedBox::new(WorkingPulse::new(palette.working, animated).finish())
                .with_width(16.)
                .with_height(16.)
                .finish();
        }
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

/// Whether the title opens with the status glyph Claude Code writes into the terminal title: ✳
/// at rest, and a cycling sparkle or braille spinner while it works.
fn starts_with_agent_glyph(title: &str) -> bool {
    let mut chars = title.chars();
    matches!(
        (chars.next(), chars.next()),
        (
            Some('✳' | '✢' | '✶' | '✻' | '✽' | '·' | '\u{2800}'..='\u{28FF}'),
            Some(' ')
        )
    )
}

fn session_title(
    session: &Session,
    palette: &Palette,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let weight = if session.unread {
        Weight::Semibold
    } else {
        Weight::Normal
    };
    if !starts_with_agent_glyph(&session.title) {
        return label(&session.title, 13., palette.text, weight, appearance);
    }
    Text::new_inline(session.title.clone(), appearance.ui_font_family(), 13.)
        .with_color(palette.text)
        .with_style(Properties::default().weight(weight))
        .with_clip(ClipConfig::ellipsis())
        .with_single_highlight(
            Highlight::new()
                .with_properties(Properties::default().weight(weight))
                .with_foreground_color(palette.working),
            vec![0],
        )
        .finish()
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
    age: Box<dyn Element>,
    /// The session's status, or its close button while the row is hovered.
    status_or_close: Box<dyn Element>,
}

fn row_content(parts: RowParts) -> Box<dyn Element> {
    let trailing = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(10.)
        .with_child(
            ConstrainedBox::new(Align::new(parts.age).finish())
                .with_width(28.)
                .with_height(20.)
                .finish(),
        )
        .with_child(
            ConstrainedBox::new(Align::new(parts.status_or_close).finish())
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
        age: session_age(session, palette, appearance),
        status_or_close: if departing.hovered {
            Empty::new().finish()
        } else {
            session_status(session, palette)
        },
    };
    RowSurface::new(
        row_content(parts),
        palette.row_fills(),
        Fade::settled(if departing.hovered { 1. } else { 0. }),
        departing.selected && !departing.active,
        departing.active.then_some(CardEntrance::Settled),
    )
    .finish()
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
    let transitions = &workspace.vertical_tabs_panel.torch_rows;
    let fills = palette.row_fills();
    let card = active.then(|| transitions.card());
    let row = Hoverable::new(
        workspace.tabs[index].tab_mouse_state.clone(),
        move |state| {
            let hovered = state.is_hovered();
            let hover = transitions.hover(locator.pane_group_id, hovered);
            let status_or_close = if hovered { close_button } else { status };
            let status_or_close = match hover.since() {
                Some(since) => Revealing::new(status_or_close, Reveal::Swap, since).finish(),
                None => status_or_close,
            };
            let parts = RowParts {
                title,
                age,
                status_or_close,
            };
            RowSurface::new(
                row_content(parts),
                fills,
                hover,
                in_multi_selection && !active,
                card,
            )
            .finish()
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
    name_color: ColorU,
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
                label(label_text, 12., name_color, Weight::Semibold, appearance),
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
    let name_color = match group {
        Some(group) => group.color.resolve(None),
        None => members[0]
            .project_directory
            .as_deref()
            .and_then(|directory| directory_color(directory, app)),
    }
    .map_or(palette.muted, |color| {
        color
            .to_ansi_color(&appearance.theme().terminal_colors().normal)
            .into()
    });
    let title = group_title(
        &members[0].group_label,
        members.len(),
        collapsed,
        name_color,
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
            .on_click(move |ctx, _, position| {
                ctx.dispatch_typed_action(WorkspaceAction::ToggleProjectSessionMenu {
                    directory: directory.clone(),
                    position,
                })
            })
            .finish()
        });
        let menu_directory = members[0].project_directory.clone();
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
        .on_right_click(move |ctx, _, position| {
            if let Some(directory) = &menu_directory {
                ctx.dispatch_typed_action(WorkspaceAction::ToggleProjectGroupMenu {
                    directory: directory.clone(),
                    position,
                })
            }
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
    let group_content = draggable_group(&members[0].group_key, group_content, state);
    if let Some(group) = group {
        SavePosition::new(group_content, &vtab_group_position_id(group.id)).finish()
    } else {
        group_content
    }
}

fn draggable_group(
    key: &str,
    content: Box<dyn Element>,
    state: &VerticalTabsPanelState,
) -> Box<dyn Element> {
    let draggable: DraggableState = state
        .torch_group_mouse_states
        .borrow_mut()
        .entry(key.to_owned())
        .or_default()
        .draggable
        .clone();
    let cursor = draggable.clone();
    let position_id = group_position_id(key);
    let key = key.to_owned();
    let content = Draggable::new(draggable, content)
        .with_drag_axis(DragAxis::VerticalOnly)
        // Rows drag themselves; only a press on the heading moves the whole group.
        .with_defer_to_handled_child_mouse_down()
        .on_drag(move |ctx, _, rect, _| {
            ctx.dispatch_typed_action(WorkspaceAction::DragSidebarGroup {
                key: key.clone(),
                cursor_position: cursor
                    .dragging_mouse_position()
                    .unwrap_or_else(|| rect.center()),
            })
        })
        .on_drop(|ctx, _, _, _| ctx.dispatch_typed_action(WorkspaceAction::DropGroup))
        .finish();
    SavePosition::new(content, &position_id).finish()
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
        palette.muted,
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

/// Rows the Agents panel shows before pointing to the full list.
const AGENTS_PANEL_ROWS: usize = 5;
const AGENT_ROW_HEIGHT: f32 = 42.;

/// The sidebar's Agents panel, showing every window's agents.
#[derive(Default)]
pub(in crate::workspace::view) struct AgentsPanelState {
    pub(in crate::workspace::view) collapsed: bool,
    header_mouse_state: MouseStateHandle,
    more_mouse_state: MouseStateHandle,
    row_mouse_states: RefCell<HashMap<EntityId, MouseStateHandle>>,
}

/// What a pane's agent is doing beyond its task: the tool it is asking for, or where it runs.
fn agent_context(pane: &AgentPane) -> String {
    if let Some(tool) = &pane.requested_tool {
        return format!("{} wants to use {tool}", pane.agent);
    }
    let mut context = format!("{} · {}", pane.agent, pane.project);
    if let Some(branch) = &pane.branch {
        context.push_str(&format!(" · {branch}"));
    }
    context
}

/// The header's summary of agents waiting on the user, otherwise of busy ones.
fn agents_summary(panes: &[AgentPane]) -> (String, bool) {
    let count = |state| panes.iter().filter(|pane| pane.state() == state).count();
    match (count(AgentState::NeedsYou), count(AgentState::Working)) {
        (0, 0) => (panes.len().to_string(), false),
        (0, working) => (format!("{working} working"), false),
        (needs, _) => (
            format!("{needs} need{} you", if needs == 1 { "s" } else { "" }),
            true,
        ),
    }
}

fn agent_row(
    pane: &AgentPane,
    mouse_state: MouseStateHandle,
    now: Instant,
    palette: &Palette,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let title = label(
        pane.task
            .clone()
            .unwrap_or_else(|| format!("{} session", pane.agent)),
        13.,
        palette.text,
        Weight::Normal,
        appearance,
    );
    let time = label(
        pane.time_in_state(now).unwrap_or_default(),
        12.,
        palette.secondary,
        Weight::Normal,
        appearance,
    );
    let context_color = if pane.requested_tool.is_some() {
        pane.state_color(appearance)
    } else {
        palette.secondary
    };
    let context = label(
        agent_context(pane),
        12.,
        context_color,
        Weight::Normal,
        appearance,
    );
    let status = ConstrainedBox::new(pane.render_status_icon(appearance))
        .with_width(16.)
        .with_height(16.)
        .finish();
    let contents = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(10.)
        .with_child(status)
        .with_child(
            Expanded::new(
                1.,
                Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(2.)
                    .with_child(
                        Flex::row()
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .with_spacing(8.)
                            .with_child(Expanded::new(1., title).finish())
                            .with_child(time)
                            .finish(),
                    )
                    .with_child(context)
                    .finish(),
            )
            .finish(),
        )
        .finish();
    let hover = palette.hover;
    let terminal_view_id = pane.terminal_view_id;
    Hoverable::new(mouse_state, move |state| {
        ConstrainedBox::new(
            Container::new(contents)
                .with_padding(Padding::uniform(4.).with_horizontal(12.))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
                .with_background(if state.is_hovered() {
                    ElementFill::Solid(hover)
                } else {
                    ElementFill::None
                })
                .finish(),
        )
        .with_height(AGENT_ROW_HEIGHT)
        .finish()
    })
    .with_cursor(Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(WorkspaceAction::FocusTerminalViewInWorkspace {
            terminal_view_id,
        })
    })
    .finish()
}

/// Every window's agents, most in need of the user first, pinned below the sessions. `None`
/// when no agent is running.
fn agents_panel(
    workspace: &Workspace,
    palette: &Palette,
    app: &AppContext,
) -> Option<Box<dyn Element>> {
    let panes = all_agent_panes(workspace, app);
    if panes.is_empty() {
        return None;
    }
    let appearance = Appearance::as_ref(app);
    let state = &workspace.vertical_tabs_panel.torch_agents;
    let (summary, needs_user) = agents_summary(&panes);
    let heading = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.)
        .with_child(icon(
            if state.collapsed {
                WarpIcon::ChevronRight
            } else {
                WarpIcon::ChevronDown
            },
            12.,
            palette.secondary,
        ))
        .with_child(
            Expanded::new(
                1.,
                label("Agents", 12., palette.text, Weight::Semibold, appearance),
            )
            .finish(),
        )
        .with_child(label(
            summary,
            12.,
            if needs_user {
                palette.attention
            } else {
                palette.muted
            },
            Weight::Normal,
            appearance,
        ))
        .finish();
    let header = Hoverable::new(state.header_mouse_state.clone(), move |_| {
        padded_heading(heading)
    })
    .with_cursor(Cursor::PointingHand)
    .on_click(|ctx, _, _| ctx.dispatch_typed_action(WorkspaceAction::ToggleSidebarAgentsCollapsed))
    .finish();
    let mut panel = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(header);
    if !state.collapsed {
        let now = Instant::now();
        let mut row_mouse_states = state.row_mouse_states.borrow_mut();
        row_mouse_states.retain(|id, _| panes.iter().any(|pane| pane.terminal_view_id == *id));
        for pane in panes.iter().take(AGENTS_PANEL_ROWS) {
            let mouse_state = row_mouse_states
                .entry(pane.terminal_view_id)
                .or_default()
                .clone();
            panel.add_child(agent_row(pane, mouse_state, now, palette, appearance));
        }
        if let Some(hidden) = panes
            .len()
            .checked_sub(AGENTS_PANEL_ROWS)
            .filter(|n| *n > 0)
        {
            let secondary = palette.secondary;
            let hover = palette.hover;
            let more = label(
                format!("{hidden} more…"),
                12.,
                secondary,
                Weight::Normal,
                appearance,
            );
            panel.add_child(
                Hoverable::new(state.more_mouse_state.clone(), move |state| {
                    Container::new(more)
                        .with_padding(Padding::uniform(6.).with_horizontal(12.))
                        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
                        .with_background(if state.is_hovered() {
                            ElementFill::Solid(hover)
                        } else {
                            ElementFill::None
                        })
                        .finish()
                })
                .with_cursor(Cursor::PointingHand)
                .on_click(|ctx, _, _| {
                    ctx.dispatch_typed_action(WorkspaceAction::TogglePalette {
                        mode: PaletteMode::Agents,
                        source: PaletteSource::Keybinding,
                    })
                })
                .finish(),
            );
        }
    }
    Some(
        Container::new(panel.finish())
            .with_padding_left(9.)
            .with_padding_right(9.)
            .with_padding_bottom(8.)
            .with_border(Border::top(1.).with_border_fill(palette.header_rule))
            .finish(),
    )
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
    let mut content = Flex::column()
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
        );
    if let Some(agents) = agents_panel(workspace, palette, app) {
        content.add_child(agents);
    }
    let content = content.finish();
    let panel = Resizable::new(state.resizable_state.clone(), content)
        .with_dragbar_side(DragBarSide::Right)
        .with_bounds_callback(Box::new(|size| (220., (size.x() * 0.45).max(220.))))
        .on_resize(|ctx, _| ctx.dispatch_typed_action(WorkspaceAction::SidebarResized))
        .finish();
    SavePosition::new(panel, super::super::VERTICAL_TABS_PANEL_POSITION_ID).finish()
}

pub(in crate::workspace::view) fn render(
    workspace: &Workspace,
    app: &AppContext,
) -> Box<dyn Element> {
    let appearance = Appearance::as_ref(app);
    let palette = Palette::new(appearance.theme());
    workspace
        .vertical_tabs_panel
        .torch_rows
        .select(workspace, app);
    let sessions = if workspace.vertical_tabs_panel_open {
        sessions(workspace, app)
    } else {
        Vec::new()
    };
    let active = sessions
        .get(workspace.active_tab_index)
        .cloned()
        .unwrap_or_else(|| session(workspace, workspace.active_tab_index, app));
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
                // Aligns the active title with the terminal's first column.
                .with_padding_left(if workspace.vertical_tabs_panel_open {
                    *crate::terminal::view::PADDING_LEFT
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
    let pane_radius = CornerRadius::with_all(Radius::Pixels(12.));
    let main = Container::new(Clipped::new(main).finish())
        .with_background(appearance.theme().background())
        .with_corner_radius(pane_radius)
        .with_border(Border::all(1.).with_border_fill(palette.rule))
        .with_drop_shadow(soft_shadow(palette.contact_shadow, 1., 1.5))
        .finish();
    // A second, broader shadow under the same rounded rect gives the pane soft depth.
    let main = Container::new(main)
        .with_background(appearance.theme().background())
        .with_corner_radius(pane_radius)
        .with_drop_shadow(soft_shadow(palette.shadow, 6., 10.))
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
