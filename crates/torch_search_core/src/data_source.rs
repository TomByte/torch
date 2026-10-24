use std::any::Any;
use std::collections::HashSet;
use std::sync::Arc;

use enum_iterator::{Sequence, all};
use lazy_static::lazy_static;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use serde_json::json;
use torch_core::ui::appearance::Appearance;
use torch_core::ui::theme::Fill;
use torchui_core::{Action, AppContext, Element, Entity, ModelHandle};

use super::item::SearchItem;
use super::mixer::{AsyncDataSource, BoxFuture, DataSourceRunErrorWrapper};
use crate::item::IconLocation;
use crate::mixer::{DataSourceRunError, SyncDataSource};
use crate::result_renderer::ItemHighlightState;

lazy_static! {
    static ref HISTORY_FILTER_ATOM: FilterAtom = FilterAtom {
        primary_text: "history:",
        aliases: vec!["h:"]
    };
    static ref WORKFLOWS_FILTER_ATOM: FilterAtom = FilterAtom {
        primary_text: "workflows:",
        aliases: vec!["w:"]
    };
    static ref NOTEBOOKS_FILTER_ATOM: FilterAtom = FilterAtom {
        primary_text: "notebooks:",
        aliases: vec!["n:"]
    };
    static ref ACTIONS_FILTER_ATOM: FilterAtom = FilterAtom {
        primary_text: "actions:",
        aliases: vec![]
    };
    static ref SESSIONS_FILTER_ATOM: FilterAtom = FilterAtom {
        primary_text: "sessions:",
        aliases: vec![]
    };
    static ref AGENTS_FILTER_ATOM: FilterAtom = FilterAtom {
        primary_text: "agents:",
        aliases: vec![]
    };
    static ref LAUNCH_CONFIG_FILTER_ATOM: FilterAtom = FilterAtom {
        primary_text: "launch_configs:",
        aliases: vec![]
    };
    static ref ENV_VARS_FILTER_ATOM: FilterAtom = FilterAtom {
        primary_text: "env_vars:",
        aliases: vec![]
    };
    static ref FILES_FILTER_ATOM: FilterAtom = FilterAtom {
        primary_text: "files:",
        aliases: vec![]
    };
    static ref REPOS_FILTER_ATOM: FilterAtom = FilterAtom {
        primary_text: "repos:",
        aliases: vec![]
    };

    // If a query filter does not have a filter atom, it cannot be applied by typing
    static ref NO_FILTER_ATOM: FilterAtom = FilterAtom {
        primary_text: "",
        aliases: vec![]
    };
}

/// Represents a 'filter atom' that may be typed out in the search input to apply a filter.
pub struct FilterAtom {
    /// The 'canonical' text representing the atom. This text is used for
    /// autosuggestions/tab-completion to apply the filter. For example, this is 'history:' for the
    /// history filter.
    pub primary_text: &'static str,

    /// Alternative strings that may be typed out in the search input to apply the filter. For
    /// example, this is ['h:'] for the history filter.
    pub aliases: Vec<&'static str>,
}

impl FilterAtom {
    /// Returns the atom string that matches the given `query`, if any.
    pub fn query_match(&self, query: &str) -> Option<&str> {
        // If primary_text is empty, this is NO_ATOM, which never matches
        if self.primary_text.is_empty() {
            return None;
        }

        if query.starts_with(self.primary_text) {
            Some(self.primary_text)
        } else {
            self.aliases
                .iter()
                .find(|alias| query.starts_with(**alias))
                .copied()
        }
    }
}

/// Filters that may be included as part of the universal search query.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, Serialize, Sequence)]
pub enum QueryFilter {
    /// Only include results from HistoryDataSource.
    History,

    /// Only include command workflows from WorkflowsDataSource.
    Workflows,

    /// Only include results from NotebooksDataSource.
    Notebooks,

    /// Filter results for command palette actions.
    Actions,

    /// Filter results for open sessions.
    Sessions,

    /// Filter results for open tabs.
    Tabs,

    /// Filter results for sessions running a coding agent.
    Agents,

    /// Filter results for launch configurations.
    LaunchConfigurations,

    /// Filter results for environment variables.
    EnvironmentVariables,

    /// Filter results for files.
    Files,

    /// Filter results for known/indexed code repos.
    Repos,
}

impl QueryFilter {
    /// Returns all possible `QueryFilter`s. Note all filters may not be enabled for a given
    /// instance of a `SearchMixer`.
    pub fn all() -> impl Iterator<Item = QueryFilter> {
        all::<Self>()
    }

    /// Returns placeholder text to be shown in an empty input when the filter is active.
    pub fn placeholder_text(&self) -> &'static str {
        match self {
            QueryFilter::History => "Search history",
            QueryFilter::Workflows => "Search workflows",
            QueryFilter::Notebooks => "Search notebooks",
            QueryFilter::Actions => "Search actions",
            QueryFilter::Sessions => "Search sessions",
            QueryFilter::Tabs => "Search tabs",
            QueryFilter::Agents => "Search agents, projects, or states like working or idle",
            QueryFilter::LaunchConfigurations => "Search launch configurations",
            QueryFilter::EnvironmentVariables => "Search environment variables",
            QueryFilter::Files => "Search files",
            QueryFilter::Repos => "Search code repos",
        }
    }

    /// Returns text that is used to represent the filter as a filter 'atom' in the search input.
    pub fn filter_atom(&self) -> &'static FilterAtom {
        match self {
            QueryFilter::History => &HISTORY_FILTER_ATOM,
            QueryFilter::Workflows => &WORKFLOWS_FILTER_ATOM,
            QueryFilter::Notebooks => &NOTEBOOKS_FILTER_ATOM,
            QueryFilter::Actions => &ACTIONS_FILTER_ATOM,
            QueryFilter::Sessions => &SESSIONS_FILTER_ATOM,
            QueryFilter::Tabs => &NO_FILTER_ATOM,
            QueryFilter::Agents => &AGENTS_FILTER_ATOM,
            QueryFilter::LaunchConfigurations => &LAUNCH_CONFIG_FILTER_ATOM,
            QueryFilter::EnvironmentVariables => &ENV_VARS_FILTER_ATOM,
            QueryFilter::Files => &FILES_FILTER_ATOM,
            QueryFilter::Repos => &REPOS_FILTER_ATOM,
        }
    }

    /// Returns the display name (e.g. the string to be used in UI) representing the filter.
    pub fn display_name(&self) -> &'static str {
        match self {
            QueryFilter::History => "history",
            QueryFilter::Workflows => "workflows",
            QueryFilter::Notebooks => "notebooks",
            QueryFilter::Actions => "actions",
            QueryFilter::Sessions => "sessions",
            QueryFilter::Tabs => "tabs",
            QueryFilter::Agents => "agents",
            QueryFilter::LaunchConfigurations => "launch configurations",
            QueryFilter::EnvironmentVariables => "environment variables",
            QueryFilter::Files => "files",
            QueryFilter::Repos => "repos",
        }
    }

    /// Returns the path to the canonical icon for the filter.
    pub fn icon_svg_path(&self) -> Option<&'static str> {
        match self {
            QueryFilter::History => Some("bundled/svg/history.svg"),
            QueryFilter::Workflows => Some("bundled/svg/workflow.svg"),
            QueryFilter::Notebooks => Some("bundled/svg/notebook.svg"),
            QueryFilter::Actions => None,
            QueryFilter::Sessions => Some("bundled/svg/terminal-input.svg"),
            QueryFilter::Tabs => Some("bundled/svg/terminal-input.svg"),
            QueryFilter::Agents => Some("bundled/svg/terminal-input.svg"),
            QueryFilter::LaunchConfigurations => Some("bundled/svg/navigation.svg"),
            QueryFilter::EnvironmentVariables => Some("bundled/svg/env-var-collection.svg"),
            QueryFilter::Files => Some("bundled/svg/completion-file.svg"),
            QueryFilter::Repos => Some("bundled/svg/folder.svg"),
        }
    }
}

/// A structure representing a query that can be executed against a data source.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    pub filters: HashSet<QueryFilter>,
    pub text: String,
}

/// Allow anything that can be converted into a &str to be converted into a
/// Query.
impl<T> From<T> for Query
where
    T: AsRef<str>,
{
    fn from(s: T) -> Self {
        Self {
            filters: Default::default(),
            text: s.as_ref().trim().to_owned(),
        }
    }
}

/// The type of a query result.
#[derive(Clone)]
pub struct QueryResult<T: Action + Clone> {
    item: Arc<dyn SearchItem<Action = T>>,
    /// Tiebreaker for sorting (results from earlier-registered data sources get a lower value
    /// so they appear first among equal-scored results)
    pub(crate) source_order: usize,
}

impl<T: Action + Clone> QueryResult<T> {
    pub fn is_multiline(&self) -> bool {
        self.item.is_multiline()
    }

    pub fn render_icon(
        &self,
        highlight_state: ItemHighlightState,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        self.item.render_icon(highlight_state, appearance)
    }

    pub fn icon_location(&self, appearance: &Appearance) -> IconLocation {
        self.item.icon_location(appearance)
    }

    pub fn render_item(
        &self,
        highlight_state: ItemHighlightState,
        app: &AppContext,
    ) -> Box<dyn Element> {
        self.item.render_item(highlight_state, app)
    }

    pub fn item_background(
        &self,
        highlight_state: ItemHighlightState,
        appearance: &Appearance,
    ) -> Option<Fill> {
        self.item.item_background(highlight_state, appearance)
    }

    pub fn render_details(&self, ctx: &AppContext) -> Option<Box<dyn Element>> {
        self.item.render_details(ctx)
    }

    pub fn priority_tier(&self) -> u8 {
        self.item.priority_tier()
    }

    pub fn score(&self) -> OrderedFloat<f64> {
        self.item.score()
    }

    pub fn accept_result(&self) -> T {
        self.item.accept_result()
    }

    pub fn execute_result(&self) -> T {
        self.item.execute_result()
    }

    pub fn accessibility_label(&self) -> String {
        self.item.accessibility_label()
    }

    pub fn accessibility_help_message(&self) -> Option<String> {
        self.item.accessibility_help_message()
    }

    pub fn detail_data(&self) -> Option<crate::item::SearchItemDetail> {
        self.item.detail_data()
    }

    /// Returns whether this item is a static separator,
    /// meaning it is a non-interactible item that should act as a simple UI element.
    pub fn is_static_separator(&self) -> bool {
        self.item.is_static_separator()
    }

    /// Returns whether this item is disabled.
    /// Disabled items cannot be accepted or selected.
    pub fn is_disabled(&self) -> bool {
        self.item.is_disabled()
    }

    /// Returns an optional tooltip string to display when hovering over this item.
    pub fn tooltip(&self) -> Option<String> {
        self.item.tooltip()
    }
}

impl<G: Action + Clone, T: SearchItem<Action = G> + 'static> From<T> for QueryResult<G> {
    fn from(value: T) -> Self {
        Self {
            item: Arc::new(value),
            source_order: usize::MAX,
        }
    }
}

/// Blanket impl of [`SyncDataSource`] for any [`ModelHandle`] of a type that also implements
/// `SyncDataSource`.
impl<T> SyncDataSource for ModelHandle<T>
where
    T: SyncDataSource + Entity,
{
    type Action = T::Action;

    fn run_query(
        &self,
        query: &Query,
        app: &AppContext,
    ) -> Result<Vec<QueryResult<Self::Action>>, DataSourceRunErrorWrapper> {
        self.as_ref(app).run_query(query, app)
    }
}

/// Blanket impl of [`AsyncDataSource`] for any [`ModelHandle`] of a type that also implements
/// `AsyncDataSource`.
impl<T> AsyncDataSource for ModelHandle<T>
where
    T: AsyncDataSource + Entity,
{
    type Action = T::Action;

    fn run_query(
        &self,
        query: &Query,
        app: &AppContext,
    ) -> BoxFuture<'static, Result<Vec<QueryResult<Self::Action>>, DataSourceRunErrorWrapper>> {
        self.as_ref(app).run_query(query, app)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataSourceSearchError {
    pub(crate) message: String,
}

impl DataSourceSearchError {
    pub fn new(message: String) -> Self {
        Self { message }
    }
}

impl DataSourceRunError for DataSourceSearchError {
    fn user_facing_error(&self) -> String {
        self.message.clone()
    }

    fn telemetry_payload(&self) -> serde_json::Value {
        json!(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
