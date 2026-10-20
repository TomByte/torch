//! Local session navigation, available immediately when opening Torch's palette.
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use fuzzy_match::match_indices_case_insensitive;
use ordered_float::OrderedFloat;
use warp_core::ui::color::blend::Blend;
use warp_core::ui::theme::Fill;
use warpui::elements::{Container, Flex, ParentElement, Text};
use warpui::text_layout::ClipConfig;
use warpui::{AppContext, Element, Entity, EntityId, SingletonEntity};

use crate::appearance::Appearance;
use crate::search::command_palette::mixer::CommandPaletteItemAction;
use crate::search::command_palette::render_util::render_search_item_icon_on_background;
use crate::search::data_source::{Query, QueryFilter, QueryResult};
use crate::search::item::SearchItem;
use crate::search::mixer::{DataSourceRunErrorWrapper, SyncDataSource};
use crate::search::result_renderer::ItemHighlightState;
use crate::session_management::TabNavigationData;
use crate::settings::{LocalProject, expand_project_path, project_for_directory};
use crate::ui_components::icons::Icon;

#[derive(Default)]
pub struct DataSource {
    tabs: Vec<TabNavigationData>,
    projects: Vec<LocalProject>,
    session_directories: HashMap<EntityId, PathBuf>,
}

impl DataSource {
    pub fn set_projects(&mut self, projects: Vec<LocalProject>) {
        self.projects = projects;
    }

    pub fn set_tabs(&mut self, tabs: Vec<TabNavigationData>) {
        self.session_directories = tabs
            .iter()
            .filter_map(|tab| {
                let path = expand_project_path(std::path::Path::new(tab.subtitle.as_ref()?));
                Some((tab.pane_group_id, path.canonicalize().unwrap_or(path)))
            })
            .collect();
        self.tabs = tabs;
    }
}

pub(super) fn directory_path(text: &str) -> Option<PathBuf> {
    let text = text.trim();
    let path = if text == "~" {
        dirs::home_dir()?
    } else if let Some(relative) = text.strip_prefix("~/") {
        dirs::home_dir()?.join(relative)
    } else {
        let path = PathBuf::from(text);
        if !path.is_absolute() {
            return None;
        }
        path
    };
    path.is_dir().then_some(path)
}

impl SyncDataSource for DataSource {
    type Action = CommandPaletteItemAction;

    fn run_query(
        &self,
        query: &Query,
        _: &AppContext,
    ) -> Result<Vec<QueryResult<Self::Action>>, DataSourceRunErrorWrapper> {
        Ok(self.search(query))
    }
}

impl DataSource {
    fn search(&self, query: &Query) -> Vec<QueryResult<CommandPaletteItemAction>> {
        let mut items = Vec::new();
        let allows = |filter| query.filters.is_empty() || query.filters.contains(&filter);
        if allows(QueryFilter::Sessions) {
            items.extend(self.tabs.iter().enumerate().map(|(rank, tab)| {
                Item {
                    title: format!("Switch to {}", tab.title),
                    detail: tab
                        .subtitle
                        .clone()
                        .unwrap_or_else(|| "Open session".into()),
                    icon: Icon::TerminalInput,
                    tier: 3,
                    score: 1000. - rank as f64,
                    action: CommandPaletteItemAction::NavigateToTab {
                        pane_group_id: tab.pane_group_id,
                        window_id: tab.window_id,
                    },
                }
            }));
        }
        if allows(QueryFilter::Actions) {
            items.push(Item {
                title: "New session".into(),
                detail: "Start a terminal in the current directory".into(),
                icon: Icon::TerminalInput,
                tier: 2,
                score: 2.,
                action: CommandPaletteItemAction::NewLocalSession { directory: None },
            });
            items.push(Item {
                title: "New session in directory…".into(),
                detail: "Choose a folder".into(),
                icon: Icon::Folder,
                tier: 2,
                score: 1.,
                action: CommandPaletteItemAction::ChooseLocalDirectory,
            });
        }
        if allows(QueryFilter::Repos) {
            for (rank, project) in self.projects.iter().enumerate() {
                let open = self.tabs.iter().find(|tab| {
                    self.session_directories
                        .get(&tab.pane_group_id)
                        .is_some_and(|directory| {
                            project_for_directory(&self.projects, directory) == Some(project)
                        })
                });
                let action = open
                    .map(|tab| CommandPaletteItemAction::NavigateToTab {
                        pane_group_id: tab.pane_group_id,
                        window_id: tab.window_id,
                    })
                    .unwrap_or_else(|| CommandPaletteItemAction::NewLocalSession {
                        directory: Some(project.path.clone()),
                    });
                items.push(Item {
                    title: format!(
                        "{} project {}",
                        if open.is_some() { "Resume" } else { "Open" },
                        project.name
                    ),
                    detail: project.path.display().to_string(),
                    icon: Icon::Folder,
                    tier: 3,
                    score: 2000. - rank as f64,
                    action,
                });
                items.push(Item {
                    title: format!("New session in {}", project.name),
                    detail: project.path.display().to_string(),
                    icon: Icon::TerminalInput,
                    tier: 1,
                    score: 1.,
                    action: CommandPaletteItemAction::NewLocalSession {
                        directory: Some(project.path.clone()),
                    },
                });
            }
            let mut seen: HashSet<PathBuf> = self
                .projects
                .iter()
                .map(|project| project.path.clone())
                .collect();
            let paths = self
                .tabs
                .iter()
                .filter_map(|tab| tab.subtitle.as_deref())
                .filter_map(directory_path)
                .chain(directory_path(&query.text));
            for path in paths {
                if !seen.insert(path.clone()) {
                    continue;
                }
                items.push(Item {
                    title: format!(
                        "New session in {}",
                        path.file_name()
                            .unwrap_or(path.as_os_str())
                            .to_string_lossy()
                    ),
                    detail: path.to_string_lossy().into_owned(),
                    icon: Icon::Folder,
                    tier: 1,
                    score: 0.,
                    action: CommandPaletteItemAction::NewLocalSession {
                        directory: Some(path),
                    },
                });
            }
        }
        if allows(QueryFilter::Actions) {
            items.push(Item {
                title: "Manage projects".into(),
                detail: "Add project folders in Settings".into(),
                icon: Icon::Folder,
                tier: 1,
                score: 0.,
                action: CommandPaletteItemAction::ManageLocalProjects,
            });
        }
        let typed_path = directory_path(&query.text);
        items.into_iter().filter_map(|mut item| {
            let is_typed_directory = item.title.starts_with("New session in ") && matches!(&item.action, CommandPaletteItemAction::NewLocalSession { directory: Some(path) } if Some(path) == typed_path.as_ref());
            if is_typed_directory { item.tier = 5; }
            if !query.text.trim().is_empty() && !is_typed_directory {
                let matched = match_indices_case_insensitive(&format!("{} {}", item.title, item.detail), query.text.trim())?;
                item.score = if item.title.eq_ignore_ascii_case(query.text.trim()) || self.projects.iter().any(|project| project.name.eq_ignore_ascii_case(query.text.trim()) && item.detail == project.path.display().to_string() && (item.title.starts_with("Open project ") || item.title.starts_with("Resume project "))) {
                    item.tier = 4;
                    f64::MAX
                } else { matched.score as f64 };
            }
            Some(item.into())
        }).collect()
    }
}

impl Entity for DataSource {
    type Event = ();
}

struct Item {
    title: String,
    detail: String,
    icon: Icon,
    tier: u8,
    score: f64,
    action: CommandPaletteItemAction,
}

impl SearchItem for Item {
    type Action = CommandPaletteItemAction;
    fn is_multiline(&self) -> bool {
        true
    }
    fn render_icon(&self, state: ItemHighlightState, appearance: &Appearance) -> Box<dyn Element> {
        let background = self
            .item_background(state, appearance)
            .unwrap_or_else(|| crate::themes::command_surface(appearance.theme()));
        render_search_item_icon_on_background(
            appearance,
            self.icon,
            appearance
                .theme()
                .foreground()
                .with_opacity(75)
                .into_solid(),
            background,
        )
    }
    fn item_background(&self, state: ItemHighlightState, appearance: &Appearance) -> Option<Fill> {
        let theme = appearance.theme();
        match state {
            ItemHighlightState::Selected { .. } => {
                Some(theme.background().blend(&theme.accent().with_opacity(16)))
            }
            ItemHighlightState::Hovered => Some(theme.surface_1()),
            ItemHighlightState::Default => None,
        }
    }
    fn render_item(&self, _: ItemHighlightState, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        Flex::column()
            .with_child(
                Text::new_inline(self.title.clone(), appearance.ui_font_family(), 14.)
                    .with_color(appearance.theme().foreground().into_solid())
                    .finish(),
            )
            .with_child(
                Container::new(
                    Text::new_inline(self.detail.clone(), appearance.ui_font_family(), 12.)
                        .with_clip(ClipConfig::start())
                        .with_color(
                            appearance
                                .theme()
                                .foreground()
                                .with_opacity(75)
                                .into_solid(),
                        )
                        .finish(),
                )
                .with_padding_top(3.)
                .finish(),
            )
            .finish()
    }
    fn priority_tier(&self) -> u8 {
        self.tier
    }
    fn score(&self) -> OrderedFloat<f64> {
        OrderedFloat(self.score)
    }
    fn accept_result(&self) -> Self::Action {
        self.action.clone()
    }
    fn execute_result(&self) -> Self::Action {
        self.accept_result()
    }
    fn accessibility_label(&self) -> String {
        format!("{}. {}", self.title, self.detail)
    }
}

#[cfg(test)]
#[path = "local_navigation_tests.rs"]
mod tests;
