use itertools::Itertools;
use serde::{Deserialize, Serialize};
use torch_graphql::mutations::generate_metadata_for_command::{
    GenerateMetadataForCommandFailureType, GenerateMetadataForCommandSuccess,
};

/// Generated command metadata from server.
#[derive(Debug)]
pub struct GeneratedCommandMetadata {
    pub command: String,
    pub title: String,
    pub description: String,
    pub arguments: Vec<GeneratedArgument>,
}

/// Metadata for a parameter in the workflow.
#[derive(Debug)]
pub struct GeneratedArgument {
    pub name: String,
    pub description: String,
    pub default_value: String,
}

impl From<GenerateMetadataForCommandSuccess> for GeneratedCommandMetadata {
    fn from(value: GenerateMetadataForCommandSuccess) -> Self {
        GeneratedCommandMetadata {
            command: value.parameterized_command,
            title: value.title,
            description: value.description,
            arguments: value
                .parameters
                .into_iter()
                .map(|p| GeneratedArgument {
                    name: p.name,
                    description: p.description,
                    default_value: p.value,
                })
                .collect_vec(),
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum GeneratedCommandMetadataError {
    /// OpenAI failed to generate a parsable response.
    BadCommand,
    /// Request to OpenAI failed
    AiProviderError,
    /// User is over rate limit.
    RateLimited,
    Other,
}

impl GeneratedCommandMetadataError {
    pub fn user_facing_message(&self) -> String {
        match self {
            Self::BadCommand => {
                "Failed to generate metadata. Please try again with a different command."
            }
            Self::AiProviderError => "Something went wrong. Please try again.",
            Self::RateLimited => "Looks like you're out of AI credits. Please try again later.",
            Self::Other => "Something went wrong. Please try again.",
        }
        .to_string()
    }
}

impl From<GenerateMetadataForCommandFailureType> for GeneratedCommandMetadataError {
    fn from(value: GenerateMetadataForCommandFailureType) -> Self {
        match value {
            GenerateMetadataForCommandFailureType::BadCommand => Self::BadCommand,
            GenerateMetadataForCommandFailureType::AiProviderError => Self::AiProviderError,
            GenerateMetadataForCommandFailureType::RateLimited => Self::RateLimited,
            GenerateMetadataForCommandFailureType::Other => Self::Other,
        }
    }
}
