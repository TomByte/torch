use torchui_core::{Entity, ModelContext, SingletonEntity};

/// Execution mode that Torch is running under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Torch is running as a normal desktop app.
    App,
    /// Torch is running as a CLI.
    Sdk,
    /// Torch is running as the remote server daemon.
    RemoteServerDaemon,
}

impl ExecutionMode {
    /// Whether a CLI-based MCP server can fall back to inheriting this process's PATH when
    /// no explicit `mcp_execution_path` setting is available.
    ///
    /// The desktop app keeps requiring a shell-derived path, so a failed MCP spawn surfaces
    /// as an actionable toast instead of silently launching with the wrong PATH. The SDK CLI
    /// receives an authoritative PATH from its launcher (an interactive shell
    /// or a CLI invocation) before Torch starts, so inheriting it is safe and is the only PATH
    /// available to a fresh SDK process before terminal bootstrap populates
    /// `mcp_execution_path`. The remote server daemon is headless and long-lived with no user
    /// present to open a terminal and populate that setting, and no window to show the
    /// alternative's failure toast in, so it also inherits; since inheritance only ever fills
    /// in a missing path rather than overriding a configured one, that's a better failure mode
    /// than refusing to start the server.
    pub fn can_inherit_process_path_for_mcp(&self) -> bool {
        match self {
            ExecutionMode::App => false,
            ExecutionMode::Sdk | ExecutionMode::RemoteServerDaemon => true,
        }
    }
}

/// Model tracking the mode that Torch is running in.
///
/// This gates functionality that's disabled when Torch is running in SDK mode.
#[derive(Clone, Debug)]
pub struct AppExecutionMode {
    mode: ExecutionMode,
    is_sandboxed: bool,
}

impl AppExecutionMode {
    /// Create an `AppExecutionMode` model with the execution mode set.
    pub fn new(mode: ExecutionMode, is_sandboxed: bool, _ctx: &mut ModelContext<Self>) -> Self {
        Self { mode, is_sandboxed }
    }

    /// True if running as an interactive app client.
    fn is_app(&self) -> bool {
        matches!(self.mode, ExecutionMode::App)
    }

    /// Whether Active AI features are allowed in this execution mode.
    ///
    /// Active AI should only run in interactive clients, where there's a user
    /// to engage with it.
    pub fn allows_active_ai(&self) -> bool {
        self.is_app()
    }

    /// Whether the app can sync user preferences to the cloud. This does not gate
    /// modifying preferences locally.
    pub fn can_sync_preferences(&self) -> bool {
        self.is_app()
    }

    /// Whether the app can save and restore sessions.
    pub fn can_save_session(&self) -> bool {
        self.is_app()
    }

    /// Whether the app can *automatically* update. This does not prevent manual updates.
    pub fn can_autoupdate(&self) -> bool {
        self.is_app() && cfg!(not(target_family = "wasm"))
    }

    /// Whether the app can automatically start MCP servers from the previous session.
    pub fn can_autostart_mcp_servers(&self) -> bool {
        self.is_app()
    }

    /// Whether the app can show interactive onboarding UIs (e.g. the onboarding
    /// callout tutorial). Onboarding requires a user to interact with it, so it
    /// is disabled in headless modes like SDK/CLI.
    pub fn can_show_onboarding(&self) -> bool {
        self.is_app()
    }

    /// Whether the app can sync agent conversations (tasks and cloud conversation metadata).
    /// In CLI mode, we don't need this data since there's no user viewing it.
    pub fn can_fetch_agent_runs_for_management(&self) -> bool {
        self.is_app()
    }

    /// If true, the app is running autonomously, without a user present.
    /// Wherever possible, prefer more targeted capability checks like
    /// [`Self::can_autostart_mcp_servers`].
    pub fn is_autonomous(&self) -> bool {
        matches!(
            self.mode,
            ExecutionMode::Sdk | ExecutionMode::RemoteServerDaemon
        )
    }

    /// Whether a CLI-based MCP server can fall back to inheriting this process's PATH when
    /// no explicit `mcp_execution_path` setting is available.
    pub fn can_inherit_process_path_for_mcp(&self) -> bool {
        self.mode.can_inherit_process_path_for_mcp()
    }

    /// If true, Torch is running in a sandbox like a Docker container or VM, rather than directly
    /// on a user machine.
    pub fn is_sandboxed(&self) -> bool {
        self.is_sandboxed
    }
}

impl Entity for AppExecutionMode {
    type Event = ();
}

impl SingletonEntity for AppExecutionMode {}
