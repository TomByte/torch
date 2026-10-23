//! Shared agent-event stream utilities used by orchestration consumers and
//! third-party harness bridges.

mod driver;
mod message_hydrator;

pub(crate) use driver::{
    AgentEventConsumer, AgentEventConsumerControlFlow, AgentEventDriverConfig, AgentEventFilter,
    AgentMessageEventMetadata, ServerApiAgentEventSource, run_agent_event_driver,
};
pub(crate) use message_hydrator::MessageHydrator;
