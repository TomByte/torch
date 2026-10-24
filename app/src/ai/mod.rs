//! This module should houses all horizontal/cross-cutting AI functionality throughout
//! Torch (including Agent Mode).
//!
//! The side panel Torch AI implementation lives in `super::ai_assistant`.
pub(crate) mod active_agent_views_model;
pub(crate) mod agent;
pub(crate) mod agent_conversations_model;
pub(crate) mod agent_tips;
pub(crate) mod ai_document_view;
#[cfg(not(target_family = "wasm"))]
pub mod aws_credentials;
#[cfg(not(target_family = "wasm"))]
pub(crate) mod block_context;
pub(crate) mod blocklist;
pub mod chatgpt_subscription;
#[cfg(any(feature = "local_fs", not(target_family = "wasm")))]
pub(crate) mod codebase_auto_indexing;
pub mod control_code_parser;
#[cfg(feature = "local_fs")]
pub(crate) mod conversation_export;
pub(crate) mod conversation_navigation;
pub(crate) mod conversation_rename;
pub(crate) mod conversation_utils;
pub mod credit_availability;
pub(crate) mod custom_endpoints;
pub(crate) mod custom_model_router_editor;
pub(crate) mod custom_model_routers;
pub(crate) mod document;
#[cfg(not(target_family = "wasm"))]
pub mod geap_credentials;
pub(crate) mod get_relevant_files;
pub(crate) mod llms;
pub(crate) mod metadata_project_rules;
pub(crate) mod persisted_workspace;
pub(crate) mod predict;
#[cfg(all(not(target_family = "wasm"), feature = "local_fs"))]
pub(crate) mod remote_context_files;
pub mod request_usage_model;
pub(crate) mod restored_conversations;
pub(crate) mod skills;
pub(crate) mod stored_screenshots;
pub(crate) mod voice;
pub use agent_tips::*;
pub use credit_availability::*;
pub use request_usage_model::*;
use torchui::AppContext;
#[cfg(not(target_family = "wasm"))]
pub mod execution_profiles;
pub mod facts;
pub(crate) mod generate_block_title;
pub(crate) mod generate_code_review_content;
pub(crate) mod loading;
pub mod mcp;
pub mod outline;

pub(crate) use ai::paths;

pub fn init(app: &mut AppContext) {
    blocklist::keyboard_navigable_buttons::init(app);
    blocklist::block::number_shortcut_buttons::init(app);
    blocklist::toggleable_items::init(app);
    blocklist::suggested_agent_mode_workflow_modal::init(app);
    blocklist::suggested_rule_modal::init(app);
    ai_document_view::init(app);
}
