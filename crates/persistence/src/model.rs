//! These types are named after the database tables, and are used to represent specific queries.

use std::collections::HashMap;

use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use super::schema::{
    app, blocks, code_pane_tabs, code_panes, code_review_panes, commands, ignored_suggestions,
    notebook_panes, pane_branches, pane_leaves, pane_nodes, panels, projects, settings_panes,
    tab_groups, tabs, terminal_panes, windows, workspace_language_server, workspace_metadata,
};

#[derive(Insertable)]
#[diesel(table_name = app)]
pub struct NewApp {
    pub active_window_id: Option<i32>,
}

#[derive(Identifiable, Queryable)]
pub struct Window {
    pub id: i32,
    pub active_tab_index: i32,
    pub window_width: Option<f32>,
    pub window_height: Option<f32>,
    pub origin_x: Option<f32>,
    pub origin_y: Option<f32>,
    pub quake_mode: bool,
    pub universal_search_width: Option<f32>,
    pub warp_ai_width: Option<f32>,
    pub voltron_width: Option<f32>,
    pub warp_drive_index_width: Option<f32>,
    pub fullscreen_state: i32,
    pub agent_management_filters: Option<String>,
    pub left_panel_open: Option<bool>,
    pub vertical_tabs_panel_open: Option<bool>,
    pub team_uid: Option<String>,
    pub vertical_tabs_panel_width: Option<f32>,
}

#[derive(Clone, Identifiable, Queryable, AsChangeset)]
#[diesel(table_name = workspace_metadata)]
pub struct WorkspaceMetadata {
    pub id: i32,
    pub repo_path: String,
    pub navigated_ts: Option<NaiveDateTime>,
    pub modified_ts: Option<NaiveDateTime>,
    pub queried_ts: Option<NaiveDateTime>,
}

#[derive(Clone, Insertable, AsChangeset)]
#[diesel(table_name = workspace_metadata)]
pub struct NewWorkspaceMetadata {
    pub repo_path: String,
    pub navigated_ts: Option<NaiveDateTime>,
    pub modified_ts: Option<NaiveDateTime>,
    pub queried_ts: Option<NaiveDateTime>,
}

#[derive(Clone, Identifiable, Insertable, Queryable, AsChangeset)]
#[diesel(table_name = workspace_language_server)]
pub struct WorkspaceLanguageServer {
    pub id: i32,
    pub workspace_id: i32,
    pub language_server_name: String,
    pub enabled: String,
}

#[derive(Clone, Insertable, AsChangeset)]
#[diesel(table_name = workspace_language_server)]
pub struct NewWorkspaceLanguageServer {
    pub workspace_id: i32,
    pub language_server_name: String,
    pub enabled: String,
}

#[derive(Default, Clone, Debug, Insertable, Queryable, AsChangeset)]
#[diesel(table_name = projects)]
pub struct Project {
    pub path: String,
    pub added_ts: NaiveDateTime,
    pub last_opened_ts: Option<NaiveDateTime>,
}

impl Project {
    pub fn last_used_at(&self) -> NaiveDateTime {
        self.last_opened_ts.unwrap_or(self.added_ts)
    }
}

impl PartialEq for Project {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Eq for Project {}

#[derive(Insertable)]
#[diesel(table_name = windows)]
pub struct NewWindow {
    pub active_tab_index: i32,
    pub window_width: Option<f32>,
    pub window_height: Option<f32>,
    pub origin_x: Option<f32>,
    pub origin_y: Option<f32>,
    pub quake_mode: bool,
    pub universal_search_width: Option<f32>,
    pub warp_ai_width: Option<f32>,
    pub voltron_width: Option<f32>,
    pub warp_drive_index_width: Option<f32>,
    pub fullscreen_state: i32,
    pub left_panel_open: Option<bool>,
    pub vertical_tabs_panel_open: Option<bool>,
    pub vertical_tabs_panel_width: Option<f32>,
}

#[derive(Identifiable, Queryable, Associations)]
#[diesel(belongs_to(Window))]
pub struct Tab {
    pub id: i32,
    pub window_id: i32,
    pub custom_title: Option<String>,
    pub color: Option<String>,
    pub tab_group_id: Option<i32>,
    pub pinned: bool,
}

#[derive(Insertable)]
#[diesel(table_name = tabs)]
pub struct NewTab {
    pub window_id: i32,
    pub custom_title: Option<String>,
    pub color: Option<String>,
    pub tab_group_id: Option<i32>,
    pub pinned: bool,
}

/// Persisted form of a tab group. `name` is optional — untitled groups omit
/// it and the UI falls back to a default label.
#[derive(Identifiable, Queryable, Associations)]
#[diesel(belongs_to(Window))]
#[diesel(table_name = tab_groups)]
pub struct TabGroup {
    pub id: i32,
    pub window_id: i32,
    pub name: Option<String>,
    pub color: Option<String>,
    pub collapsed: bool,
    pub pinned: bool,
}

#[derive(Insertable)]
#[diesel(table_name = tab_groups)]
pub struct NewTabGroup {
    pub window_id: i32,
    pub name: Option<String>,
    pub color: Option<String>,
    pub collapsed: bool,
    pub pinned: bool,
}

/// The panes data model includes pane_nodes, pane_leaves and pane_branches.
/// In addition, each kind of pane has a table for its specific data (i.e. the cwd for terminal panes).
/// The pane_nodes table specifically keeps the node data so it is responsible for
/// keeping track of the tree relationships.
/// The pane_leaves table keeps info about a given pane (i.e. what kind of pane it is).
/// The pane_branches table keeps info about a branch in the tree (e.g. whether
/// the branch splits horizontally or vertically).
#[derive(Identifiable, Queryable)]
#[diesel(table_name = pane_leaves)]
#[diesel(primary_key(pane_node_id, kind))]
pub struct PaneLeaf {
    pub pane_node_id: i32,
    pub kind: String,
    pub is_focused: bool,
    pub custom_vertical_tabs_title: Option<String>,
}

#[derive(Identifiable, Queryable, Selectable)]
#[diesel(table_name = terminal_panes)]
#[diesel(primary_key(id))]
pub struct TerminalPane {
    pub id: i32,
    // This is hardcoded in the database, and not used in the app, but Diesel requires it so that
    // fields are in the same order as the table's columns.
    pub kind: String,
    pub uuid: Vec<u8>,
    pub cwd: Option<String>,
    pub is_active: bool,
    /// This is serialized JSON data for a ShellLaunchData struct.
    pub shell_launch_data: Option<String>,
    /// This is serialized JSON data for an InputConfig struct.
    pub input_config: Option<String>,
    pub llm_model_override: Option<String>,
    pub active_profile_id: Option<String>,
    /// This is serialized JSON data for a Vec<AIConversationId>.
    pub conversation_ids: Option<String>,
    /// The active conversation ID if the agent view was open in fullscreen mode.
    pub active_conversation_id: Option<String>,
    /// Serialized JSON for the CLI agent conversation running in the pane, if resumable.
    pub cli_agent_session: Option<String>,
}

#[derive(Identifiable, Queryable, Selectable)]
#[diesel(table_name = notebook_panes)]
#[diesel(primary_key(id))]
pub struct NotebookPane {
    pub id: i32,
    // This is hardcoded in the database, and not used in the app, but Diesel requires it so that
    // fields are in the same order as the table's columns.
    pub kind: String,
    pub notebook_id: Option<String>,
    pub local_path: Option<Vec<u8>>,
}

#[derive(Identifiable, Queryable, Selectable)]
#[diesel(table_name = code_panes)]
#[diesel(primary_key(id))]
pub struct CodePane {
    pub id: i32,
    pub active_tab_index: i32,
    pub source_data: Option<String>,
}

#[derive(Identifiable, Queryable, Selectable)]
#[diesel(table_name = code_pane_tabs)]
#[diesel(primary_key(id))]
pub struct CodePaneTab {
    pub id: i32,
    pub code_pane_id: i32,
    pub tab_index: i32,
    pub local_path: Option<Vec<u8>>,
}

#[derive(Identifiable, Queryable, Selectable)]
#[diesel(table_name = code_review_panes)]
#[diesel(primary_key(id))]
pub struct CodeReviewPane {
    pub id: i32,
    pub kind: String,
    pub terminal_uuid: Vec<u8>,
    pub repo_path: String,
}

#[derive(Identifiable, Queryable, Selectable)]
#[diesel(table_name = settings_panes)]
#[diesel(primary_key(id))]
pub struct SettingsPane {
    pub id: i32,
    pub kind: String,
    pub current_page: String,
}

/// Subset of the [`terminal_panes`] table needed to retrieve per-session block lists.
///
/// The true primary key of the table is [`terminal_panes::id`]. However, Diesel's associations API
/// requires matching on the primary key, so this view pretends that [`terminal_panes::uuid`] is
/// the primary key. This is safe because the UUID is _also_ unique across all panes.
#[derive(Identifiable, Selectable, Queryable)]
#[diesel(table_name = terminal_panes)]
#[diesel(primary_key(uuid))]
pub struct TerminalSession {
    pub uuid: Vec<u8>,
}

#[derive(Queryable)]
pub struct PaneBranch {
    #[allow(dead_code)]
    pub id: i32,
    #[allow(dead_code)]
    pub pane_node_id: i32,
    pub horizontal: bool,
}

#[derive(Queryable)]
pub struct PaneNode {
    pub id: i32,
    #[allow(dead_code)]
    pub tab_id: i32,
    #[allow(dead_code)]
    pub parent_pane_node_id: Option<i32>,
    pub flex: Option<f32>,
    pub is_leaf: bool,
}

#[derive(Insertable)]
#[diesel(table_name = pane_leaves)]
pub struct NewPane {
    pub pane_node_id: i32,
    pub kind: String,
    pub is_focused: bool,
    pub custom_vertical_tabs_title: Option<String>,
}

/// The [`pane_leaves::kind`] value for terminal panes.
pub const TERMINAL_PANE_KIND: &str = "terminal";

/// The [`pane_leaves::kind`] value for notebook panes.
pub const NOTEBOOK_PANE_KIND: &str = "notebook";

/// The [`pane_leaves::kind`] value for EVC panes.
pub const ENV_VAR_COLLECTION_PANE_KIND: &str = "env_var_collection";

/// The [`pane_leaves::kind`] value for code panes.
pub const CODE_PANE_KIND: &str = "code";

/// The [`pane_leaves::kind`] value for workflow panes.
pub const WORKFLOW_PANE_KIND: &str = "workflow";

/// The [`pane_leaves::kind`] value for settings panes.
pub const SETTINGS_PANE_KIND: &str = "settings";

/// The [`pane_leaves::kind`] value for AI fact panes
/// (where kind name is historical and not worth a migration to change).
pub const AI_FACT_PANE_KIND: &str = "ai_memory";

/// The [`pane_leaves::kind`] value for MCP server panes
pub const MCP_SERVER_PANE_KIND: &str = "mcp_server";

/// The [`pane_leaves::kind`] value for code review panes.
pub const CODE_REVIEW_PANE_KIND: &str = "code_review";

/// The [`pane_leaves::kind`] value for execution profile editor panes.
pub const EXECUTION_PROFILE_EDITOR_PANE_KIND: &str = "execution_profile_editor";

/// The [`pane_leaves::kind`] value for the get-started pane.
pub const GET_STARTED_PANE_KIND: &str = "get_started";

/// The [`pane_leaves::kind`] value for AI document panes.
pub const AI_DOCUMENT_PANE_KIND: &str = "ai_document";

/// The [`pane_leaves::kind`] value for ambient agent (cloud mode) panes.
pub const AMBIENT_AGENT_PANE_KIND: &str = "ambient_agent";

#[derive(Insertable)]
#[diesel(table_name = terminal_panes)]
pub struct NewTerminalPane {
    pub id: i32,
    pub uuid: Vec<u8>,
    pub cwd: Option<String>,
    pub is_active: bool,
    /// This is serialized JSON data for a ShellLaunchData struct.
    pub shell_launch_data: Option<String>,
    /// This is serialized JSON data for an InputConfig struct.
    pub input_config: Option<String>,
    pub llm_model_override: Option<String>,
    pub active_profile_id: Option<String>,
    /// This is serialized JSON data for a Vec<AIConversationId>.
    pub conversation_ids: Option<String>,
    /// The active conversation ID if the agent view was open in fullscreen mode.
    pub active_conversation_id: Option<String>,
    /// Serialized JSON for the CLI agent conversation running in the pane, if resumable.
    pub cli_agent_session: Option<String>,
}

#[derive(Insertable)]
#[diesel(table_name = notebook_panes)]
pub struct NewNotebookPane {
    pub id: i32,
    pub local_path: Option<Vec<u8>>,
}

#[derive(Insertable)]
#[diesel(table_name = code_panes)]
pub struct NewCodePane {
    pub id: i32,
    pub active_tab_index: i32,
    pub source_data: Option<String>,
}

#[derive(Insertable)]
#[diesel(table_name = code_pane_tabs)]
pub struct NewCodePaneTab {
    pub code_pane_id: i32,
    pub tab_index: i32,
    pub local_path: Option<Vec<u8>>,
}

#[derive(Insertable)]
#[diesel(table_name = code_review_panes)]
pub struct NewCodeReviewPane {
    pub id: i32,
    pub terminal_uuid: Vec<u8>,
    pub repo_path: String,
}

#[derive(Insertable)]
#[diesel(table_name = settings_panes)]
pub struct NewSettingsPane {
    pub id: i32,
    pub current_page: String,
}

#[derive(Insertable)]
#[diesel(table_name = pane_branches)]
pub struct NewPaneBranch {
    pub pane_node_id: i32,
    pub horizontal: bool,
}

#[derive(Insertable)]
#[diesel(table_name = pane_nodes)]
pub struct NewPaneNode {
    pub tab_id: i32,
    pub parent_pane_node_id: Option<i32>,
    pub flex: Option<f32>,
    pub is_leaf: bool,
}

#[derive(Insertable)]
#[diesel(table_name = blocks)]
pub struct NewBlock<'a> {
    pub block_id: &'a str,
    // Note that there is no pane leaf UUID foreign key relationship because there's no good way to
    // enforce it: when we remove a pane and subsequently create a new snapshot, the old blocks
    // will now violate the constraint. While sqlite does have deferred constraints, it doesn't
    // work well with ON DELETE CASCADE (i.e. the cascade happens on the delete, not after the
    // transaction commit).
    pub pane_leaf_uuid: Vec<u8>,
    pub stylized_command: &'a Vec<u8>,
    pub stylized_output: &'a Vec<u8>,
    pub pwd: Option<&'a String>,
    pub git_branch: Option<&'a String>,
    pub git_branch_name: Option<&'a String>,
    pub virtual_env: Option<&'a String>,
    pub conda_env: Option<&'a String>,
    pub exit_code: i32,
    pub did_execute: bool,
    pub is_background: bool,
    pub completed_ts: Option<NaiveDateTime>,
    pub start_ts: Option<NaiveDateTime>,
    pub ps1: Option<&'a String>,
    pub rprompt: Option<&'a String>,
    pub honor_ps1: bool,
    pub shell: Option<&'a str>,
    pub user: Option<&'a str>,
    pub host: Option<&'a str>,
    pub prompt_snapshot: Option<&'a String>,
    pub ai_metadata: Option<&'a String>,
    pub is_local: Option<bool>,
    pub agent_view_visibility: Option<String>,
}

#[derive(Identifiable, Queryable, Selectable, Associations)]
#[diesel(table_name = blocks)]
#[diesel(belongs_to(TerminalSession, foreign_key = pane_leaf_uuid))]
pub struct Block {
    pub id: Option<i32>,
    pub pane_leaf_uuid: Vec<u8>,
    pub stylized_command: Vec<u8>,
    pub stylized_output: Vec<u8>,
    pub pwd: Option<String>,
    pub git_branch: Option<String>,
    pub git_branch_name: Option<String>,
    pub virtual_env: Option<String>,
    pub conda_env: Option<String>,
    pub exit_code: i32,
    pub did_execute: bool,
    pub completed_ts: Option<NaiveDateTime>,
    pub start_ts: Option<NaiveDateTime>,
    pub ps1: Option<String>,
    pub honor_ps1: bool,
    pub shell: Option<String>,
    pub user: Option<String>,
    pub host: Option<String>,
    pub is_background: bool,
    pub rprompt: Option<String>,
    /// JSON-serialized representation of the Torch prompt snapshot (Context Chips). Note that this
    /// is different from PS1 and RPROMPT1
    pub prompt_snapshot: Option<String>,
    pub block_id: String,
    pub ai_metadata: Option<String>,
    pub is_local: Option<bool>,
    pub agent_view_visibility: Option<String>,
}

#[derive(Insertable)]
#[diesel(table_name = commands)]
pub struct NewCommand {
    pub command: String,
    pub exit_code: Option<i32>,
    pub start_ts: Option<NaiveDateTime>,
    pub completed_ts: Option<NaiveDateTime>,
    pub pwd: Option<String>,
    pub shell: Option<String>,
    pub username: Option<String>,
    pub hostname: Option<String>,
    pub session_id: Option<i64>,
    pub git_branch: Option<String>,
    pub workflow_command: Option<String>,
    pub is_agent_executed: Option<bool>,
}

#[derive(Identifiable, Queryable, Default, Clone)]
#[diesel(table_name = commands)]
pub struct Command {
    pub id: i32,
    pub command: String,
    pub exit_code: Option<i32>,
    pub start_ts: Option<NaiveDateTime>,
    pub completed_ts: Option<NaiveDateTime>,
    pub pwd: Option<String>,
    pub shell: Option<String>,
    pub username: Option<String>,
    pub hostname: Option<String>,
    pub session_id: Option<i64>,
    pub git_branch: Option<String>,
    pub cloud_workflow_id: Option<String>,
    pub workflow_command: Option<String>,
    pub is_agent_executed: Option<bool>,
}

pub type TokenUsageCategory = String;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ModelTokenUsage {
    /// Identifier used for both display and replay. For warp/byok rows this is the
    /// server-known model id; for custom endpoint rows this is the resolved alias
    /// (or fallback label) — the upstream `config_key` is translated into this
    /// label once at ingestion time and is not retained separately.
    pub model_id: String,
    /// Alias for backward compat: old persisted data used `total_tokens` for warp usage.
    #[serde(default, alias = "total_tokens")]
    pub warp_tokens: u32,
    #[serde(default)]
    pub byok_tokens: u32,
    #[serde(default)]
    pub custom_endpoint_tokens: u32,
    #[serde(default)]
    pub warp_token_usage_by_category: HashMap<TokenUsageCategory, u32>,
    #[serde(default)]
    pub byok_token_usage_by_category: HashMap<TokenUsageCategory, u32>,
    #[serde(default)]
    pub custom_endpoint_token_usage_by_category: HashMap<TokenUsageCategory, u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ToolCallStats {
    pub count: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct RunCommandStats {
    pub count: i32,
    pub commands_executed: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ApplyFileDiffStats {
    pub count: i32,
    pub lines_added: i32,
    pub lines_removed: i32,
    pub files_changed: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ToolUsageMetadata {
    pub run_command_stats: RunCommandStats,
    pub read_files_stats: ToolCallStats,
    pub search_codebase_stats: ToolCallStats,
    pub grep_stats: ToolCallStats,
    pub file_glob_stats: ToolCallStats,
    pub apply_file_diff_stats: ApplyFileDiffStats,
    pub write_to_long_running_shell_command_stats: ToolCallStats,
    pub read_mcp_resource_stats: ToolCallStats,
    pub call_mcp_tool_stats: ToolCallStats,
    pub suggest_plan_stats: ToolCallStats,
    pub suggest_create_plan_stats: ToolCallStats,
    pub read_shell_command_output_stats: ToolCallStats,
    pub use_computer_stats: ToolCallStats,
}

impl ToolUsageMetadata {
    pub fn total_tool_calls(&self) -> i32 {
        self.run_command_stats.count
            + self.read_files_stats.count
            + self.search_codebase_stats.count
            + self.grep_stats.count
            + self.file_glob_stats.count
            + self.write_to_long_running_shell_command_stats.count
            + self.read_mcp_resource_stats.count
            + self.call_mcp_tool_stats.count
            + self.suggest_plan_stats.count
            + self.suggest_create_plan_stats.count
            + self.apply_file_diff_stats.count
            + self.read_shell_command_output_stats.count
            + self.use_computer_stats.count
    }
}

/// The kind of a context-window segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContextWindowSegmentType {
    #[default]
    Unknown,
    SystemPrompt,
    ToolDefinitions,
    ConversationHistory,
    LatestInput,
    Images,
    Other,
}

impl ContextWindowSegmentType {
    /// Snake-case identifier used for display-name lookup.
    pub fn as_str(&self) -> &'static str {
        match self {
            ContextWindowSegmentType::Unknown => "unknown",
            ContextWindowSegmentType::SystemPrompt => "system_prompt",
            ContextWindowSegmentType::ToolDefinitions => "tool_definitions",
            ContextWindowSegmentType::ConversationHistory => "conversation_history",
            ContextWindowSegmentType::LatestInput => "latest_input",
            ContextWindowSegmentType::Images => "images",
            ContextWindowSegmentType::Other => "other",
        }
    }
}

/// A single portion of the context window, described by its kind and an
/// estimated token count. Segment token counts add up to the token total
/// represented by `context_window_usage`.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ContextWindowSegment {
    pub segment_type: ContextWindowSegmentType,
    /// Estimated number of tokens this segment occupies in the context window.
    pub token_count: u32,
}

/// A flat breakdown of charged usage — input/output/cache-read/cache-write
/// inference cost (in US cents) plus platform cost, and the matching token
/// counts — summed across every usage category and model.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default, PartialEq)]
pub struct ChargedUsageTotals {
    pub input_cost_in_cents: f32,
    #[serde(default)]
    pub input_cost_in_credits: f32,
    pub output_cost_in_cents: f32,
    #[serde(default)]
    pub output_cost_in_credits: f32,
    pub input_cache_read_cost_in_cents: f32,
    #[serde(default)]
    pub input_cache_read_cost_in_credits: f32,
    pub input_cache_write_cost_in_cents: f32,
    #[serde(default)]
    pub input_cache_write_cost_in_credits: f32,
    pub platform_cost_in_cents: f32,
    #[serde(default)]
    pub platform_cost_in_credits: f32,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub input_cache_read_tokens: u32,
    pub input_cache_write_tokens: u32,
    /// Number of web searches performed, summed across every usage category
    /// and model. `#[serde(default)]` so blobs persisted before this field
    /// existed still deserialize.
    #[serde(default)]
    pub web_search_count: u32,
    /// Cumulative cost of web searches performed, in US cents. Included in
    /// [`Self::total_cost_in_cents`] since it's part of the real, actually-
    /// charged dollar total.
    #[serde(default)]
    pub web_search_cost_in_cents: f32,
    #[serde(default)]
    pub web_search_cost_in_credits: f32,
}

impl ChargedUsageTotals {
    /// Total inference + platform + web-search cost, in US cents.
    pub fn total_cost_in_cents(&self) -> f32 {
        self.input_cost_in_cents
            + self.output_cost_in_cents
            + self.input_cache_read_cost_in_cents
            + self.input_cache_write_cost_in_cents
            + self.platform_cost_in_cents
            + self.web_search_cost_in_cents
    }

    pub fn total_cost_in_credits(&self) -> f32 {
        self.input_cost_in_credits
            + self.output_cost_in_credits
            + self.input_cache_read_cost_in_credits
            + self.input_cache_write_cost_in_credits
            + self.platform_cost_in_credits
            + self.web_search_cost_in_credits
    }

    /// Total tokens across every category (input + output + cache-read + cache-write).
    pub fn total_tokens(&self) -> u32 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.input_cache_read_tokens)
            .saturating_add(self.input_cache_write_tokens)
    }
}

impl std::ops::AddAssign for ChargedUsageTotals {
    fn add_assign(&mut self, rhs: Self) {
        self.input_cost_in_cents += rhs.input_cost_in_cents;
        self.input_cost_in_credits += rhs.input_cost_in_credits;
        self.output_cost_in_cents += rhs.output_cost_in_cents;
        self.output_cost_in_credits += rhs.output_cost_in_credits;
        self.input_cache_read_cost_in_cents += rhs.input_cache_read_cost_in_cents;
        self.input_cache_read_cost_in_credits += rhs.input_cache_read_cost_in_credits;
        self.input_cache_write_cost_in_cents += rhs.input_cache_write_cost_in_cents;
        self.input_cache_write_cost_in_credits += rhs.input_cache_write_cost_in_credits;
        self.platform_cost_in_cents += rhs.platform_cost_in_cents;
        self.platform_cost_in_credits += rhs.platform_cost_in_credits;
        self.input_tokens = self.input_tokens.saturating_add(rhs.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(rhs.output_tokens);
        self.input_cache_read_tokens = self
            .input_cache_read_tokens
            .saturating_add(rhs.input_cache_read_tokens);
        self.input_cache_write_tokens = self
            .input_cache_write_tokens
            .saturating_add(rhs.input_cache_write_tokens);
        self.web_search_count += rhs.web_search_count;
        self.web_search_cost_in_cents += rhs.web_search_cost_in_cents;
        self.web_search_cost_in_credits += rhs.web_search_cost_in_credits;
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ConversationUsageMetadata {
    pub was_summarized: bool,
    pub context_window_usage: f32,
    pub credits_spent: f32,
    #[serde(default)]
    pub platform_credits_spent: f32,
    /// Server-authoritative cumulative cost billed to the customer in US cents (inference at
    /// the customer's own price plus platform cost), for conversations whose charges arrived
    /// via GraphQL rather than the stream. `None` when the server could not establish a
    /// complete billed total; it must not be treated as numeric zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_billed_cost_in_cents: Option<f32>,
    #[serde(default)]
    pub credits_spent_for_last_block: Option<f32>,
    /// Per-category charged-usage breakdown for the most recent block (all
    /// agent outputs since the last user input), summed via
    /// [`ChargedUsageTotals::from`] from `StreamFinished.request_charges`.
    /// `None` when the server didn't provide charges (flag off) or before
    /// any block has completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charged_usage_for_last_block: Option<ChargedUsageTotals>,
    /// Cumulative per-category charged-usage breakdown summed across the
    /// whole conversation so far, from
    /// `ConversationUsageMetadata.total_charges`. `None` when the server
    /// didn't provide it (flag off, or a legacy conversation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_charged_usage: Option<ChargedUsageTotals>,
    #[serde(default)]
    pub token_usage: Vec<ModelTokenUsage>,
    #[serde(default)]
    pub tool_usage_metadata: ToolUsageMetadata,
    #[serde(default)]
    pub context_window_segments: Vec<ContextWindowSegment>,
}

impl ConversationUsageMetadata {
    pub fn total_tool_calls(&self) -> i32 {
        self.tool_usage_metadata.total_tool_calls()
    }

    /// Cumulative cost billed to the customer so far in the conversation, in US cents.
    /// Prefers the summed per-turn charges, which carry billed cents on the wire, and falls
    /// back to the GraphQL total for conversations restored without them. `None` when neither
    /// is known.
    pub fn billed_cost_in_cents(&self) -> Option<f32> {
        self.total_charged_usage
            .map(|charged| charged.total_cost_in_cents())
            .or(self.total_billed_cost_in_cents)
    }
}

#[derive(Debug, Insertable)]
#[diesel(table_name = ignored_suggestions)]
pub struct NewIgnoredSuggestion {
    pub suggestion: String,
    pub suggestion_type: String,
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;

#[derive(Insertable)]
#[diesel(table_name = panels)]
pub struct NewPanel {
    pub tab_id: i32,
    pub left_panel: Option<String>,
    pub right_panel: Option<String>,
}

#[derive(Identifiable, Queryable, Selectable)]
#[diesel(table_name = panels)]
pub struct Panel {
    pub id: i32,
    pub tab_id: i32,
    pub left_panel: Option<String>,
    pub right_panel: Option<String>,
}
