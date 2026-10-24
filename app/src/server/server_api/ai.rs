use std::collections::{HashMap, HashSet};
use std::time::Duration;

use ai::api_keys::{ChatGPTConnection, ChatGPTConnectionStatus};
use ai::index::full_source_code_embedding::store_client::{IntermediateNode, StoreClient};
use ai::index::full_source_code_embedding::{
    self, CodebaseContextConfig, ContentHash, EmbeddingConfig, NodeHash, RepoMetadata,
};
use anyhow::{Context as _, anyhow};
use async_trait::async_trait;
use base64::Engine;
use bytes::Bytes;
use cynic::{MutationBuilder, QueryBuilder};
use itertools::Itertools;
#[cfg(test)]
use mockall::automock;
use prost::Message;
use torch_core::channel::ChannelState;
use torch_core::features::FeatureFlag;
use torch_errors::report_error;
use torch_graphql::client::Operation;
use torch_graphql::mutations::delete_ai_conversation::{
    DeleteAIConversation, DeleteAIConversationVariables, DeleteConversationInput,
    DeleteConversationResult,
};
use torch_graphql::mutations::disconnect_chatgpt::{
    DisconnectChatGPT, DisconnectChatGPTResult, DisconnectChatGPTVariables,
};
use torch_graphql::mutations::generate_code_embeddings::{
    GenerateCodeEmbeddings, GenerateCodeEmbeddingsInput, GenerateCodeEmbeddingsResult,
    GenerateCodeEmbeddingsVariables,
};
use torch_graphql::mutations::generate_commands::{
    GenerateCommands, GenerateCommandsInput, GenerateCommandsResult, GenerateCommandsStatus,
    GenerateCommandsVariables,
};
use torch_graphql::mutations::generate_metadata_for_command::{
    GenerateMetadataForCommand, GenerateMetadataForCommandInput, GenerateMetadataForCommandResult,
    GenerateMetadataForCommandStatus, GenerateMetadataForCommandVariables,
};
use torch_graphql::mutations::populate_merkle_tree_cache::{
    PopulateMerkleTreeCache, PopulateMerkleTreeCacheResult, PopulateMerkleTreeCacheVariables,
};
use torch_graphql::mutations::request_bonus::{
    ProvideNegativeFeedbackResponseForAiConversation,
    ProvideNegativeFeedbackResponseForAiConversationInput,
    ProvideNegativeFeedbackResponseForAiConversationVariables, RequestsRefundedResult,
};
use torch_graphql::mutations::start_chatgpt_link::{
    StartChatGPTLink, StartChatGPTLinkResult, StartChatGPTLinkVariables,
};
use torch_graphql::mutations::update_merkle_tree::{
    MerkleTreeNode, UpdateMerkleTree, UpdateMerkleTreeInput, UpdateMerkleTreeResult,
    UpdateMerkleTreeVariables,
};
use torch_graphql::queries::codebase_context_config::{
    CodebaseContextConfigQuery, CodebaseContextConfigResult, CodebaseContextConfigVariables,
};
use torch_graphql::queries::free_available_models::{
    FreeAvailableModels, FreeAvailableModelsInput, FreeAvailableModelsResult,
    FreeAvailableModelsVariables,
};
#[cfg(not(feature = "agent_mode_evals"))]
use torch_graphql::queries::get_ai_credit_availability::{
    GetAICreditAvailability, GetAICreditAvailabilityVariables,
};
use torch_graphql::queries::get_chatgpt_connection::{
    GetChatGPTConnection, GetChatGPTConnectionVariables,
};
use torch_graphql::queries::get_conversation_usage::{
    ConversationUsage, GetConversationUsage, GetConversationUsageVariables, UserResult,
};
use torch_graphql::queries::get_feature_model_choices::{
    GetFeatureModelChoices, GetFeatureModelChoicesVariables,
};
use torch_graphql::queries::get_relevant_fragments::{
    GetRelevantFragmentsQuery, GetRelevantFragmentsResult, GetRelevantFragmentsVariables,
};
#[cfg(not(feature = "agent_mode_evals"))]
use torch_graphql::queries::get_request_limit_info::{
    GetRequestLimitInfo, GetRequestLimitInfoVariables,
};
use torch_graphql::queries::rerank_fragments::{
    RerankFragments, RerankFragmentsResult, RerankFragmentsVariables,
};
use torch_graphql::queries::sync_merkle_tree::{
    SyncMerkleTree, SyncMerkleTreeInput, SyncMerkleTreeResult, SyncMerkleTreeVariables,
};
use torch_server_client::PresignedDownloadResponse;
use warp_multi_agent_api::ConversationData;

use super::ServerApi;
#[cfg(not(feature = "agent_mode_evals"))]
use crate::ai::BonusGrant;
use crate::ai::agent::api::ServerConversationToken;
use crate::ai::agent::conversation::{
    AIAgentConversationFormat, AIAgentHarness, AIAgentSerializedBlockFormat,
    ServerAIConversationMetadata,
};
use crate::ai::generate_code_review_content::api::{
    GenerateCodeReviewContentRequest, GenerateCodeReviewContentResponse,
};
use crate::ai::llms::{
    AvailableLLMs, DisableReason, LLMContextWindow, LLMInfo, LLMModelHost, LLMSpec,
    LLMUsageMetadata, ModelsByFeature, RoutingHostConfig,
};
#[cfg(feature = "agent_mode_evals")]
use crate::ai::request_usage_model::RequestLimitInfo;
use crate::ai::{AICreditAvailability, RequestUsageInfo};
use crate::ai_assistant::execution_context::WarpAiExecutionContext;
use crate::ai_assistant::{AIGeneratedCommand, GenerateCommandsFromNaturalLanguageError};
use crate::persistence::model::ConversationUsageMetadata;
use crate::server::graphql::{get_request_context, get_user_facing_error_message};
use crate::terminal::model::block::SerializedBlock;
use crate::workflows::ai_assist::{GeneratedCommandMetadata, GeneratedCommandMetadataError};
#[cfg(not(feature = "agent_mode_evals"))]
use crate::{
    server::ids::ServerId,
    workspaces::{gql_convert::PLACEHOLDER_WORKSPACE_UID, workspace::WorkspaceUid},
};

const AI_ASSISTANT_REQUEST_TIMEOUT_SECONDS: u64 = 30;

/// Request body for `POST /agent/conversations/{conversation_id}/fork`.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ForkConversationRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
}

/// Response body for `POST /agent/conversations/{conversation_id}/fork`. The returned id is sent
/// on the subsequent `POST /agent/runs` request under `conversation_id` (resume semantics).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ForkConversationResponse {
    pub forked_conversation_id: String,
}

/// Request body for `POST /agent/conversations/{conversation_id}/rename`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RenameConversationRequest {
    pub title: String,
}

/// Response body for `POST /agent/conversations/{conversation_id}/rename`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RenameConversationResponse {
    pub title: String,
}

// --- Orchestrations V2 messaging types ---

pub(crate) fn build_fork_conversation_url(conversation_id: &str) -> String {
    format!(
        "agent/conversations/{}/fork",
        urlencoding::encode(conversation_id)
    )
}

pub(crate) fn build_rename_conversation_url(conversation_id: &str) -> String {
    format!(
        "agent/conversations/{}/rename",
        urlencoding::encode(conversation_id)
    )
}

#[cfg_attr(test, automock)]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
pub trait AIClient: 'static + Send + Sync {
    async fn generate_commands_from_natural_language(
        &self,
        prompt: String,
        ai_execution_context: Option<WarpAiExecutionContext>,
    ) -> Result<Vec<AIGeneratedCommand>, GenerateCommandsFromNaturalLanguageError>;

    async fn generate_metadata_for_command(
        &self,
        command: String,
    ) -> Result<GeneratedCommandMetadata, GeneratedCommandMetadataError>;

    async fn get_request_limit_info(&self) -> Result<RequestUsageInfo, anyhow::Error>;

    /// Fetches the server-authoritative decision on whether the authenticated
    /// user can start an interactive AI request.
    async fn get_ai_credit_availability(&self) -> Result<AICreditAvailability, anyhow::Error>;

    /// Fetches the ChatGPT account linked to the authenticated user, if any.
    async fn get_chatgpt_connection(&self) -> Result<ChatGPTConnectionStatus, anyhow::Error>;

    /// Unlinks the authenticated user's ChatGPT account on the server.
    async fn disconnect_chatgpt(&self) -> Result<(), anyhow::Error>;

    /// Returns the browser URL that links a ChatGPT account to the authenticated user. The
    /// browser flow finishes by opening `continue_url` with `chatgpt_linked=1` or
    /// `chatgpt_error=<code>` appended.
    async fn start_chatgpt_link(&self, continue_url: String) -> Result<String, anyhow::Error>;

    /// Returns conversation usage history for the current user over the requested number of days.
    ///
    /// If `last_updated_end_timestamp` is provided, only conversations updated before that timestamp are returned.
    async fn get_conversation_usage_history(
        &self,
        days: Option<i32>,
        limit: Option<i32>,
        last_updated_end_timestamp: Option<torch_graphql::scalars::Time>,
    ) -> Result<Vec<ConversationUsage>, anyhow::Error>;

    async fn get_feature_model_choices(&self) -> Result<ModelsByFeature, anyhow::Error>;

    /// Fetches the free-tier available models without requiring authentication.
    /// Used during pre-login onboarding so logged-out users see an accurate model list
    /// instead of the hard-coded `ModelsByFeature::default()` fallback.
    async fn get_free_available_models(
        &self,
        referrer: Option<String>,
    ) -> Result<ModelsByFeature, anyhow::Error>;

    async fn update_merkle_tree(
        &self,
        embedding_config: EmbeddingConfig,
        nodes: Vec<IntermediateNode>,
    ) -> anyhow::Result<HashMap<NodeHash, bool>>;

    async fn generate_code_embeddings(
        &self,
        embedding_config: EmbeddingConfig,
        fragments: Vec<full_source_code_embedding::Fragment>,
        root_hash: NodeHash,
        repo_metadata: RepoMetadata,
    ) -> anyhow::Result<HashMap<ContentHash, bool>>;

    async fn provide_negative_feedback_response_for_ai_conversation(
        &self,
        conversation_id: String,
        request_ids: Vec<String>,
    ) -> anyhow::Result<i32, anyhow::Error>;

    /// Materialize a server-side fork of a conversation.
    async fn fork_conversation(
        &self,
        conversation_id: String,
        title: Option<String>,
    ) -> anyhow::Result<ForkConversationResponse, anyhow::Error>;

    /// Rename a server-side conversation and return the normalized title.
    async fn rename_conversation(
        &self,
        conversation_id: String,
        title: String,
    ) -> anyhow::Result<RenameConversationResponse, anyhow::Error>;

    async fn get_ai_conversation(
        &self,
        server_conversation_token: ServerConversationToken,
    ) -> anyhow::Result<(ConversationData, ServerAIConversationMetadata), anyhow::Error>;

    async fn list_ai_conversation_metadata(
        &self,
        conversation_ids: Option<Vec<String>>,
    ) -> anyhow::Result<Vec<ServerAIConversationMetadata>>;

    async fn get_ai_conversation_format(
        &self,
        server_conversation_token: ServerConversationToken,
    ) -> anyhow::Result<AIAgentConversationFormat, anyhow::Error>;

    async fn get_block_snapshot(
        &self,
        server_conversation_token: ServerConversationToken,
    ) -> anyhow::Result<SerializedBlock, anyhow::Error>;

    async fn delete_ai_conversation(
        &self,
        server_conversation_token: String,
    ) -> anyhow::Result<(), anyhow::Error>;

    /// Downloads the bytes of a computer-use screenshot stored in Torch-managed
    /// object storage. Requires view access to the conversation.
    async fn download_stored_screenshot(
        &self,
        conversation_id: &str,
        screenshot_uid: &str,
    ) -> anyhow::Result<Bytes, anyhow::Error>;

    /// Generates AI copy for code-review flows: commit messages at dialog-open
    /// time and PR titles / bodies at confirm time. `output_type` in the
    /// request picks which of the three the server returns.
    async fn generate_code_review_content(
        &self,
        request: GenerateCodeReviewContentRequest,
    ) -> Result<GenerateCodeReviewContentResponse, anyhow::Error>;
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl AIClient for ServerApi {
    async fn generate_commands_from_natural_language(
        &self,
        prompt: String,
        // TODO: use relevant context from RequestContext and deprecate usage of ai_execution_context
        _ai_execution_context: Option<WarpAiExecutionContext>,
    ) -> Result<Vec<AIGeneratedCommand>, GenerateCommandsFromNaturalLanguageError> {
        let default_err = GenerateCommandsFromNaturalLanguageError::Other;

        let variables = GenerateCommandsVariables {
            input: GenerateCommandsInput { prompt },
            request_context: get_request_context(),
        };

        let operation = GenerateCommands::build(variables);
        let response = self
            .send_graphql_request(
                operation,
                Some(Duration::from_secs(AI_ASSISTANT_REQUEST_TIMEOUT_SECONDS)),
            )
            .await
            .map_err(|_| default_err)?;

        match response.generate_commands {
            GenerateCommandsResult::GenerateCommandsOutput(output) => match output.status {
                GenerateCommandsStatus::GenerateCommandsSuccess(success) => {
                    Ok(success.commands.into_iter().map(Into::into).collect_vec())
                }
                GenerateCommandsStatus::GenerateCommandsFailure(failure) => {
                    Err(failure.type_.into())
                }
                GenerateCommandsStatus::Unknown => {
                    Err(GenerateCommandsFromNaturalLanguageError::Other)
                }
            },
            _ => Err(GenerateCommandsFromNaturalLanguageError::Other),
        }
    }

    async fn generate_metadata_for_command(
        &self,
        command: String,
    ) -> Result<GeneratedCommandMetadata, GeneratedCommandMetadataError> {
        let default_err = GeneratedCommandMetadataError::Other;
        let variables = GenerateMetadataForCommandVariables {
            input: GenerateMetadataForCommandInput { command },
            request_context: get_request_context(),
        };

        let operation = GenerateMetadataForCommand::build(variables);
        let response = self
            .send_graphql_request(
                operation,
                Some(Duration::from_secs(AI_ASSISTANT_REQUEST_TIMEOUT_SECONDS)),
            )
            .await
            .map_err(|_| default_err)?;

        match response.generate_metadata_for_command {
            GenerateMetadataForCommandResult::GenerateMetadataForCommandOutput(output) => {
                match output.status {
                    GenerateMetadataForCommandStatus::GenerateMetadataForCommandSuccess(
                        success,
                    ) => Ok(success.into()),
                    GenerateMetadataForCommandStatus::GenerateMetadataForCommandFailure(
                        failure,
                    ) => Err(failure.type_.into()),
                    GenerateMetadataForCommandStatus::Unknown => {
                        Err(GeneratedCommandMetadataError::Other)
                    }
                }
            }
            _ => Err(GeneratedCommandMetadataError::Other),
        }
    }

    #[cfg(feature = "agent_mode_evals")]
    async fn get_request_limit_info(&self) -> Result<RequestUsageInfo, anyhow::Error> {
        Ok(RequestUsageInfo {
            request_limit_info: RequestLimitInfo::new_for_evals(),
            bonus_grants: vec![],
        })
    }

    #[cfg(not(feature = "agent_mode_evals"))]
    async fn get_request_limit_info(&self) -> Result<RequestUsageInfo, anyhow::Error> {
        let variables = GetRequestLimitInfoVariables {
            request_context: get_request_context(),
        };
        let operation = GetRequestLimitInfo::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.user {
            torch_graphql::queries::get_request_limit_info::UserResult::UserOutput(user_output) => {
                let request_limit_info = user_output.user.request_limit_info.into();

                let workspace_and_team_bonus_grants = user_output
                    .user
                    .workspaces
                    .into_iter()
                    .filter(|workspace| workspace.uid != PLACEHOLDER_WORKSPACE_UID.into())
                    .flat_map(|workspace| {
                        let workspace_uid =
                            WorkspaceUid::from(ServerId::from_string_lossy(workspace.uid.inner()));
                        workspace
                            .bonus_grants_info
                            .grants
                            .into_iter()
                            .map(move |grant| {
                                BonusGrant::from_gql_workspace_or_team_bonus_grant(
                                    grant,
                                    workspace_uid,
                                )
                            })
                    });

                let bonus_grants: Vec<BonusGrant> = user_output
                    .user
                    .bonus_grants
                    .into_iter()
                    .map(BonusGrant::from_gql_user_bonus_grant)
                    .chain(workspace_and_team_bonus_grants)
                    .collect();

                Ok(RequestUsageInfo {
                    request_limit_info,
                    bonus_grants,
                })
            }
            torch_graphql::queries::get_request_limit_info::UserResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)))
            }
            torch_graphql::queries::get_request_limit_info::UserResult::Unknown => {
                Err(anyhow!("failed to get request limit info"))
            }
        }
    }

    #[cfg(feature = "agent_mode_evals")]
    async fn get_ai_credit_availability(&self) -> Result<AICreditAvailability, anyhow::Error> {
        Ok(AICreditAvailability::available_with_source(Some(
            crate::ai::AICreditSource::BaseLimit,
        )))
    }

    #[cfg(not(feature = "agent_mode_evals"))]
    async fn get_ai_credit_availability(&self) -> Result<AICreditAvailability, anyhow::Error> {
        let variables = GetAICreditAvailabilityVariables {
            request_context: get_request_context(),
        };
        let operation = GetAICreditAvailability::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.user {
            torch_graphql::queries::get_ai_credit_availability::UserResult::UserOutput(output) => {
                Ok(output.user.ai_credit_availability.into())
            }
            torch_graphql::queries::get_ai_credit_availability::UserResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)))
            }
            torch_graphql::queries::get_ai_credit_availability::UserResult::Unknown => {
                Err(anyhow!("failed to get AI credit availability"))
            }
        }
    }

    async fn get_chatgpt_connection(&self) -> Result<ChatGPTConnectionStatus, anyhow::Error> {
        use torch_graphql::queries::get_chatgpt_connection::UserResult;

        let operation = GetChatGPTConnection::build(GetChatGPTConnectionVariables {
            request_context: get_request_context(),
        });
        let response = self.send_graphql_request(operation, None).await?;

        match response.user {
            UserResult::UserOutput(output) => Ok(match output.user.chatgpt_connection {
                Some(connection) => ChatGPTConnectionStatus::Connected(ChatGPTConnection {
                    email: connection.email,
                    connected_at: connection.connected_at.utc().into(),
                    token_sharing_active: connection.token_sharing_active,
                }),
                None => ChatGPTConnectionStatus::NotConnected,
            }),
            UserResult::UserFacingError(e) => Err(anyhow!(get_user_facing_error_message(e))),
            UserResult::Unknown => Err(anyhow!("failed to get ChatGPT connection")),
        }
    }

    async fn disconnect_chatgpt(&self) -> Result<(), anyhow::Error> {
        let operation = DisconnectChatGPT::build(DisconnectChatGPTVariables {
            request_context: get_request_context(),
        });
        let response = self.send_graphql_request(operation, None).await?;

        match response.disconnect_chatgpt {
            DisconnectChatGPTResult::DisconnectChatGPTOutput(_) => Ok(()),
            DisconnectChatGPTResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)))
            }
            DisconnectChatGPTResult::Unknown => Err(anyhow!("failed to disconnect ChatGPT")),
        }
    }

    async fn start_chatgpt_link(&self, continue_url: String) -> Result<String, anyhow::Error> {
        let operation = StartChatGPTLink::build(StartChatGPTLinkVariables {
            request_context: get_request_context(),
            continue_url,
        });
        let response = self.send_graphql_request(operation, None).await?;

        match response.start_chatgpt_link {
            StartChatGPTLinkResult::StartChatGPTLinkOutput(output) => Ok(output.authorization_url),
            StartChatGPTLinkResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)))
            }
            StartChatGPTLinkResult::Unknown => Err(anyhow!("failed to start ChatGPT link")),
        }
    }

    async fn get_conversation_usage_history(
        &self,
        days: Option<i32>,
        limit: Option<i32>,
        last_updated_end_timestamp: Option<torch_graphql::scalars::Time>,
    ) -> Result<Vec<ConversationUsage>, anyhow::Error> {
        let operation = GetConversationUsage::build(GetConversationUsageVariables {
            request_context: get_request_context(),
            days,
            limit,
            last_updated_end_timestamp,
        });
        let response = self.send_graphql_request(operation, None).await?;
        match response.user {
            UserResult::UserOutput(output) => Ok(output.user.conversation_usage),
            UserResult::Unknown => Err(anyhow!("Unable to fetch conversation usage")),
        }
    }

    async fn get_feature_model_choices(&self) -> Result<ModelsByFeature, anyhow::Error> {
        let variables = GetFeatureModelChoicesVariables {
            request_context: get_request_context(),
        };
        let operation = GetFeatureModelChoices::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.user {
            torch_graphql::queries::get_feature_model_choices::UserResult::UserOutput(
                torch_graphql::queries::get_feature_model_choices::UserOutput {
                    user: torch_graphql::queries::get_feature_model_choices::User { mut workspaces },
                },
            ) if !workspaces.is_empty() => {
                // This is safe (`remove()` can panic) because we ensure workspaces is non-empty
                // above.
                workspaces.remove(0).feature_model_choice.try_into()
            }
            _ => Err(anyhow!("Failed to get available feature model choices")),
        }
    }

    async fn get_free_available_models(
        &self,
        referrer: Option<String>,
    ) -> Result<ModelsByFeature, anyhow::Error> {
        // This resolver is public; it does not require an auth token. We must NOT go through
        // `send_graphql_request`, which awaits `get_or_refresh_access_token()`
        let variables = FreeAvailableModelsVariables {
            input: FreeAvailableModelsInput { referrer },
            request_context: get_request_context(),
        };
        let operation = FreeAvailableModels::build(variables);

        // Best-effort: if the user has a valid token (e.g. anonymous Firebase), include it;
        // otherwise send unauthenticated. Either is acceptable for this resolver.
        let auth_token = self
            .get_or_refresh_access_token()
            .await
            .ok()
            .and_then(|token| token.bearer_token());

        let response = operation
            .send_request(
                self.base_client.owned_http_client(),
                self.base_client
                    .graphql_request_options_with_token(auth_token),
            )
            .await?
            .data
            .ok_or_else(|| anyhow!("Missing data in freeAvailableModels response"))?;

        match response.free_available_models {
            FreeAvailableModelsResult::FreeAvailableModelsOutput(output) => {
                output.feature_model_choice.try_into()
            }
            FreeAvailableModelsResult::Unknown => {
                Err(anyhow!("Unexpected freeAvailableModels response variant"))
            }
        }
    }

    async fn update_merkle_tree(
        &self,
        embedding_config: EmbeddingConfig,
        nodes: Vec<IntermediateNode>,
    ) -> anyhow::Result<HashMap<NodeHash, bool>> {
        let nodes = nodes
            .into_iter()
            .map(|node| MerkleTreeNode {
                hash: node.hash.into(),
                children: node.children.into_iter().map(Into::into).collect(),
            })
            .collect_vec();
        let variables = UpdateMerkleTreeVariables {
            input: UpdateMerkleTreeInput {
                embedding_config: embedding_config.into(),
                nodes,
            },
            request_context: get_request_context(),
        };
        let operation = UpdateMerkleTree::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.update_merkle_tree {
            UpdateMerkleTreeResult::UpdateMerkleTreeOutput(output) => {
                let mut node_results = HashMap::with_capacity(output.results.len());
                for result in output.results {
                    node_results.insert(result.hash.try_into()?, result.success);
                }
                Ok(node_results)
            }
            UpdateMerkleTreeResult::UpdateMerkleTreeError(e) => Err(anyhow!(e.error)),
            UpdateMerkleTreeResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)))
            }
            UpdateMerkleTreeResult::Unknown => Err(anyhow!("failed to update merkle tree")),
        }
    }

    async fn generate_code_embeddings(
        &self,
        embedding_config: EmbeddingConfig,
        fragments: Vec<full_source_code_embedding::Fragment>,
        root_hash: NodeHash,
        repo_metadata: RepoMetadata,
    ) -> anyhow::Result<HashMap<ContentHash, bool>> {
        let variables = GenerateCodeEmbeddingsVariables {
            input: GenerateCodeEmbeddingsInput {
                embedding_config: embedding_config.into(),
                fragments: fragments.into_iter().map(Into::into).collect(),
                repo_metadata: repo_metadata.into(),
                root_hash: root_hash.into(),
            },
            request_context: get_request_context(),
        };

        let operation = GenerateCodeEmbeddings::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.generate_code_embeddings {
            GenerateCodeEmbeddingsResult::GenerateCodeEmbeddingsOutput(output) => {
                let mut results = HashMap::with_capacity(output.embedding_results.len());
                for result in output.embedding_results {
                    results.insert(result.hash.try_into()?, result.success);
                }
                Ok(results)
            }
            GenerateCodeEmbeddingsResult::GenerateCodeEmbeddingsError(e) => Err(anyhow!(e.error)),
            GenerateCodeEmbeddingsResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)))
            }
            GenerateCodeEmbeddingsResult::Unknown => {
                Err(anyhow!("failed to generate code embeddings"))
            }
        }
    }

    async fn provide_negative_feedback_response_for_ai_conversation(
        &self,
        conversation_id: String,
        request_ids: Vec<String>,
    ) -> anyhow::Result<i32, anyhow::Error> {
        let variables = ProvideNegativeFeedbackResponseForAiConversationVariables {
            input: ProvideNegativeFeedbackResponseForAiConversationInput {
                conversation_id: conversation_id.into(),
                request_ids: request_ids.into_iter().map(Into::into).collect(),
            },
            request_context: get_request_context(),
        };

        let operation = ProvideNegativeFeedbackResponseForAiConversation::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.provide_negative_feedback_response_for_ai_conversation {
            RequestsRefundedResult::RequestsRefundedOutput(output) => Ok(output.requests_refunded),
            RequestsRefundedResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)))
            }
            RequestsRefundedResult::Unknown => Err(anyhow!(
                "failed to provide negative feedback response for ai conversation"
            )),
        }
    }

    async fn fork_conversation(
        &self,
        conversation_id: String,
        title: Option<String>,
    ) -> anyhow::Result<ForkConversationResponse, anyhow::Error> {
        let request = ForkConversationRequest { title };
        let response: ForkConversationResponse = self
            .post_public_api(&build_fork_conversation_url(&conversation_id), &request)
            .await?;
        Ok(response)
    }

    async fn rename_conversation(
        &self,
        conversation_id: String,
        title: String,
    ) -> anyhow::Result<RenameConversationResponse, anyhow::Error> {
        let request = RenameConversationRequest { title };
        let response: RenameConversationResponse = self
            .post_public_api(&build_rename_conversation_url(&conversation_id), &request)
            .await?;
        Ok(response)
    }

    #[tracing::instrument(skip_all, err, fields(tags.cloud_agent = true))]
    async fn get_ai_conversation(
        &self,
        server_conversation_token: ServerConversationToken,
    ) -> anyhow::Result<(ConversationData, ServerAIConversationMetadata), anyhow::Error> {
        use torch_graphql::queries::list_ai_conversations::{
            ListAIConversations, ListAIConversationsInput, ListAIConversationsResult,
            ListAIConversationsVariables,
        };

        let conversation_id = server_conversation_token.as_str().to_string();
        let operation = ListAIConversations::build(ListAIConversationsVariables {
            input: ListAIConversationsInput {
                conversation_ids: Some(vec![cynic::Id::new(conversation_id)]),
            },
            request_context: get_request_context(),
        });
        let response = self.send_graphql_request(operation, None).await?;

        let gql_conversation = match response.list_ai_conversations {
            ListAIConversationsResult::ListAIConversationsOutput(output) => output
                .conversations
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("Conversation not found"))?,
            ListAIConversationsResult::UserFacingError(e) => {
                return Err(anyhow!(get_user_facing_error_message(e)));
            }
            ListAIConversationsResult::Unknown => {
                return Err(anyhow!("Failed to get AI conversation"));
            }
        };

        let conversation_data_bytes = base64::engine::general_purpose::STANDARD
            .decode(&gql_conversation.final_task_list)
            .map_err(|e| anyhow!("Failed to decode base64 conversation data: {e}"))?;

        let conversation_data = ConversationData::decode(conversation_data_bytes.as_slice())
            .map_err(|e| anyhow!("Failed to decode proto ConversationData: {e}"))?;

        // Build AIConversationMetadata from GraphQL response
        let metadata = gql_conversation.try_into()?;

        Ok((conversation_data, metadata))
    }

    async fn list_ai_conversation_metadata(
        &self,
        conversation_ids: Option<Vec<String>>,
    ) -> anyhow::Result<Vec<ServerAIConversationMetadata>> {
        if !FeatureFlag::CloudConversations.is_enabled() {
            return Ok(vec![]);
        }
        use torch_graphql::queries::list_ai_conversations::{
            ListAIConversationMetadata, ListAIConversationMetadataResult,
            ListAIConversationMetadataVariables, ListAIConversationsInput,
        };

        let input = ListAIConversationsInput {
            conversation_ids: conversation_ids
                .map(|ids| ids.into_iter().map(cynic::Id::new).collect()),
        };

        let variables = ListAIConversationMetadataVariables {
            input,
            request_context: get_request_context(),
        };

        let operation = ListAIConversationMetadata::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.list_ai_conversations {
            ListAIConversationMetadataResult::ListAIConversationsOutput(output) => {
                let metadata_vec: Result<Vec<_>, _> = output
                    .conversations
                    .into_iter()
                    .map(|conv| conv.try_into())
                    .collect();
                metadata_vec
            }
            ListAIConversationMetadataResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)))
            }
            ListAIConversationMetadataResult::Unknown => {
                Err(anyhow!("Failed to list AI conversations metadata"))
            }
        }
    }

    async fn get_ai_conversation_format(
        &self,
        server_conversation_token: ServerConversationToken,
    ) -> anyhow::Result<AIAgentConversationFormat, anyhow::Error> {
        use torch_graphql::queries::get_ai_conversation_format::{
            GetAIConversationFormat, GetAIConversationFormatResult,
            GetAIConversationFormatVariables,
        };
        use torch_graphql::queries::list_ai_conversations::ListAIConversationsInput;

        let conversation_id = server_conversation_token.as_str().to_string();
        let operation = GetAIConversationFormat::build(GetAIConversationFormatVariables {
            input: ListAIConversationsInput {
                conversation_ids: Some(vec![cynic::Id::new(conversation_id)]),
            },
            request_context: get_request_context(),
        });
        let response = self.send_graphql_request(operation, None).await?;

        match response.list_ai_conversations {
            GetAIConversationFormatResult::ListAIConversationsOutput(output) => {
                let conversation = output
                    .conversations
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow!("Conversation not found"))?;
                Ok(convert_conversation_format(conversation.format))
            }
            GetAIConversationFormatResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)))
            }
            GetAIConversationFormatResult::Unknown => {
                Err(anyhow!("Failed to get AI conversation format"))
            }
        }
    }

    async fn get_block_snapshot(
        &self,
        server_conversation_token: ServerConversationToken,
    ) -> anyhow::Result<SerializedBlock, anyhow::Error> {
        let conversation_id = server_conversation_token.as_str();
        // Make sure to use `SerializedBlock::from_json` to correctly handle the serialized
        // command and output grid contents.
        let response = self
            .get_public_api_response(&format!(
                "agent/conversations/{conversation_id}/block-snapshot?redirect=false"
            ))
            .await?;
        let download = response
            .json::<PresignedDownloadResponse>()
            .await
            .map_err(reqwest::Error::without_url)
            .context("Failed to decode block snapshot download URL")?;
        let json_bytes = self.base_client.download_presigned(&download.url).await?;
        SerializedBlock::from_json(&json_bytes)
    }

    async fn delete_ai_conversation(
        &self,
        server_conversation_token: String,
    ) -> anyhow::Result<(), anyhow::Error> {
        let variables = DeleteAIConversationVariables {
            input: DeleteConversationInput {
                conversation_id: server_conversation_token.into(),
            },
            request_context: get_request_context(),
        };

        let operation = DeleteAIConversation::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.delete_conversation {
            DeleteConversationResult::DeleteConversationOutput(_) => Ok(()),
            DeleteConversationResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)))
            }
            DeleteConversationResult::Unknown => Err(anyhow!("Failed to delete AI conversation")),
        }
    }

    async fn download_stored_screenshot(
        &self,
        conversation_id: &str,
        screenshot_uid: &str,
    ) -> anyhow::Result<Bytes, anyhow::Error> {
        // The endpoint redirects to a short-lived signed URL, which the HTTP
        // client follows transparently. Strip that URL from body-read errors so
        // it cannot leak into logs or Sentry breadcrumbs.
        let response = self
            .get_public_api_response(&format!(
                "agent/conversations/{}/screenshots/{}/download",
                urlencoding::encode(conversation_id),
                urlencoding::encode(screenshot_uid)
            ))
            .await?;
        response
            .bytes()
            .await
            .map_err(|error| anyhow::Error::new(error.without_url()))
    }

    async fn generate_code_review_content(
        &self,
        request: GenerateCodeReviewContentRequest,
    ) -> Result<GenerateCodeReviewContentResponse, anyhow::Error> {
        let auth_token = self.get_or_refresh_access_token().await?;
        let request_builder = self.base_client.http_client().post(format!(
            "{}/ai/generate_code_review_content",
            ChannelState::server_root_url()
        ));
        let response = if let Some(token) = auth_token.as_bearer_token() {
            request_builder.bearer_auth(token)
        } else {
            request_builder
        }
        .json(&request)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
        Ok(response)
    }
}

impl TryFrom<torch_graphql::queries::get_feature_model_choices::FeatureModelChoice>
    for ModelsByFeature
{
    type Error = anyhow::Error;

    fn try_from(
        value: torch_graphql::queries::get_feature_model_choices::FeatureModelChoice,
    ) -> Result<Self, Self::Error> {
        Ok(Self {
            agent_mode: value.agent_mode.try_into()?,
            coding: value.coding.try_into()?,
            cli_agent: Some(value.cli_agent.try_into()?),
            computer_use: Some(value.computer_use_agent.try_into()?),
        })
    }
}

impl TryFrom<torch_graphql::workspace::FeatureModelChoice> for ModelsByFeature {
    type Error = anyhow::Error;

    fn try_from(value: torch_graphql::workspace::FeatureModelChoice) -> Result<Self, Self::Error> {
        Ok(Self {
            agent_mode: value.agent_mode.try_into()?,
            coding: value.coding.try_into()?,
            cli_agent: Some(value.cli_agent.try_into()?),
            computer_use: Some(value.computer_use_agent.try_into()?),
        })
    }
}

impl TryFrom<torch_graphql::queries::get_feature_model_choices::AvailableLlms> for AvailableLLMs {
    type Error = anyhow::Error;

    fn try_from(
        value: torch_graphql::queries::get_feature_model_choices::AvailableLlms,
    ) -> Result<Self, Self::Error> {
        Self::new(
            value.default_id.into(),
            value.choices.into_iter().map(LLMInfo::from),
            value.preferred_codex_model_id.map(Into::into),
        )
    }
}

impl TryFrom<torch_graphql::workspace::AvailableLlms> for AvailableLLMs {
    type Error = anyhow::Error;

    fn try_from(value: torch_graphql::workspace::AvailableLlms) -> Result<Self, Self::Error> {
        Self::new(
            value.default_id.into(),
            value.choices.into_iter().map(LLMInfo::from),
            value.preferred_codex_model_id.map(Into::into),
        )
    }
}

impl From<torch_graphql::queries::get_feature_model_choices::LlmInfo> for LLMInfo {
    fn from(value: torch_graphql::queries::get_feature_model_choices::LlmInfo) -> Self {
        let host_configs = {
            let mut map = std::collections::HashMap::new();
            for config in value.host_configs {
                let config: RoutingHostConfig = config.into();
                let host = config.model_routing_host.clone();
                if map.insert(host.clone(), config).is_some() {
                    log::warn!(
                        "Duplicate LlmModelHost entry for {:?}, using latest value",
                        host
                    );
                }
            }
            map
        };
        Self {
            id: value.id.into(),
            display_name: value.display_name,
            base_model_name: value.base_model_name,
            reasoning_level: value.reasoning_level,
            usage_metadata: value.usage_metadata.into(),
            description: value.description,
            disable_reason: value.disable_reason.map(DisableReason::from),
            vision_supported: value.vision_supported,
            spec: value.spec.map(Into::into),
            provider: value.provider.into(),
            host_configs,
            discount_percentage: value.pricing.discount_percentage.map(|v| v as f32),
            context_window: LLMContextWindow {
                is_configurable: value.context_window.is_configurable,
                min: value.context_window.min.into(),
                max: value.context_window.max.into(),
                default_max: value.context_window.default.into(),
            },
        }
    }
}

impl From<torch_graphql::workspace::LlmInfo> for LLMInfo {
    fn from(value: torch_graphql::workspace::LlmInfo) -> Self {
        let host_configs = {
            let mut map = std::collections::HashMap::new();
            for config in value.host_configs {
                let config: RoutingHostConfig = config.into();
                let host = config.model_routing_host.clone();
                if map.insert(host.clone(), config).is_some() {
                    log::warn!(
                        "Duplicate LlmModelHost entry for {:?}, using latest value",
                        host
                    );
                }
            }
            map
        };
        Self {
            id: value.id.into(),
            display_name: value.display_name,
            base_model_name: value.base_model_name,
            reasoning_level: value.reasoning_level,
            usage_metadata: value.usage_metadata.into(),
            description: value.description,
            disable_reason: value.disable_reason.map(DisableReason::from),
            vision_supported: value.vision_supported,
            spec: value.spec.map(Into::into),
            provider: value.provider.into(),
            host_configs,
            discount_percentage: value.pricing.discount_percentage.map(|v| v as f32),
            context_window: LLMContextWindow {
                is_configurable: value.context_window.is_configurable,
                min: value.context_window.min.into(),
                max: value.context_window.max.into(),
                default_max: value.context_window.default.into(),
            },
        }
    }
}

impl From<torch_graphql::queries::get_feature_model_choices::RoutingHostConfig>
    for RoutingHostConfig
{
    fn from(value: torch_graphql::queries::get_feature_model_choices::RoutingHostConfig) -> Self {
        Self {
            enabled: value.enabled,
            model_routing_host: value.model_routing_host.into(),
        }
    }
}

impl From<torch_graphql::workspace::RoutingHostConfig> for RoutingHostConfig {
    fn from(value: torch_graphql::workspace::RoutingHostConfig) -> Self {
        Self {
            enabled: value.enabled,
            model_routing_host: value.model_routing_host.into(),
        }
    }
}

impl From<torch_graphql::queries::get_feature_model_choices::LlmModelHost> for LLMModelHost {
    fn from(value: torch_graphql::queries::get_feature_model_choices::LlmModelHost) -> Self {
        match value {
            torch_graphql::queries::get_feature_model_choices::LlmModelHost::DirectApi => {
                LLMModelHost::DirectApi
            }
            torch_graphql::queries::get_feature_model_choices::LlmModelHost::AwsBedrock => {
                LLMModelHost::AwsBedrock
            }
            torch_graphql::queries::get_feature_model_choices::LlmModelHost::CustomEndpoint => {
                LLMModelHost::CustomEndpoint
            }
            torch_graphql::queries::get_feature_model_choices::LlmModelHost::GeminiEnterprise => {
                LLMModelHost::GeminiEnterprise
            }
            torch_graphql::queries::get_feature_model_choices::LlmModelHost::Other(value) => {
                log::warn!(
                    "Unknown LlmModelHost '{value}'. Make sure to update client GraphQL types!"
                );
                LLMModelHost::Unknown
            }
        }
    }
}

impl From<torch_graphql::queries::get_feature_model_choices::LlmSpec> for LLMSpec {
    fn from(value: torch_graphql::queries::get_feature_model_choices::LlmSpec) -> Self {
        Self {
            cost: value.cost as f32,
            quality: value.quality as f32,
            speed: value.speed as f32,
        }
    }
}

impl From<torch_graphql::workspace::LlmSpec> for LLMSpec {
    fn from(value: torch_graphql::workspace::LlmSpec) -> Self {
        Self {
            cost: value.cost as f32,
            quality: value.quality as f32,
            speed: value.speed as f32,
        }
    }
}

impl From<torch_graphql::queries::get_feature_model_choices::LlmUsageMetadata>
    for LLMUsageMetadata
{
    fn from(value: torch_graphql::queries::get_feature_model_choices::LlmUsageMetadata) -> Self {
        Self {
            request_multiplier: value.request_multiplier.max(1) as usize,
            credit_multiplier: value.credit_multiplier.map(|v| v as f32),
        }
    }
}

impl From<torch_graphql::workspace::LlmUsageMetadata> for LLMUsageMetadata {
    fn from(value: torch_graphql::workspace::LlmUsageMetadata) -> Self {
        Self {
            request_multiplier: value.request_multiplier.max(1) as usize,
            credit_multiplier: value.credit_multiplier.map(|v| v as f32),
        }
    }
}

impl From<torch_graphql::queries::get_feature_model_choices::DisableReason> for DisableReason {
    fn from(value: torch_graphql::queries::get_feature_model_choices::DisableReason) -> Self {
        match value {
            torch_graphql::queries::get_feature_model_choices::DisableReason::AdminDisabled => {
                DisableReason::AdminDisabled
            }
            torch_graphql::queries::get_feature_model_choices::DisableReason::OutOfRequests => {
                DisableReason::OutOfRequests
            }
            torch_graphql::queries::get_feature_model_choices::DisableReason::ProviderOutage => {
                DisableReason::ProviderOutage
            }
            torch_graphql::queries::get_feature_model_choices::DisableReason::RequiresUpgrade => {
                DisableReason::RequiresUpgrade
            }
            torch_graphql::queries::get_feature_model_choices::DisableReason::Other(_) => {
                DisableReason::Unavailable
            }
        }
    }
}

impl From<torch_graphql::workspace::DisableReason> for DisableReason {
    fn from(value: torch_graphql::workspace::DisableReason) -> Self {
        match value {
            torch_graphql::workspace::DisableReason::AdminDisabled => DisableReason::AdminDisabled,
            torch_graphql::workspace::DisableReason::OutOfRequests => DisableReason::OutOfRequests,
            torch_graphql::workspace::DisableReason::ProviderOutage => {
                DisableReason::ProviderOutage
            }
            torch_graphql::workspace::DisableReason::RequiresUpgrade => {
                DisableReason::RequiresUpgrade
            }
            torch_graphql::workspace::DisableReason::Other(_) => DisableReason::Unavailable,
        }
    }
}

// Conversions for AIConversationMetadata from GraphQL types

fn convert_harness(harness: torch_graphql::ai::AgentHarness) -> AIAgentHarness {
    match harness {
        torch_graphql::ai::AgentHarness::Oz => AIAgentHarness::Oz,
        torch_graphql::ai::AgentHarness::ClaudeCode => AIAgentHarness::ClaudeCode,
        torch_graphql::ai::AgentHarness::Gemini => AIAgentHarness::Gemini,
        torch_graphql::ai::AgentHarness::Pi => AIAgentHarness::Pi,
        torch_graphql::ai::AgentHarness::Codex => AIAgentHarness::Codex,
        torch_graphql::ai::AgentHarness::Other(value) => {
            report_error!(
                "Invalid AgentHarness; update client GraphQL types",
                extra: { "harness" => %value },
                torch_errors::ReportErrorLogMode::OncePerRun
            );
            AIAgentHarness::Unknown
        }
    }
}

fn convert_block_snapshot_format(
    format: torch_graphql::ai::SerializedBlockFormat,
) -> AIAgentSerializedBlockFormat {
    match format {
        torch_graphql::ai::SerializedBlockFormat::JsonV1 => AIAgentSerializedBlockFormat::JsonV1,
    }
}

fn convert_conversation_format(
    format: torch_graphql::ai::AIConversationFormat,
) -> AIAgentConversationFormat {
    AIAgentConversationFormat {
        has_task_list: format.has_task_list,
        block_snapshot: format.block_snapshot.map(convert_block_snapshot_format),
    }
}

impl TryFrom<torch_graphql::ai::AIConversation> for ServerAIConversationMetadata {
    type Error = anyhow::Error;

    fn try_from(value: torch_graphql::ai::AIConversation) -> Result<Self, Self::Error> {
        // Full conversion including per-model token usage and tool usage
        // stats, so restored conversations render the same usage details
        // (e.g. the credits-expansion "Models" rows) as live ones.
        let usage: ConversationUsageMetadata = (&value.usage.usage_metadata).into();
        let metadata = value.metadata.try_into()?;
        let permissions = value.permissions.try_into()?;
        let server_conversation_token =
            ServerConversationToken::new(value.conversation_id.into_inner());

        Ok(Self {
            title: value.title,
            working_directory: value.working_directory,
            harness: convert_harness(value.harness),
            usage,
            metadata,
            creator: value.creator.map(Into::into),
            permissions,
            server_conversation_token,
        })
    }
}

impl TryFrom<torch_graphql::queries::list_ai_conversations::AIConversationMetadata>
    for ServerAIConversationMetadata
{
    type Error = anyhow::Error;

    fn try_from(
        value: torch_graphql::queries::list_ai_conversations::AIConversationMetadata,
    ) -> Result<Self, Self::Error> {
        let usage: ConversationUsageMetadata = (&value.usage.usage_metadata).into();
        let metadata = value.metadata.try_into()?;
        let permissions = value.permissions.try_into()?;
        let server_conversation_token =
            ServerConversationToken::new(value.conversation_id.into_inner());

        Ok(Self {
            title: value.title,
            working_directory: value.working_directory,
            harness: convert_harness(value.harness),
            usage,
            metadata,
            creator: value.creator.map(Into::into),
            permissions,
            server_conversation_token,
        })
    }
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl StoreClient for ServerApi {
    async fn update_intermediate_nodes(
        &self,
        embedding_config: EmbeddingConfig,
        nodes: Vec<IntermediateNode>,
    ) -> Result<HashMap<NodeHash, bool>, full_source_code_embedding::Error> {
        let results = self.update_merkle_tree(embedding_config, nodes).await?;
        Ok(results)
    }

    async fn generate_embeddings(
        &self,
        embedding_config: EmbeddingConfig,
        fragments: Vec<full_source_code_embedding::Fragment>,
        root_hash: NodeHash,
        repo_metadata: RepoMetadata,
    ) -> Result<HashMap<ContentHash, bool>, full_source_code_embedding::Error> {
        let results = self
            .generate_code_embeddings(embedding_config, fragments, root_hash, repo_metadata)
            .await?;
        Ok(results)
    }

    async fn populate_merkle_tree_cache(
        &self,
        embedding_config: EmbeddingConfig,
        root_hash: NodeHash,
        repo_metadata: RepoMetadata,
    ) -> Result<bool, full_source_code_embedding::Error> {
        let variables = PopulateMerkleTreeCacheVariables {
            embedding_config: embedding_config.into(),
            root_hash: root_hash.into(),
            repo_metadata: repo_metadata.into(),
            request_context: get_request_context(),
        };
        let operation = PopulateMerkleTreeCache::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.populate_merkle_tree_cache {
            PopulateMerkleTreeCacheResult::PopulateMerkleTreeCacheOutput(output) => {
                Ok(output.success)
            }
            PopulateMerkleTreeCacheResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)).into())
            }
            PopulateMerkleTreeCacheResult::Unknown => {
                Err(anyhow!("failed to populate merkle tree cache").into())
            }
        }
    }

    async fn sync_merkle_tree(
        &self,
        nodes: Vec<NodeHash>,
        embedding_config: EmbeddingConfig,
    ) -> Result<HashSet<NodeHash>, full_source_code_embedding::Error> {
        let input = SyncMerkleTreeInput {
            hashed_nodes: nodes.into_iter().map(Into::into).collect(),
            embedding_config: embedding_config.into(),
        };

        let variables = SyncMerkleTreeVariables {
            input,
            request_context: get_request_context(),
        };

        let operation = SyncMerkleTree::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.sync_merkle_tree {
            SyncMerkleTreeResult::SyncMerkleTreeOutput(output) => {
                let mut node_results = HashSet::with_capacity(output.changed_nodes.len());
                for hash in output.changed_nodes {
                    node_results.insert(hash.try_into()?);
                }
                Ok(node_results)
            }
            SyncMerkleTreeResult::SyncMerkleTreeError(e) => Err(anyhow!(e.error).into()),
            SyncMerkleTreeResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)).into())
            }
            SyncMerkleTreeResult::Unknown => Err(anyhow!("failed to sync merkle tree").into()),
        }
    }

    async fn rerank_fragments(
        &self,
        query: String,
        fragments: Vec<full_source_code_embedding::Fragment>,
    ) -> Result<Vec<full_source_code_embedding::Fragment>, full_source_code_embedding::Error> {
        let variables = RerankFragmentsVariables {
            query,
            fragments: fragments.into_iter().map(Into::into).collect(),
            request_context: get_request_context(),
        };
        let operation = RerankFragments::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.rerank_fragments {
            RerankFragmentsResult::RerankFragmentsOutput(output) => Ok(output
                .ranked_fragments
                .into_iter()
                .map(|fragment| fragment.try_into())
                .collect::<Result<Vec<_>, _>>()?),
            RerankFragmentsResult::RerankFragmentsError(e) => Err(anyhow!(e.error).into()),
            RerankFragmentsResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)).into())
            }
            RerankFragmentsResult::Unknown => Err(anyhow!("failed to rerank fragments").into()),
        }
    }

    async fn get_relevant_fragments(
        &self,
        embedding_config: EmbeddingConfig,
        query: String,
        root_hash: NodeHash,
        repo_metadata: RepoMetadata,
    ) -> Result<Vec<ContentHash>, full_source_code_embedding::Error> {
        let variables = GetRelevantFragmentsVariables {
            query,
            root_hash: root_hash.into(),
            embedding_config: embedding_config.into(),
            request_context: get_request_context(),
            repo_metadata: repo_metadata.into(),
        };
        let operation = GetRelevantFragmentsQuery::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.get_relevant_fragments {
            GetRelevantFragmentsResult::GetRelevantFragmentsOutput(output) => Ok(output
                .candidate_hashes
                .into_iter()
                .map(|hash| hash.try_into())
                .collect::<Result<Vec<_>, _>>()?),
            GetRelevantFragmentsResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)).into())
            }
            GetRelevantFragmentsResult::GetRelevantFragmentsError(e) => {
                Err(anyhow!(e.error).into())
            }
            GetRelevantFragmentsResult::Unknown => {
                Err(anyhow!("failed to get relevant fragments").into())
            }
        }
    }

    async fn codebase_context_config(
        &self,
    ) -> Result<CodebaseContextConfig, full_source_code_embedding::Error> {
        let variables = CodebaseContextConfigVariables {
            request_context: get_request_context(),
        };
        let operation = CodebaseContextConfigQuery::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.codebase_context_config {
            CodebaseContextConfigResult::CodebaseContextConfigOutput(output) => {
                Ok(CodebaseContextConfig {
                    embedding_config: output.embedding_config.try_into()?,
                    embedding_cadence: Duration::from_secs(output.embedding_cadence as u64),
                })
            }
            CodebaseContextConfigResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)).into())
            }
            CodebaseContextConfigResult::Unknown => {
                Err(anyhow!("failed to retrieve codebase context config").into())
            }
        }
    }
}
