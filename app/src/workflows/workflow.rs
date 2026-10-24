use serde::{Deserialize, Serialize};

/// Workflow model used by Torch.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum Workflow {
    AgentMode {
        name: String,
        /// The query to be inserted in the terminal input.
        query: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default)]
        arguments: Vec<Argument>,
    },
    #[serde(untagged)]
    Command {
        name: String,
        command: String,
        #[serde(default)]
        tags: Vec<String>,
        description: Option<String>,
        #[serde(default)]
        arguments: Vec<Argument>,
        source_url: Option<String>,
        author: Option<String>,
        author_url: Option<String>,
        #[serde(default)]
        shells: Vec<warp_workflows::Shell>,
    },
}

impl Workflow {
    pub fn name(&self) -> &str {
        match self {
            Self::AgentMode { name, .. } => name.as_str(),
            Self::Command { name, .. } => name.as_str(),
        }
    }

    /// The core "content" of the workflow.
    ///
    /// For Command workflows, this is the shell command. For Agent Mode workflows, this is the
    /// query.
    pub fn content(&self) -> &str {
        match self {
            Self::AgentMode { query, .. } => query,
            Self::Command { command, .. } => command,
        }
    }

    pub fn prompt(&self) -> Option<&str> {
        if let Self::AgentMode { query, .. } = self {
            Some(query.as_str())
        } else {
            None
        }
    }

    pub fn command(&self) -> Option<&str> {
        if let Self::Command { command, .. } = self {
            Some(command.as_str())
        } else {
            None
        }
    }

    pub fn description(&self) -> Option<&String> {
        match self {
            Self::AgentMode { description, .. } => description.as_ref(),
            Self::Command { description, .. } => description.as_ref(),
        }
    }

    pub fn arguments(&self) -> &Vec<Argument> {
        match self {
            Self::AgentMode { arguments, .. } => arguments,
            Self::Command { arguments, .. } => arguments,
        }
    }

    pub fn tags(&self) -> Option<&Vec<String>> {
        match self {
            Self::Command { tags, .. } => Some(tags),
            Self::AgentMode { .. } => None,
        }
    }

    pub fn source_url(&self) -> Option<&String> {
        match self {
            Self::Command { source_url, .. } => source_url.as_ref(),
            Self::AgentMode { .. } => None,
        }
    }

    pub fn author_name(&self) -> Option<&String> {
        match self {
            Self::Command { author, .. } => author.as_ref(),
            Self::AgentMode { .. } => None,
        }
    }

    pub fn shells(&self) -> Option<&Vec<warp_workflows::Shell>> {
        match self {
            Self::Command { shells, .. } => Some(shells),
            Self::AgentMode { .. } => None,
        }
    }

    pub fn is_command_workflow(&self) -> bool {
        matches!(self, Self::Command { .. })
    }

    pub fn is_agent_mode_workflow(&self) -> bool {
        matches!(self, Self::AgentMode { .. })
    }

    /// Returns `true` if the workflow name starts with the given character (case-insensitive).
    ///
    /// Used by prompt search datasources to prefix-match on single-character queries, where
    /// fuzzy matching would be unreliable.
    pub fn name_starts_with_char_ignore_case(&self, c: char) -> bool {
        self.name()
            .chars()
            .next()
            .is_some_and(|first| first.eq_ignore_ascii_case(&c))
    }

    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        Workflow::Command {
            name: name.into(),
            command: command.into(),
            tags: Vec::new(),
            arguments: Vec::new(),
            description: None,
            source_url: None,
            author: None,
            author_url: None,
            shells: Vec::new(),
        }
    }

    pub fn with_arguments(mut self, new_arguments: Vec<Argument>) -> Self {
        match self {
            Workflow::AgentMode {
                ref mut arguments, ..
            }
            | Workflow::Command {
                ref mut arguments, ..
            } => {
                *arguments = new_arguments;
            }
        }
        self
    }

    pub fn with_description(mut self, new_description: String) -> Self {
        match self {
            Workflow::AgentMode {
                ref mut description,
                ..
            }
            | Workflow::Command {
                ref mut description,
                ..
            } => {
                *description = Some(new_description);
            }
        }
        self
    }

    pub fn set_name(&mut self, new_name: &str) {
        match self {
            Workflow::AgentMode { name, .. } | Workflow::Command { name, .. } => {
                new_name.clone_into(name)
            }
        }
    }
}

/// Create a Torch Workflow model from a public-facing workflow
impl From<warp_workflows::Workflow> for Workflow {
    fn from(workflow: warp_workflows::Workflow) -> Self {
        Workflow::Command {
            name: workflow.name,
            command: workflow.command,
            description: workflow.description,
            arguments: workflow.arguments.into_iter().map(Argument::from).collect(),
            tags: workflow.tags,
            source_url: workflow.source_url,
            author: workflow.author,
            author_url: workflow.author_url,
            shells: workflow.shells,
        }
    }
}

/// Argument model to be used in `Torch`
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, Hash, Default)]
pub struct Argument {
    pub name: String,
    pub description: Option<String>,
    pub default_value: Option<String>,
}

impl From<warp_workflows::Argument> for Argument {
    fn from(arg: warp_workflows::Argument) -> Self {
        Argument {
            name: arg.name,
            description: arg.description,
            default_value: arg.default_value,
        }
    }
}

impl Argument {
    pub fn new(name: impl Into<String>) -> Self {
        Argument {
            name: name.into(),
            description: None,
            default_value: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default_value = Some(default.into());
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &Option<String> {
        &self.description
    }

    pub fn default_value(&self) -> &Option<String> {
        &self.default_value
    }
}
