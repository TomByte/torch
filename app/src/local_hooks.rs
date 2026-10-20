//! Opt-in, dependency-free CLI status integration shipped inside Torch.
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

const MARKER: &str = "Torch session status";
const EVENTS: &[&str] = &[
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
// The launching terminal supplies its executable, so updates and relocated apps need no rewrite.
const COMMAND: &str = r#"if [ "$TORCH_TERMINAL" = 1 ] && [ -x "$TORCH_HOOK_EXECUTABLE" ]; then "$TORCH_HOOK_EXECUTABLE" --torch-claude-status-hook; fi; exit 0"#;

/// Runs before GUI, logging, configuration, or network initialization.
pub fn run_if_requested() -> bool {
    if std::env::args_os().nth(1).as_deref()
        != Some(std::ffi::OsStr::new("--torch-claude-status-hook"))
    {
        return false;
    }
    if std::env::var("TORCH_TERMINAL").as_deref() == Ok("1") {
        let _ = write_status(std::io::stdin().lock(), std::io::stdout().lock());
    }
    true
}

fn write_status(reader: impl Read, mut writer: impl Write) -> Result<()> {
    let mut input = Vec::new();
    reader.take(1024 * 1024 + 1).read_to_end(&mut input)?;
    ensure!(input.len() <= 1024 * 1024, "Hook input exceeds limit");
    let data: Value = serde_json::from_slice(&input)?;
    if let Some(output) = status_output(&data) {
        serde_json::to_writer(&mut writer, &output)?;
        writer.write_all(b"\n")?;
    }
    Ok(())
}

fn status_output(data: &Value) -> Option<Value> {
    if data
        .get("agent_id")
        .is_some_and(|id| !id.is_null() && id != "")
    {
        return None;
    }
    let hook = data.get("hook_event_name")?.as_str()?;
    let event = match hook {
        "UserPromptSubmit" => "prompt_submit",
        "PermissionRequest" => "permission_request",
        "PostToolUse" | "PostToolUseFailure" => "tool_complete",
        "Stop" => "stop",
        "StopFailure" => "stop_failure",
        "Elicitation" => "question_asked",
        "ElicitationResult" => "permission_replied",
        "PreToolUse" if data["tool_name"] == "AskUserQuestion" => "question_asked",
        "PreToolUse" => "permission_replied",
        "Notification" => match data["notification_type"].as_str()? {
            "permission_prompt" => "permission_request",
            "elicitation_dialog" => "question_asked",
            "agent_needs_input" => "agent_needs_input",
            _ => return None,
        },
        _ => return None,
    };
    let mut body = json!({"v": 1, "agent": "claude", "event": event,
        "session_id": data["session_id"].as_str(), "cwd": data["cwd"].as_str()});
    if hook == "Stop" {
        for (source, target) in [
            ("background_tasks", "background_task_count"),
            ("session_crons", "session_cron_count"),
        ] {
            if let Some(items) = data[source].as_array() {
                body[target] = json!(items.len());
            }
        }
    }
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
    Some(json!({"terminalSequence": format!("\x1b]777;notify;warp://cli-agent;{body}\x07")}))
}

pub(crate) fn default_config_dir() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".claude")))
}

fn updated_settings(mut settings: Value, install: bool) -> Result<Value> {
    let root = settings
        .as_object_mut()
        .context("Claude settings must be a JSON object")?;
    if !root.contains_key("hooks") && !install {
        return Ok(settings);
    }
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("Claude hooks must be a JSON object")?;
    for event in EVENTS {
        let mut groups = match hooks.get(*event) {
            Some(value) => value
                .as_array()
                .context("Claude hook groups must be arrays")?
                .clone(),
            None => Vec::new(),
        };
        let mut retained = Vec::new();
        for mut group in groups {
            let entries = group
                .get_mut("hooks")
                .and_then(Value::as_array_mut)
                .context("Claude hook group must contain a hooks array")?;
            let previous_count = entries.len();
            entries.retain(|entry| entry["statusMessage"] != MARKER);
            if !entries.is_empty() || previous_count == 0 {
                retained.push(group);
            }
        }
        groups = retained;
        if install {
            groups.push(json!({"hooks": [{"type": "command", "command": COMMAND,
                "timeout": 3, "statusMessage": MARKER}]}));
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
pub(crate) fn configure(config_dir: &Path, install: bool) -> Result<Option<PathBuf>> {
    let path = config_dir.join("settings.json");
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        ensure!(
            !metadata.file_type().is_symlink(),
            "Choose the real configuration folder; settings.json is a symlink"
        );
    }
    let original = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("Could not read Claude settings"),
    };
    let settings: Value = serde_json::from_slice(original.as_deref().unwrap_or(b"{}"))
        .context("Claude settings contain invalid JSON; no changes made")?;
    let updated = updated_settings(settings.clone(), install)?;
    if updated == settings {
        return Ok(None);
    }
    fs::create_dir_all(config_dir)?;
    let backup = if let Some(bytes) = &original {
        let mut file = tempfile::Builder::new()
            .prefix("settings.json.torch-backup-")
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
        Err(error) => return Err(error).context("Could not recheck Claude settings"),
    };
    if latest != original {
        bail!("Claude settings changed during setup. Please retry.");
    }
    temporary.persist(&path)?;
    Ok(backup)
}

#[cfg(test)]
#[path = "local_hooks_tests.rs"]
mod tests;
