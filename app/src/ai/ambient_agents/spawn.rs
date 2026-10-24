//! Stream-based API for spawning and monitoring ambient agents.
#![cfg_attr(target_family = "wasm", expect(dead_code))]

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use futures::{FutureExt, Stream, StreamExt, select};
use session_sharing_protocol::common::SessionId;

use super::{AmbientAgentTask, AmbientAgentTaskId, AmbientAgentTaskState};
use crate::server::retry_strategies::with_bounded_retry;
use crate::server::server_api::ai::{AIClient, SpawnAgentRequest, TaskStatusMessage};
use crate::server::team_scope::RequestTeamScope;

/// How long to poll for the agent to be ready.
/// This should be long enough that the shared session will be joinable.
pub const TASK_STATUS_POLLING_DURATION: Duration = Duration::from_secs(80);

#[cfg(not(test))]
const TASK_STATUS_POLL_INTERVAL: Duration = Duration::from_secs(3);
#[cfg(test)]
const TASK_STATUS_POLL_INTERVAL: Duration = Duration::from_millis(1);

/// Information about a session join link for an ambient agent task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionJoinInfo {
    pub session_id: Option<SessionId>,
    pub session_link: String,
}

impl SessionJoinInfo {
    pub fn from_task(task: &AmbientAgentTask) -> Option<Self> {
        let run_execution = task.active_run_execution();
        // The cloud-mode pane joins on `session_id`; a standalone `session_link` isn't
        // actionable without it.
        let session_id_str = run_execution.session_id?;
        let session_id = SessionId::from_str(session_id_str).ok()?;

        // `active_run_execution()` already filters out empty links.
        let session_link = run_execution.session_link.map(String::from)?;
        Some(Self {
            session_id: Some(session_id),
            session_link,
        })
    }
}

/// Lifecycle events during ambient agent startup.
#[derive(Debug)]
pub enum AmbientAgentEvent {
    /// The task was successfully spawned with the given task ID and run ID.
    TaskSpawned { task_id: AmbientAgentTaskId },
    /// The task state changed.
    StateChanged {
        state: AmbientAgentTaskState,
        status_message: Option<TaskStatusMessage>,
    },
    /// Session started and join information became available.
    SessionStarted { session_join_info: SessionJoinInfo },
    /// Timed out waiting for the agent session to be ready.
    TimedOut,
    /// Cloud agent capacity limit has been reached. This does not block
    /// the task from eventually starting.
    AtCapacity,
}

/// Spawns an ambient agent task and monitors its state.
///
/// The stream completes when:
/// - The task completes (either successfully or with a failure)
/// - The task's shared session is ready to join
/// - The timeout expires (if provided)
/// - An error occurs
///
/// If `timeout` is `None`, there is no timeout.
pub fn spawn_task(
    request: SpawnAgentRequest,
    team_scope: RequestTeamScope,
    ai_client: Arc<dyn AIClient>,
    timeout: Option<Duration>,
) -> impl Stream<Item = Result<AmbientAgentEvent, anyhow::Error>> {
    // We can't use try_stream! because of the select! macro invocation.
    // See https://github.com/tokio-rs/async-stream/issues/63.
    async_stream::stream! {
        // First, spawn the ambient agent task.
        let (task_id, at_capacity) = match ai_client.spawn_agent(request, team_scope).await {
            Ok(response) => (response.task_id, response.at_capacity),
            Err(err) => {
                yield Err(err);
                return;
            },
        };

        let mut stream = Box::pin(monitor_spawned_task(
            task_id,
            at_capacity,
            ai_client,
            timeout,
        ));
        while let Some(event) = stream.next().await {
            yield event;
        }
    }
}

/// Monitors an ambient task that has already been created.
///
/// This preserves the event contract of [`spawn_task`] while allowing callers
/// that own task creation to avoid issuing a second spawn request.
pub fn monitor_spawned_task(
    task_id: AmbientAgentTaskId,
    at_capacity: bool,
    ai_client: Arc<dyn AIClient>,
    timeout: Option<Duration>,
) -> impl Stream<Item = Result<AmbientAgentEvent, anyhow::Error>> {
    async_stream::stream! {
        yield Ok(AmbientAgentEvent::TaskSpawned { task_id });

        // Emit AtCapacity event if the server indicates capacity limit reached.
        if at_capacity {
            yield Ok(AmbientAgentEvent::AtCapacity);
        }

        let mut stream = Box::pin(poll_run_until_joinable_session(
            task_id,
            ai_client,
            timeout,
        ));
        while let Some(event) = stream.next().await {
            yield event;
        }
    }
}

fn poll_run_until_joinable_session(
    run_id: AmbientAgentTaskId,
    ai_client: Arc<dyn AIClient>,
    timeout: Option<Duration>,
) -> impl Stream<Item = Result<AmbientAgentEvent, anyhow::Error>> {
    async_stream::stream! {
        // Poll for the task until it completes OR has session join info.
        // We use a timeout to ensure we don't wait indefinitely for session info.
        // If no timeout is provided, we use a future that never completes.
        let mut timeout_timer = FutureExt::fuse(match timeout {
            Some(d) => torchui::r#async::Timer::after(d),
            None => torchui::r#async::Timer::never(),
        });
        let mut last_state = None;
        loop {
            let mut poll_timer = FutureExt::fuse(torchui::r#async::Timer::after(TASK_STATUS_POLL_INTERVAL));

            select! {
                _ = timeout_timer => {
                    yield Ok(AmbientAgentEvent::TimedOut);
                    return;
                }
                _ = poll_timer => {
                    // Wrap the status poll in with_bounded_retry so transient
                    // HTTP errors (429, 5xx) are retried with exponential
                    // backoff instead of immediately killing the CLI.
                    let poll_result = {
                        let client = ai_client.clone();
                        with_bounded_retry(
                            &format!("poll agent {run_id}"),
                            || {
                                let client = client.clone();
                                async move { client.get_ambient_agent_task(&run_id).await }
                            },
                        )
                        .await
                    };
                    match poll_result {
                        Ok(task) => {
                            // Log every non-InProgress observation BEFORE the skip check
                            // so we retain visibility into stalls where the server is
                            // wedged on the prior terminal state.
                            if task.state != AmbientAgentTaskState::InProgress {
                                log::info!("Agent {run_id} state: {:?}", task.state);
                            }

                            if last_state.as_ref() != Some(&task.state) {
                                last_state = Some(task.state.clone());
                                yield Ok(AmbientAgentEvent::StateChanged {
                                    state: task.state.clone(),
                                    status_message: task.status_message.clone(),
                                });
                            }

                            if task.state.is_terminal() {
                                return;
                            }

                            if task.state == AmbientAgentTaskState::InProgress
                                && let Some(session_join_info) = SessionJoinInfo::from_task(&task) {
                                    yield Ok(AmbientAgentEvent::SessionStarted {
                                        session_join_info,
                                    });
                                    return;
                                }
                        }
                        Err(err) => {
                            yield Err(err);
                            return;
                        },
                    }
                }
            }
        }
    }
}
