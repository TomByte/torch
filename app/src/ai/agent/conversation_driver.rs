/// Who authors the turns of an `AIConversation`, and therefore which client-side behaviours
/// apply to it. Call sites should ask the derived predicates rather than match on variants, so
/// a new driver only has to answer each predicate once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConversationDriver {
    /// Torch's agent loop: the MAA server streams events and this client executes tools.
    #[default]
    Native,
    /// Another client's conversation mirrored over a shared session; view-only here.
    SharedSessionViewer,
    /// A restored 3p CLI transcript rendered through a vehicle conversation; view-only here.
    CliAgentTranscript,
}

impl ConversationDriver {
    /// This client sends follow-ups, settles status from action results, and may cancel the turn.
    pub fn owns_turn_lifecycle(self) -> bool {
        match self {
            Self::Native => true,
            Self::SharedSessionViewer | Self::CliAgentTranscript => false,
        }
    }

    /// This client runs the conversation's tool calls through the action model.
    pub fn executes_tool_calls_locally(self) -> bool {
        match self {
            Self::Native => true,
            Self::SharedSessionViewer | Self::CliAgentTranscript => false,
        }
    }

    /// Exchange inputs are rebuilt from streamed `UserQuery` / `ToolCallResult` messages instead
    /// of being inserted by the local executor when the request is sent.
    pub fn reconstructs_inputs_from_messages(self) -> bool {
        match self {
            Self::SharedSessionViewer => true,
            Self::Native | Self::CliAgentTranscript => false,
        }
    }

    /// The agent view renders this conversation without an input footer or other affordances
    /// that would let the user act on it.
    pub fn is_read_only_ui(self) -> bool {
        match self {
            Self::SharedSessionViewer | Self::CliAgentTranscript => true,
            Self::Native => false,
        }
    }

    /// The conversation is persisted to the local session database.
    pub fn is_persisted_locally(self) -> bool {
        match self {
            Self::Native | Self::CliAgentTranscript => true,
            Self::SharedSessionViewer => false,
        }
    }
}
