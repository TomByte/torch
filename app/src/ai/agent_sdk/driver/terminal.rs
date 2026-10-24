use std::collections::HashMap;
use std::ffi::OsString;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::channel::oneshot;
use instant::Instant;
use torch_completer::completer::CommandOutput;
use torch_core::command::ExitCode;
use torch_terminal::model::grid::Dimensions;
use torch_util::path::ShellFamily;
use torchui::r#async::FutureExt;
use torchui::{AppContext, Entity, ModelContext, ModelHandle, ViewHandle};
use uuid::Uuid;

use super::AgentDriverError;
use crate::ai::agent::redaction::redact_secrets;
use crate::ai::ambient_agents::AmbientAgentTaskId;
use crate::ai::attachment_utils::attachments_download_dir;
use crate::pane_group::NewTerminalOptions;
use crate::root_view::{NewWorkspaceSource, open_new_with_workspace_source};
use crate::terminal::TerminalView;
use crate::terminal::model::RespectObfuscatedSecrets;
use crate::terminal::model::block::{BlockId, BlockState, SerializedBlock};
use crate::terminal::model::find::RegexDFAs;
use crate::terminal::model::grid::RespectDisplayedOutput;
use crate::terminal::model::index::Point;
use crate::terminal::model::session::ExecuteCommandOptions;
use crate::terminal::model::terminal_model::ShellProcessInfo;
use crate::terminal::shell::ShellType;
use crate::terminal::view::ConversationRestorationInNewPaneType;
use crate::workspaces::user_workspaces::{HeadlessTeamScope, TeamScope};

/// Describes why a terminal session bootstrap failed.
#[derive(Debug, Clone)]
pub(crate) enum BootstrapError {
    /// The PTY or shell process failed before the bootstrap script completed.
    /// When `reason` is `Some`, the message is
    /// "Shell spawn failed: {reason}. Check the Torch logs for details."
    /// When `reason` is `None`, it is
    /// "Shell spawn failed. Check the Torch logs for details."
    PtySpawnFailed { reason: Option<String> },
    /// The bootstrap script did not complete within the expected time.
    TimedOut,
    /// An unexpected internal error in the bootstrap channel
    /// (e.g. the sender was dropped without sending).
    InternalError,
}

impl std::fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BootstrapError::PtySpawnFailed { reason: Some(r) } => {
                write!(
                    f,
                    "Shell spawn failed: {r}. Check the Torch logs for details."
                )
            }
            BootstrapError::PtySpawnFailed { reason: None } => {
                write!(f, "Shell spawn failed. Check the Torch logs for details.")
            }
            BootstrapError::TimedOut => write!(
                f,
                "Terminal session did not start within the expected time. \
                 Check the Torch logs for details."
            ),
            BootstrapError::InternalError => {
                write!(f, "An unexpected internal error occurred during bootstrap.")
            }
        }
    }
}

impl std::error::Error for BootstrapError {}

const TERMINAL_SESSION_BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(60);
/// Options for creating the terminal view before constructing a [`TerminalDriver`].
pub(crate) struct TerminalDriverOptions<'a> {
    pub working_dir: PathBuf,
    pub env_vars: HashMap<OsString, OsString>,
    pub task_id: Option<AmbientAgentTaskId>,
    pub conversation_restoration: Option<ConversationRestorationInNewPaneType>,
    pub team_scope: Option<&'a HeadlessTeamScope>,
}

/// Events emitted by [`TerminalDriver`] for [`super::AgentDriver`] to react to.
pub(crate) enum TerminalDriverEvent {
    /// Terminal bootstrap is taking unusually long.
    SlowBootstrap,
}

struct WaitingCommand {
    exit_tx: oneshot::Sender<Result<ExitCode, AgentDriverError>>,
    started_block_id: Option<BlockId>,
}

/// Manages the terminal session lifecycle for the agent driver.
///
/// Responsibilities:
/// - Monitoring for terminal bootstrapping to be done
/// - Executing commands in the session
/// - Detecting block completion
pub(crate) struct TerminalDriver {
    terminal_view: ViewHandle<TerminalView>,
    #[cfg(test)]
    pub(super) launch_env_vars_for_test: Option<HashMap<OsString, OsString>>,
    /// Sender half of the bootstrap result channel. Exactly one of
    /// `SessionBootstrapped`, `PtySpawnFailed`, or `Exited` (pre-bootstrap)
    /// will send the outcome; all others no-op after the first send.
    bootstrap_tx: Option<oneshot::Sender<Result<(), BootstrapError>>>,
    /// Receiver half consumed by `wait_for_session_bootstrapped`.
    bootstrap_rx: Option<oneshot::Receiver<Result<(), BootstrapError>>>,
    /// Resolves the in-flight command's exit status. Sent `Ok` when the
    /// command's block completes, or
    /// `Err(AgentDriverError::SetupCommandExitedShell)` if the shell process
    /// exits while the command is still running.
    waiting_command: Option<WaitingCommand>,

    /// State for the pending command we're expecting to start executing.
    /// The `String` is the expected command text, and the sender is used
    /// to send the block ID to the waiting caller (or a shell-exit error if
    /// the shell dies before the command starts).
    pending_command_start: Option<(String, oneshot::Sender<Result<BlockId, AgentDriverError>>)>,

    /// True once the shell process backing this session has exited
    /// post-bootstrap. No further commands can execute, so
    /// [`Self::execute_command`] fails fast with
    /// [`AgentDriverError::SetupCommandExitedShell`].
    shell_exited: bool,

    /// The most recently submitted command (secret-redacted), used to
    /// attribute a shell exit to the command that caused it. When the shell
    /// dies mid-command, the exit path force-finishes the command's block
    /// (with exit code 0) before `Event::Exited` is delivered, so at exit
    /// time this — not any still-pending command — names the culprit.
    ///
    /// Stored redacted because it flows into error reports (server task
    /// status, Sentry) via [`AgentDriverError::SetupCommandExitedShell`].
    last_command: Option<String>,
}

impl Entity for TerminalDriver {
    type Event = TerminalDriverEvent;
}

/// Create the terminal window and extract the [`ViewHandle<TerminalView>`].
///
/// This is separate from [`TerminalDriver::new`] because [`AppContext::add_model`]
/// requires an infallible constructor; the fallible window/view creation must happen first.
fn create_terminal_view(
    options: TerminalDriverOptions<'_>,
    ctx: &mut AppContext,
) -> Result<ViewHandle<TerminalView>, AgentDriverError> {
    let initial_team_uid = options.team_scope.and_then(TeamScope::team_uid);
    let (_, root_view) = open_new_with_workspace_source(
        NewWorkspaceSource::Session {
            options: Box::new(NewTerminalOptions {
                initial_directory: Some(options.working_dir),
                env_vars: options.env_vars,
                conversation_restoration: options.conversation_restoration,
                ..Default::default()
            }),
            initial_team_uid,
        },
        ctx,
    );

    root_view
        .as_ref(ctx)
        .workspace_view()
        .as_ref(ctx)
        .active_tab_pane_group()
        .as_ref(ctx)
        .active_session_view(ctx)
        .ok_or(AgentDriverError::TerminalUnavailable)
}

impl TerminalDriver {
    /// Create a terminal view from the given options and wrap it in a new `TerminalDriver` model.
    pub(crate) fn create(
        options: TerminalDriverOptions<'_>,
        ctx: &mut AppContext,
    ) -> Result<ModelHandle<Self>, AgentDriverError> {
        #[cfg(test)]
        let launch_env_vars_for_test = options.env_vars.clone();
        let task_id = options.task_id;
        let working_dir = options.working_dir.clone();
        let terminal_view = create_terminal_view(options, ctx)?;
        let driver = ctx.add_model(|ctx| Self::new(terminal_view, task_id, working_dir, ctx));
        #[cfg(test)]
        driver.update(ctx, |driver, _| {
            driver.launch_env_vars_for_test = Some(launch_env_vars_for_test);
        });
        Ok(driver)
    }

    /// Wrap an already-created terminal view in a new `TerminalDriver` model.
    ///
    /// Unlike [`Self::create`], this does not open a new window — it reuses an
    /// existing view (e.g. a docker sandbox pane). No task ID is associated.
    pub(crate) fn create_from_existing_view(
        terminal_view: ViewHandle<TerminalView>,
        ctx: &mut AppContext,
    ) -> ModelHandle<Self> {
        ctx.add_model(|ctx| Self::new(terminal_view, None, PathBuf::default(), ctx))
    }

    /// Set up event subscriptions for an already-created terminal view.
    fn new(
        terminal_view: ViewHandle<TerminalView>,
        task_id: Option<AmbientAgentTaskId>,
        working_dir: PathBuf,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        // Set the task_id and attachments download dir on the AI controller right away
        // so they're available for file downloads.
        // Only set the download dir when a task_id is present (cloud mode),
        // since attachments require a task to fetch presigned URLs.
        if let Some(tid) = task_id {
            let attachments_dir = attachments_download_dir(&working_dir);
            terminal_view.update(ctx, |terminal, ctx| {
                terminal.ai_controller().update(ctx, |controller, ctx| {
                    controller.set_ambient_agent_task_id(Some(tid), ctx);
                    controller.set_attachments_download_dir(attachments_dir);
                });
            });
        }

        ctx.subscribe_to_view(&terminal_view, move |me, _, event, ctx| {
            me.handle_terminal_view_event(event, ctx);
        });

        let (bootstrap_tx_inner, bootstrap_rx) = oneshot::channel::<Result<(), BootstrapError>>();

        // If bootstrap already completed before we subscribed, resolve the
        // channel immediately so `wait_for_session_bootstrapped` doesn't block.
        let already_bootstrapped = terminal_view.read(ctx, |terminal, _| {
            terminal
                .model
                .lock()
                .block_list()
                .is_bootstrapping_precmd_done()
        });
        let bootstrap_tx = if already_bootstrapped {
            let _ = bootstrap_tx_inner.send(Ok(()));
            None
        } else {
            Some(bootstrap_tx_inner)
        };

        Self {
            terminal_view,
            #[cfg(test)]
            launch_env_vars_for_test: None,
            bootstrap_tx,
            bootstrap_rx: Some(bootstrap_rx),
            waiting_command: None,
            pending_command_start: None,
            shell_exited: false,
            last_command: None,
        }
    }

    /// Get a handle to the backing terminal view.
    pub fn terminal_view(&self) -> &ViewHandle<TerminalView> {
        &self.terminal_view
    }

    /// Provide mutable access to the terminal view through a closure.
    pub fn with_terminal_view(
        &self,
        ctx: &mut ModelContext<Self>,
        f: impl FnOnce(&mut TerminalView, &mut torchui::ViewContext<TerminalView>),
    ) {
        self.terminal_view.update(ctx, f);
    }

    /// Submit `text` to the active CLI agent on the terminal PTY using the
    /// agent-specific submission strategy.
    ///
    /// Used to send exit commands to third-party harnesses.
    pub(super) fn send_text_to_cli(&self, text: String, ctx: &mut ModelContext<Self>) {
        self.terminal_view.update(ctx, |terminal, ctx| {
            terminal.submit_text_to_cli_agent_pty(text, ctx);
        });
    }

    /// Sends a raw Enter (`\r`) to the CLI agent's PTY, bypassing the normal
    /// rich-input submission pipeline (which no-ops on empty text). Used to
    /// retry a bare Enter during harness exit escalation — e.g. in case a
    /// prior exit write was silently dropped, or to dismiss a confirmation
    /// prompt.
    pub(super) fn send_bare_enter_to_cli(&self, ctx: &mut ModelContext<Self>) {
        self.terminal_view.update(ctx, |terminal, ctx| {
            terminal.submit_bare_enter_to_cli_agent_pty(ctx);
        });
    }

    /// Sends a raw Ctrl-C (`\x03`) to the PTY, interrupting the foreground process group. Unlike
    /// [`Self::send_bare_enter_to_cli`] this does not require a CLI agent session, so it also
    /// reaches harnesses that run as plain commands in the session.
    pub(super) fn send_interrupt_to_pty(&self, ctx: &mut ModelContext<Self>) {
        self.terminal_view.update(ctx, |terminal, ctx| {
            terminal.write_to_pty(b"\x03".to_vec(), ctx);
        });
    }

    /// The pty's shell process info for this terminal, if the shell has been
    /// spawned and hasn't exited. Used to locate the actual foreground
    /// process group when force-killing a harness that didn't exit
    /// gracefully.
    pub(super) fn shell_process_info(&self, ctx: &AppContext) -> Option<ShellProcessInfo> {
        let terminal = self.terminal_view.as_ref(ctx);
        terminal.model.lock().shell_process_info().copied()
    }

    /// Return a snapshot of the block with the given ID.
    pub fn block_snapshot(&self, block_id: &BlockId, ctx: &AppContext) -> Option<SerializedBlock> {
        let terminal = self.terminal_view.as_ref(ctx);
        let model = terminal.model.lock();
        model
            .block_list()
            .block_with_id(block_id)
            .map(SerializedBlock::from)
    }

    /// Full visible plaintext of `block_id`'s active grid (no ANSI escape
    /// sequences; secrets obfuscated). Used by the harness output monitor
    /// to detect whether the block has stalled — two byte-identical
    /// snapshots taken N seconds apart imply the harness has produced no
    /// new output and no spinner activity.
    ///
    /// When the block is in [`BlockState::BeforeExecution`] (preexec has not
    /// fired yet), terminal output — including shell continuation prompts such
    /// as `dquote>` from unbalanced quotes — is routed to the command/header
    /// grid rather than the output grid. We read from there in that state so
    /// stall detection can observe a stuck continuation prompt.
    ///
    /// We intentionally pass `None` for `max_rows` so we compare the entire
    /// visible output; capping the row count could falsely report "stalled"
    /// when content scrolled below the cap actually changed.
    pub fn block_output_plaintext(&self, block_id: &BlockId, ctx: &AppContext) -> Option<String> {
        let terminal = self.terminal_view.as_ref(ctx);
        let model = terminal.model.lock();
        let block = model.block_list().block_with_id(block_id)?;
        let grid = if block.state() == BlockState::BeforeExecution {
            block.prompt_and_command_grid()
        } else {
            block.output_grid()
        };
        Some(grid.contents_to_string(
            false, // include_escape_sequences
            None,  // max_rows: full visible output
        ))
    }

    pub fn find_first_match_in_block_output(
        &self,
        block_id: &BlockId,
        dfas: &RegexDFAs,
        ctx: &AppContext,
    ) -> Option<BlockOutputMatch> {
        let terminal = self.terminal_view.as_ref(ctx);
        let model = terminal.model.lock();
        let block = model.block_list().block_with_id(block_id)?;
        let grid = block.output_grid();
        let m = grid.find(dfas).next()?;
        let handler = grid.grid_handler();
        let matched_text = handler.bounds_to_string(
            *m.start(),
            *m.end(),
            false, // include_esc_sequences
            RespectObfuscatedSecrets::Yes,
            false, // force_secrets_obfuscated
            RespectDisplayedOutput::Yes,
        );
        let cols = handler.columns();
        let row_start = Point::new(m.start().row, 0);
        let row_end = Point::new(m.end().row, cols.saturating_sub(1));
        let excerpt = handler.bounds_to_string(
            row_start,
            row_end,
            false,
            RespectObfuscatedSecrets::Yes,
            false,
            RespectDisplayedOutput::Yes,
        );
        Some(BlockOutputMatch {
            matched_text: matched_text.trim().to_owned(),
            excerpt: excerpt.trim().to_owned(),
        })
    }

    /// The error reported for commands affected by a shell exit, attributing
    /// the most recently submitted command as the cause.
    fn shell_exited_error(&self) -> AgentDriverError {
        AgentDriverError::SetupCommandExitedShell {
            command: self
                .last_command
                .clone()
                .unwrap_or_else(|| "<unknown>".to_string()),
        }
    }

    /// Execute a command in the terminal and return a future that resolves to a
    /// [`CommandHandle`] once the command starts executing.
    pub fn execute_command(
        &mut self,
        command: &str,
        ctx: &mut ModelContext<Self>,
    ) -> Result<
        impl Future<Output = Result<CommandHandle, AgentDriverError>> + use<>,
        AgentDriverError,
    > {
        // The shell process has exited, so no further commands can run in
        // this session. Fail fast with the shell-exit error so callers
        // (e.g. environment setup) report the failure instead of waiting
        // forever on a command that can never start.
        if self.shell_exited {
            return Err(self.shell_exited_error());
        }

        let (exit_tx, exit_rx) = oneshot::channel::<Result<ExitCode, AgentDriverError>>();
        let (start_tx, start_rx) = oneshot::channel::<Result<BlockId, AgentDriverError>>();

        // We should not be able to execute a command while we are still waiting on another one.
        // This is enforced by the caller by waiting on rx before continuing.
        if self.waiting_command.is_some() || self.pending_command_start.is_some() {
            return Err(AgentDriverError::InvalidRuntimeState);
        }

        let command_string = command.to_string();
        let command_id = Uuid::new_v4();
        let submitted_at = Instant::now();
        log::info!("Terminal command lifecycle: event=submitted command_id={command_id}");
        // Store a secret-redacted copy for shell-exit attribution: the text
        // flows into error reports (server task status, Sentry) if the shell
        // dies, so never retain the raw command here.
        let mut redacted_command = command_string.clone();
        redact_secrets(&mut redacted_command);
        self.last_command = Some(redacted_command);
        self.terminal_view.update(ctx, |terminal, ctx| {
            self.waiting_command = Some(WaitingCommand {
                exit_tx,
                started_block_id: None,
            });
            self.pending_command_start = Some((command_string, start_tx));
            terminal.execute_command_or_set_pending(command, ctx);
        });

        Ok(async move {
            let block_id = start_rx
                .await
                .map_err(|_| AgentDriverError::InvalidRuntimeState)??;
            log::info!(
                "Terminal command lifecycle: event=started command_id={command_id} block_id={block_id:?} elapsed_ms={}",
                submitted_at.elapsed().as_millis()
            );
            Ok(CommandHandle {
                exit_status_rx: exit_rx,
                block_id,
                command_id,
                submitted_at,
            })
        })
    }

    /// Execute a command through the active session's in-band command
    /// executor, without adding a block to the user-visible blocklist.
    ///
    /// Intended for silent probes (e.g. `test -d`) that the agent needs to
    /// drive through the terminal session (so they run against the correct
    /// filesystem, including inside a Docker sandbox) but should not clutter
    /// the user's command history.
    pub fn execute_silent_command(
        &self,
        command: String,
        ctx: &ModelContext<Self>,
    ) -> impl Future<Output = Result<CommandOutput, AgentDriverError>> + use<> {
        let session = self.terminal_view.read(ctx, |terminal, app| {
            terminal
                .active_block_session_id()
                .and_then(|id| terminal.sessions_model().as_ref(app).get(id))
        });
        async move {
            let session = session.ok_or(AgentDriverError::InvalidRuntimeState)?;
            session
                .execute_command(&command, None, None, ExecuteCommandOptions::default())
                .await
                .map_err(|e| {
                    log::warn!("silent command failed: {e:#}");
                    AgentDriverError::InvalidRuntimeState
                })
        }
    }

    /// Returns the shell type of the active terminal session, if known.
    pub fn active_session_shell_type(&self, ctx: &AppContext) -> Option<ShellType> {
        self.terminal_view
            .read(ctx, |terminal, app| terminal.active_session_shell_type(app))
    }

    /// Build the shell-aware `cd <escaped>` command for the active session.
    ///
    /// Shared between [`Self::cd`] and [`Self::cd_silent`] so both paths use
    /// the same [`ShellFamily::shell_escape`] logic (posix single-quoting,
    /// fish backslash, pwsh double-quote doubling) and don't drift.
    fn build_cd_command(&self, target: &str, ctx: &AppContext) -> String {
        let shell_family = self.terminal_view.read(ctx, |terminal, app| {
            terminal
                .active_session_shell_type(app)
                .map(ShellFamily::from)
                .unwrap_or(ShellFamily::Posix)
        });
        let escaped_target = shell_family.shell_escape(target);
        format!("cd {escaped_target}")
    }

    /// Change directory within the active terminal session.
    pub fn cd(
        &mut self,
        target: &str,
        ctx: &mut ModelContext<Self>,
    ) -> Result<
        impl Future<Output = Result<CommandHandle, AgentDriverError>> + use<>,
        AgentDriverError,
    > {
        let cd_command = self.build_cd_command(target, ctx);
        self.execute_command(&cd_command, ctx)
    }

    /// Change directory within the active terminal session, silently — no
    /// visible block is added to the user-facing blocklist.
    ///
    /// Uses the same shell-aware escaping as [`Self::cd`] but dispatches
    /// through [`Self::execute_silent_command`]. Intended for callers that
    /// need to position the session's CWD as an implementation detail of a
    /// larger setup step (e.g. positioning the session before running
    /// silent probes).
    pub fn cd_silent(
        &self,
        target: &str,
        ctx: &ModelContext<Self>,
    ) -> impl Future<Output = Result<CommandOutput, AgentDriverError>> + use<> {
        let cd_command = self.build_cd_command(target, ctx);
        self.execute_silent_command(cd_command, ctx)
    }

    /// The current working directory of the active terminal session, if known.
    #[allow(dead_code)]
    pub fn current_directory(&self, ctx: &AppContext) -> Option<PathBuf> {
        // TODO(ben): This should handle non-local paths.
        self.terminal_view
            .as_ref(ctx)
            .active_session_path_if_local(ctx)
    }

    /// Returns a future that resolves when the session has bootstrapped.
    ///
    /// The bootstrap result channel carries `Ok(())` on success or
    /// `Err(BootstrapError)` on failure. If the channel times out, the error
    /// is `BootstrapError::TimedOut`.
    pub fn wait_for_session_bootstrapped(
        &mut self,
    ) -> impl Future<Output = Result<(), BootstrapError>> + use<> {
        let bootstrap_rx = self.bootstrap_rx.take();

        async move {
            if let Some(rx) = bootstrap_rx {
                // Map channel cancellation (sender dropped without sending)
                // to InternalError — this shouldn't happen in practice.
                let inner = async move { rx.await.unwrap_or(Err(BootstrapError::InternalError)) };
                match inner.with_timeout(TERMINAL_SESSION_BOOTSTRAP_TIMEOUT).await {
                    Ok(result) => result,
                    Err(_timeout) => Err(BootstrapError::TimedOut),
                }
            } else {
                // bootstrap_rx already consumed — shouldn't happen in normal flow.
                Err(BootstrapError::InternalError)
            }
        }
    }
}

/// The first DFA match returned by
/// [`TerminalDriver::find_first_match_in_block_output`].
///
/// `matched_text` is the exact substring from the grid (no ANSI escapes),
/// used by the harness output monitor to map the hit back to the originating
/// pattern. `excerpt` is the full row(s) containing the match, also as
/// plaintext, suitable for surfacing in user-visible error messages.
#[derive(Debug, Clone)]
pub(crate) struct BlockOutputMatch {
    pub matched_text: String,
    pub excerpt: String,
}

/// A handle to a running terminal command.
///
/// Resolves to the command's [`ExitCode`] when the block completes.
/// Also carries the [`BlockId`] so callers can retrieve the block snapshot
/// after completion.
pub(crate) struct CommandHandle {
    exit_status_rx: oneshot::Receiver<Result<ExitCode, AgentDriverError>>,
    block_id: BlockId,
    command_id: Uuid,
    submitted_at: Instant,
}

impl CommandHandle {
    /// The block ID of the command that was executed.
    pub fn block_id(&self) -> &BlockId {
        &self.block_id
    }
}

impl Future for CommandHandle {
    type Output = Result<ExitCode, AgentDriverError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let result = Pin::new(&mut self.exit_status_rx)
            .poll(cx)
            .map(|result| match result {
                Ok(exit_status) => exit_status,
                Err(_) => Err(AgentDriverError::InvalidRuntimeState),
            });
        if let Poll::Ready(status) = &result {
            log::info!(
                "Terminal command lifecycle: event=finished command_id={} block_id={:?} elapsed_ms={} exit_code={:?} exit_status_received={}",
                self.command_id,
                self.block_id,
                self.submitted_at.elapsed().as_millis(),
                status.as_ref().ok().map(|code| code.value()),
                status.is_ok()
            );
        }
        result
    }
}

impl TerminalDriver {
    /// Handle terminal view events.
    fn handle_terminal_view_event(
        &mut self,
        event: &crate::terminal::view::Event,
        ctx: &mut ModelContext<Self>,
    ) {
        match event {
            crate::terminal::view::Event::SessionBootstrapped => {
                if let Some(tx) = self.bootstrap_tx.take() {
                    let _ = tx.send(Ok(()));
                }
            }
            crate::terminal::view::Event::PtySpawnFailed { reason } => {
                // Signal the bootstrap waiter immediately so it doesn't wait
                // for the full 60 s timeout when a spawn failure has already
                // been confirmed.
                if let Some(tx) = self.bootstrap_tx.take() {
                    let _ = tx.send(Err(BootstrapError::PtySpawnFailed {
                        reason: Some(reason.clone()),
                    }));
                }
            }
            crate::terminal::view::Event::Exited => {
                log::warn!(
                    "Terminal command lifecycle: event=shell_exited pending_start={} waiting_exit={}",
                    self.pending_command_start.is_some(),
                    self.waiting_command.is_some()
                );
                // The shell process exited before bootstrap completed —
                // cancel the wait immediately rather than sitting out the
                // full 60 s timeout. No specific reason is known at this
                // point; the logs will have details.
                if let Some(tx) = self.bootstrap_tx.take() {
                    let _ = tx.send(Err(BootstrapError::PtySpawnFailed { reason: None }));
                }

                // The shell is gone: no further command can start or finish.
                // Fail any in-flight command (e.g. an environment setup
                // command) with the shell-exit error so the run reports the
                // failure instead of hanging until the sandbox is killed.
                self.shell_exited = true;
                if let Some((_, sender)) = self.pending_command_start.take() {
                    let _ = sender.send(Err(self.shell_exited_error()));
                }
                if let Some(command) = self.waiting_command.take() {
                    let _ = command.exit_tx.send(Err(self.shell_exited_error()));
                }
            }
            crate::terminal::view::Event::SlowBootstrap => {
                ctx.emit(TerminalDriverEvent::SlowBootstrap);
            }
            crate::terminal::view::Event::ExecuteCommand(event) => {
                if let Some((_expected_command, sender)) = self
                    .pending_command_start
                    .take_if(|(cmd, _)| *cmd == event.command)
                {
                    let block_id = self.terminal_view.read(ctx, |terminal, _| {
                        terminal.model.lock().block_list().active_block_id().clone()
                    });
                    if let Some(command) = self.waiting_command.as_mut() {
                        command.started_block_id = Some(block_id.clone());
                    }
                    let _ = sender.send(Ok(block_id));
                }
            }
            crate::terminal::view::Event::BlockCompleted { block, .. } => {
                // Missing preexec hooks leave did_execute false even for commands that ran.
                if let Some(command) = self
                    .waiting_command
                    .take_if(|command| command.started_block_id.as_ref() == Some(&block.id))
                {
                    let _ = command.exit_tx.send(Ok(block.exit_code));
                } else if let Some(command) = self.waiting_command.as_ref() {
                    log::info!(
                        "Ignoring terminal block completion while waiting for command: completed_block_id={:?} started_block_id={:?}",
                        block.id,
                        command.started_block_id
                    );
                }
            }
            _ => (),
        }
    }
}
