use crate::search::mixer::SearchMixer;
use crate::terminal::history::LinkedWorkflowData;
use crate::workflows::{WorkflowSource, WorkflowType};

pub type CommandSearchMixer = SearchMixer<CommandSearchItemAction>;

#[derive(Clone, Debug)]
pub struct AcceptedHistoryItem {
    pub command: String,

    /// The workflow used to construct the command, if any.
    pub linked_workflow_data: Option<LinkedWorkflowData>,
}

/// Payload for `AcceptWorkflow`: the workflow that was selected.
#[derive(Clone, Debug)]
pub struct AcceptedWorkflow {
    pub workflow: Box<WorkflowType>,
    pub source: WorkflowSource,
}

/// The set of events that may be produced by accepting or executing a search
/// result.
#[derive(Clone, Debug)]
pub enum CommandSearchItemAction {
    /// The user accepted a history search item. The contained string is the
    /// command they accepted.
    AcceptHistory(AcceptedHistoryItem),

    /// The user requested the re-execution of a history search item. The
    /// contained string is the command they accepted.
    ExecuteHistory(String),

    /// The user accepted a workflow search item.
    AcceptWorkflow(AcceptedWorkflow),

    /// The user accepted the AI query search item with this query text.
    AcceptAIQuery(String),

    /// The user requested to run the AI query search item with this query text.
    RunAIQuery(String),

    /// The user accepted the search item to open Torch AI.
    OpenWarpAI,

    /// The user accepted the search item to translate the query to a command using Torch AI.
    TranslateUsingWarpAI,
}

#[cfg(test)]
#[path = "searcher_tests.rs"]
mod tests;
