//! Opt-in, dependency-free CLI status integration shipped inside Torch.
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

use crate::terminal::CLIAgent;

const MARKER: &str = "Torch session status";
const CLAUDE_EVENTS: &[&str] = &[
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "PostToolUseFailure",
    "Stop",
    "StopFailure",
    "Notification",
    "Elicitation",
    "ElicitationResult",
];
const CODEX_EVENTS: &[&str] = &[
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "Stop",
    "Interrupt",
];
// The launching terminal supplies its executable, so updates and relocated apps need no rewrite.
const CLAUDE_COMMAND: &str = r#"if [ "$TORCH_TERMINAL" = 1 ] && [ -x "$TORCH_HOOK_EXECUTABLE" ]; then "$TORCH_HOOK_EXECUTABLE" --torch-claude-status-hook; fi; exit 0"#;
// Codex runs hooks with the user's login shell, which may not be POSIX (e.g. fish).
const CODEX_COMMAND: &str = r#"/bin/sh -c 'if [ "$TORCH_TERMINAL" = 1 ] && [ -x "$TORCH_HOOK_EXECUTABLE" ]; then "$TORCH_HOOK_EXECUTABLE" --torch-codex-status-hook; fi; exit 0'"#;
const INPUT_LIMIT: usize = 1024 * 1024;

/// A coding agent whose lifecycle hooks Torch can configure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusAgent {
    Claude,
    Codex,
}

/// Whether an agent's configuration contains Torch's current hooks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SetupState {
    Installed,
    /// Torch hooks exist but differ from the ones this version installs.
    Outdated,
    Missing,
}

impl StatusAgent {
    pub(crate) fn for_cli_agent(agent: CLIAgent) -> Option<Self> {
        match agent {
            CLIAgent::Claude => Some(Self::Claude),
            CLIAgent::Codex => Some(Self::Codex),
            CLIAgent::Gemini
            | CLIAgent::OpenCode
            | CLIAgent::Grok
            | CLIAgent::Muse
            | CLIAgent::Amp
            | CLIAgent::Droid
            | CLIAgent::Copilot
            | CLIAgent::Pi
            | CLIAgent::OhMyPi
            | CLIAgent::Auggie
            | CLIAgent::CursorCli
            | CLIAgent::Hermes
            | CLIAgent::Kiro
            | CLIAgent::Goose
            | CLIAgent::Vibe
            | CLIAgent::Antigravity
            | CLIAgent::Unknown => None,
        }
    }

    pub(crate) fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
        }
    }

    fn hook_flag(self) -> &'static str {
        match self {
            Self::Claude => "--torch-claude-status-hook",
            Self::Codex => "--torch-codex-status-hook",
        }
    }

    fn events(self) -> &'static [&'static str] {
        match self {
            Self::Claude => CLAUDE_EVENTS,
            Self::Codex => CODEX_EVENTS,
        }
    }

    fn command(self) -> &'static str {
        match self {
            Self::Claude => CLAUDE_COMMAND,
            Self::Codex => CODEX_COMMAND,
        }
    }

    fn config_file(self) -> &'static str {
        match self {
            Self::Claude => "settings.json",
            Self::Codex => "hooks.json",
        }
    }

    pub(crate) fn default_config_dir(self) -> Option<PathBuf> {
        let (variable, folder) = match self {
            Self::Claude => ("CLAUDE_CONFIG_DIR", ".claude"),
            Self::Codex => ("CODEX_HOME", ".codex"),
        };
        std::env::var_os(variable)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|home| home.join(folder)))
    }

    /// Maps a hook payload to a Torch CLI agent event name.
    fn status_event(self, data: &Value) -> Option<&'static str> {
        let hook = data.get("hook_event_name")?.as_str()?;
        let event = match (self, hook) {
            (_, "UserPromptSubmit") => "prompt_submit",
            (_, "PermissionRequest") => "permission_request",
            (_, "PostToolUse" | "PostToolUseFailure") => "tool_complete",
            (Self::Claude, "Stop") => "stop",
            // Torch has no cancelled state; an interrupted turn is over either way.
            (Self::Codex, "Stop" | "Interrupt") => "stop",
            (Self::Claude, "StopFailure") => "stop_failure",
            (Self::Claude, "Elicitation") => "question_asked",
            (Self::Claude, "ElicitationResult") => "permission_replied",
            (Self::Claude, "PreToolUse") if data["tool_name"] == "AskUserQuestion" => {
                "question_asked"
            }
            (_, "PreToolUse") => "permission_replied",
            (Self::Claude, "Notification") => match data["notification_type"].as_str()? {
                "permission_prompt" => "permission_request",
                "elicitation_dialog" => "question_asked",
                "agent_needs_input" => "agent_needs_input",
                _ => return None,
            },
            _ => return None,
        };
        Some(event)
    }

    fn agent_name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

/// Runs before GUI, logging, configuration, or network initialization.
pub fn run_if_requested() -> bool {
    let Some(flag) = std::env::args_os().nth(1) else {
        return false;
    };
    let Some(agent) = [StatusAgent::Claude, StatusAgent::Codex]
        .into_iter()
        .find(|agent| flag == agent.hook_flag())
    else {
        return false;
    };
    if std::env::var("TORCH_TERMINAL").as_deref() == Ok("1") {
        let _ = match agent {
            StatusAgent::Claude => write_status(std::io::stdin().lock(), std::io::stdout().lock()),
            StatusAgent::Codex => write_codex_status(std::io::stdin().lock()),
        };
    }
    true
}

fn read_input(reader: impl Read) -> Result<Value> {
    let mut input = Vec::new();
    reader
        .take(INPUT_LIMIT as u64 + 1)
        .read_to_end(&mut input)?;
    ensure!(input.len() <= INPUT_LIMIT, "Hook input exceeds limit");
    Ok(serde_json::from_slice(&input)?)
}

fn write_status(reader: impl Read, mut writer: impl Write) -> Result<()> {
    let data = read_input(reader)?;
    if let Some(output) = status_output(&data) {
        serde_json::to_writer(&mut writer, &output)?;
        writer.write_all(b"\n")?;
    }
    Ok(())
}

/// Claude forwards `terminalSequence` hook output to its terminal.
fn status_output(data: &Value) -> Option<Value> {
    let body = status_body(StatusAgent::Claude, data)?;
    Some(json!({"terminalSequence": terminal_sequence(&body)}))
}

/// Codex runs hooks in a new session with captured output, so the sequence is written straight
/// to the terminal of the Codex process that ran the hook.
fn write_codex_status(reader: impl Read) -> Result<()> {
    let data = read_input(reader)?;
    let Some(body) = status_body(StatusAgent::Codex, &data) else {
        return Ok(());
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let path = agent_terminal::path().context("Codex has no terminal")?;
        let mut terminal = fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(path)?;
        terminal.write_all(terminal_sequence(&body).as_bytes())?;
    }
    #[cfg(not(unix))]
    let _ = body;
    Ok(())
}

fn status_body(agent: StatusAgent, data: &Value) -> Option<Value> {
    if data
        .get("agent_id")
        .is_some_and(|id| !id.is_null() && id != "")
    {
        return None;
    }
    let event = agent.status_event(data)?;
    let mut body = json!({"v": 1, "agent": agent.agent_name(), "event": event,
        "session_id": data["session_id"].as_str(), "cwd": data["cwd"].as_str()});
    if event == "permission_request"
        && let Some(tool) = data["tool_name"].as_str()
    {
        body["tool_name"] = json!(tool);
    }
    if agent == StatusAgent::Claude && data["hook_event_name"] == "Stop" {
        for (source, target) in [
            ("background_tasks", "background_task_count"),
            ("session_crons", "session_cron_count"),
        ] {
            if let Some(items) = data[source].as_array() {
                body[target] = json!(items.len());
            }
        }
    }
    Some(body)
}

fn terminal_sequence(body: &Value) -> String {
    // Escape non-ASCII too: terminal C1 control bytes must never terminate the OSC payload.
    let body: String = body
        .to_string()
        .chars()
        .map(|ch| {
            if ch.is_ascii() {
                ch.to_string()
            } else {
                let mut units = [0; 2];
                ch.encode_utf16(&mut units)
                    .iter()
                    .map(|unit| format!("\\u{unit:04x}"))
                    .collect()
            }
        })
        .collect();
    format!("\x1b]777;notify;warp://cli-agent;{body}\x07")
}

#[cfg(unix)]
mod agent_terminal {
    use std::fs;
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    use std::path::PathBuf;

    /// Hooks run as children of `sh -c`, itself possibly under the agent's login shell.
    const MAX_ANCESTORS: usize = 4;

    /// The controlling terminal of the nearest ancestor that has one.
    pub(super) fn path() -> Option<PathBuf> {
        let mut pid = std::os::unix::process::parent_id();
        for _ in 0..MAX_ANCESTORS {
            let (device, parent) = process_terminal(pid)?;
            if let Some(device) = device {
                return device_path(device);
            }
            if parent <= 1 {
                return None;
            }
            pid = parent;
        }
        None
    }

    /// Returns the process's controlling terminal device, if any, and its parent.
    #[cfg(target_os = "macos")]
    pub(super) fn process_terminal(pid: u32) -> Option<(Option<u64>, u32)> {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        // SAFETY: `info` is a writable buffer of exactly `size` bytes.
        let written = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDTBSDINFO,
                0,
                (&raw mut info).cast(),
                size,
            )
        };
        if written != size {
            return None;
        }
        // NODEV is all bits set.
        let device = (info.e_tdev != 0 && info.e_tdev != u32::MAX).then_some(info.e_tdev.into());
        Some((device, info.pbi_ppid))
    }

    #[cfg(target_os = "linux")]
    pub(super) fn process_terminal(pid: u32) -> Option<(Option<u64>, u32)> {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // Fields after the parenthesised command name: state, ppid, pgrp, session, tty_nr.
        let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
        let parent = fields.get(1)?.parse().ok()?;
        let device: u64 = fields.get(4)?.parse().ok()?;
        Some(((device != 0).then_some(device), parent))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    pub(super) fn process_terminal(_pid: u32) -> Option<(Option<u64>, u32)> {
        None
    }

    fn device_path(device: u64) -> Option<PathBuf> {
        let (directory, prefix) = if cfg!(target_os = "macos") {
            ("/dev", "ttys")
        } else {
            ("/dev/pts", "")
        };
        fs::read_dir(directory)
            .ok()?
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
            .map(|entry| entry.path())
            .find(|path| {
                fs::metadata(path).is_ok_and(|metadata| {
                    metadata.file_type().is_char_device() && metadata.rdev() == device
                })
            })
    }
}

fn is_torch_entry(entry: &Value) -> bool {
    entry["statusMessage"] == MARKER
}

fn hook_entries<'a>(settings: &'a Value, event: &str) -> impl Iterator<Item = &'a Value> + use<'a> {
    settings["hooks"][event]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
}

fn setup_state_of(agent: StatusAgent, settings: &Value) -> SetupState {
    let current = agent.events().iter().all(|event| {
        hook_entries(settings, event)
            .any(|entry| is_torch_entry(entry) && entry["command"] == agent.command())
    });
    if current {
        return SetupState::Installed;
    }
    let any_torch_hook = settings["hooks"].as_object().is_some_and(|hooks| {
        hooks
            .keys()
            .any(|event| hook_entries(settings, event).any(is_torch_entry))
    });
    if any_torch_hook {
        SetupState::Outdated
    } else {
        SetupState::Missing
    }
}

/// Reads the agent's configuration without changing it.
pub(crate) fn setup_state(agent: StatusAgent, config_dir: &Path) -> Result<SetupState> {
    let bytes = match fs::read(config_dir.join(agent.config_file())) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SetupState::Missing);
        }
        Err(error) => return Err(error).context("Could not read agent settings"),
    };
    let settings: Value = serde_json::from_slice(&bytes).context("Agent settings are invalid")?;
    Ok(setup_state_of(agent, &settings))
}

fn updated_settings(agent: StatusAgent, mut settings: Value, install: bool) -> Result<Value> {
    let root = settings
        .as_object_mut()
        .context("Agent settings must be a JSON object")?;
    if !root.contains_key("hooks") && !install {
        return Ok(settings);
    }
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("Agent hooks must be a JSON object")?;
    for event in agent.events() {
        let mut groups = match hooks.get(*event) {
            Some(value) => value
                .as_array()
                .context("Agent hook groups must be arrays")?
                .clone(),
            None => Vec::new(),
        };
        let mut retained = Vec::new();
        for mut group in groups {
            let entries = group
                .get_mut("hooks")
                .and_then(Value::as_array_mut)
                .context("Agent hook group must contain a hooks array")?;
            let previous_count = entries.len();
            entries.retain(|entry| !is_torch_entry(entry));
            if !entries.is_empty() || previous_count == 0 {
                retained.push(group);
            }
        }
        groups = retained;
        if install {
            groups.push(
                json!({"hooks": [{"type": "command", "command": agent.command(),
                "timeout": 3, "statusMessage": MARKER}]}),
            );
        }
        if groups.is_empty() {
            hooks.remove(*event);
        } else {
            hooks.insert((*event).to_owned(), json!(groups));
        }
    }
    Ok(settings)
}

/// Returns the private backup path, if an existing file was changed.
pub(crate) fn configure(
    agent: StatusAgent,
    config_dir: &Path,
    install: bool,
) -> Result<Option<PathBuf>> {
    let file_name = agent.config_file();
    let path = config_dir.join(file_name);
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        ensure!(
            !metadata.file_type().is_symlink(),
            "Choose the real configuration folder; {file_name} is a symlink"
        );
    }
    let original = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("Could not read agent settings"),
    };
    let settings: Value = serde_json::from_slice(original.as_deref().unwrap_or(b"{}"))
        .with_context(|| format!("{file_name} contains invalid JSON; no changes made"))?;
    let updated = updated_settings(agent, settings.clone(), install)?;
    if updated == settings {
        return Ok(None);
    }
    fs::create_dir_all(config_dir)?;
    let backup = if let Some(bytes) = &original {
        let mut file = tempfile::Builder::new()
            .prefix(&format!("{file_name}.torch-backup-"))
            .tempfile_in(config_dir)?;
        file.write_all(bytes)?;
        file.as_file().sync_all()?;
        Some(file.keep()?.1)
    } else {
        None
    };
    let mut temporary = tempfile::NamedTempFile::new_in(config_dir)?;
    serde_json::to_writer_pretty(&mut temporary, &updated)?;
    temporary.write_all(b"\n")?;
    temporary.as_file().sync_all()?;
    let latest = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("Could not recheck agent settings"),
    };
    if latest != original {
        bail!("{file_name} changed during setup. Please retry.");
    }
    temporary.persist(&path)?;
    Ok(backup)
}

#[cfg(test)]
#[path = "local_hooks_tests.rs"]
mod tests;
