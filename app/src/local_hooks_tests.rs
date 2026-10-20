use super::*;

fn event(data: Value) -> Value {
    let output = status_output(&data).unwrap();
    let sequence = output["terminalSequence"].as_str().unwrap();
    serde_json::from_str(
        sequence
            .strip_prefix("\x1b]777;notify;warp://cli-agent;")
            .unwrap()
            .strip_suffix('\x07')
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn lifecycle_distinguishes_attention_completion_and_errors() {
    for (hook, expected) in [
        ("UserPromptSubmit", "prompt_submit"),
        ("PermissionRequest", "permission_request"),
        ("Stop", "stop"),
        ("StopFailure", "stop_failure"),
        ("Elicitation", "question_asked"),
        ("ElicitationResult", "permission_replied"),
    ] {
        assert_eq!(event(json!({"hook_event_name": hook}))["event"], expected);
    }
    assert_eq!(
        event(json!({"hook_event_name":"PreToolUse", "tool_name":"AskUserQuestion"}))["event"],
        "question_asked"
    );
    assert_eq!(
        event(json!({"hook_event_name":"PreToolUse", "tool_name":"Bash"}))["event"],
        "permission_replied"
    );
}

#[test]
fn idle_and_child_events_do_not_overwrite_parent_status() {
    for input in [
        json!({"hook_event_name":"Notification", "notification_type":"idle_prompt"}),
        json!({"hook_event_name":"Stop", "agent_id":"child"}),
        json!({"hook_event_name":"SubagentStop"}),
        json!(null),
    ] {
        assert!(status_output(&input).is_none());
    }
}

#[test]
fn metadata_cannot_inject_terminal_controls_or_forward_private_content() {
    let input = json!({"hook_event_name":"Stop", "cwd":"/tmp/hello\u{7}\u{1b}\u{9c}🦀", "session_id":"session",
        "prompt":"private prompt", "transcript_path":"secret", "background_tasks":[{"command":"private command"}], "session_crons":[]});
    let output = status_output(&input).unwrap();
    let sequence = output["terminalSequence"].as_str().unwrap();
    assert_eq!(sequence.matches('\x1b').count(), 1);
    assert_eq!(sequence.matches('\x07').count(), 1);
    assert!(sequence.is_ascii());
    assert!(!sequence.contains("private"));
    assert!(!sequence.contains("secret"));
    let body = event(input.clone());
    assert_eq!(body["cwd"], input["cwd"]);
    assert_eq!(body["background_task_count"], 1);
    assert_eq!(body["session_cron_count"], 0);
    assert_eq!(output.as_object().unwrap().len(), 1);
}

#[test]
fn invalid_and_oversized_input_produce_no_output() {
    for input in [b"not json".to_vec(), vec![b' '; 1024 * 1024 + 1]] {
        let mut output = Vec::new();
        assert!(write_status(input.as_slice(), &mut output).is_err());
        assert!(output.is_empty());
    }
}

#[test]
fn install_repairs_legacy_hooks_and_removal_preserves_other_configuration() {
    let original = json!({"permissions":{"defaultMode":"default"}, "enabledPlugins":{"example":true},
        "hooks":{"Stop":[{"matcher":"", "hooks":[{"type":"command", "command":"other-hook"}]}, {"matcher":"unrelated", "hooks":[]}],
        "FutureEvent":[{"hooks":[{"type":"command", "command":"future-hook"}]}]}});
    let mut legacy = original.clone();
    legacy["hooks"]["Stop"][0]["hooks"]
        .as_array_mut()
        .unwrap()
        .push(json!({"statusMessage":MARKER, "command":"/old/python /old/checkout/status.py"}));
    let installed = updated_settings(legacy, true).unwrap();
    assert!(!installed.to_string().contains("/old/"));
    assert_eq!(
        updated_settings(installed.clone(), true).unwrap(),
        installed
    );
    assert_eq!(updated_settings(installed, false).unwrap(), original);
}

#[test]
fn installation_is_private_atomic_and_makes_exact_backup_in_custom_folder() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("another user ' config 🦀");
    fs::create_dir_all(&config).unwrap();
    let original = b"{\n  \"permissions\": {\"defaultMode\": \"default\"}\n}\n";
    fs::write(config.join("settings.json"), original).unwrap();
    let backup = configure(&config, true).unwrap().unwrap();
    assert_eq!(fs::read(backup).unwrap(), original);
    assert!(configure(&config, true).unwrap().is_none());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(config.join("settings.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    configure(&config, false).unwrap();
    let restored: Value =
        serde_json::from_slice(&fs::read(config.join("settings.json")).unwrap()).unwrap();
    assert_eq!(restored["permissions"], json!({"defaultMode":"default"}));
    assert_eq!(restored["hooks"], json!({}));
}

#[test]
fn malformed_settings_are_never_replaced() {
    let dir = tempfile::tempdir().unwrap();
    for raw in [
        "broken",
        "[]",
        r#"{"hooks": []}"#,
        r#"{"hooks":{"Stop":[{}]}}"#,
    ] {
        fs::write(dir.path().join("settings.json"), raw).unwrap();
        assert!(configure(dir.path(), true).is_err());
        assert_eq!(
            fs::read_to_string(dir.path().join("settings.json")).unwrap(),
            raw
        );
    }
}

#[test]
fn remove_without_install_does_not_create_files() {
    let dir = tempfile::tempdir().unwrap();
    let absent = dir.path().join("absent");
    assert!(configure(&absent, false).unwrap().is_none());
    assert!(!absent.exists());
}

#[cfg(unix)]
#[test]
fn hook_command_resolves_each_installation_path_and_is_quiet_elsewhere() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("Torch ' $(bad) 🦀");
    fs::write(
        &binary,
        "#!/bin/sh\n[ \"$1\" = --torch-claude-status-hook ] || exit 1\nprintf invoked\n",
    )
    .unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
    for (marker, expected) in [("1", "invoked"), ("", "")] {
        let result = Command::new("sh")
            .args(["-c", COMMAND])
            .env("TORCH_TERMINAL", marker)
            .env("TORCH_HOOK_EXECUTABLE", &binary)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(result.status.success());
        assert_eq!(result.stdout, expected.as_bytes());
        assert!(result.stderr.is_empty());
    }
    fs::remove_file(&binary).unwrap();
    let result = Command::new("sh")
        .args(["-c", COMMAND])
        .env("TORCH_TERMINAL", "1")
        .env("TORCH_HOOK_EXECUTABLE", binary)
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(result.stdout.is_empty());
    assert!(result.stderr.is_empty());
}

#[cfg(unix)]
#[test]
fn symlinked_settings_are_not_replaced_or_followed() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("real.json");
    fs::write(&target, "{}").unwrap();
    std::os::unix::fs::symlink(&target, dir.path().join("settings.json")).unwrap();
    assert!(configure(dir.path(), true).is_err());
    assert_eq!(fs::read_to_string(target).unwrap(), "{}");
    assert!(
        fs::symlink_metadata(dir.path().join("settings.json"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
