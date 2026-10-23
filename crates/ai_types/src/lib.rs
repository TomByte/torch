//! Pure, dependency-light AI model types shared across Torch crates.
//!
//! This crate sits below `torch_terminal` and `ai` in the dependency graph, so
//! it must not depend on any workspace crate above `torch_core`. Keep the
//! contents limited to IDs, small enums, and small serializable structs.

mod agent;
mod ambient_agent;
mod execution_context;

pub use agent::{
    AIAgentActionId, AIConversationId, EntrypointType, PassiveSuggestionTriggerType, TaskId,
};
pub use ambient_agent::{AmbientAgentTaskId, ParseAmbientAgentTaskIdError};
pub use execution_context::{WarpAiExecutionContext, WarpAiOsContext};
