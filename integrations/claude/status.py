#!/usr/bin/env python3
"""Forward Claude lifecycle events to Torch's local terminal status protocol."""

import json
import os
import sys


def notification(data):
    # Subagents share the terminal; their Stop events must not finish the parent.
    if data.get("agent_id"):
        return None
    hook = data.get("hook_event_name")
    events = {
        "UserPromptSubmit": "prompt_submit",
        "PermissionRequest": "permission_request",
        "PostToolUse": "tool_complete",
        "PostToolUseFailure": "tool_complete",
        "Stop": "stop",
        "StopFailure": "stop_failure",
        "Elicitation": "question_asked",
        "ElicitationResult": "permission_replied",
    }
    event = events.get(hook)
    if hook == "PreToolUse":
        event = "question_asked" if data.get("tool_name") == "AskUserQuestion" else "permission_replied"
    elif hook == "Notification":
        event = {
            "permission_prompt": "permission_request",
            "elicitation_dialog": "question_asked",
            "agent_needs_input": "agent_needs_input",
        }.get(data.get("notification_type"))
    if event is None:
        return None
    # Deliberately omit prompts, tool inputs, responses and transcript paths.
    result = {
        "v": 1,
        "agent": "claude",
        "event": event,
        "session_id": data.get("session_id"),
        "cwd": data.get("cwd"),
    }
    if hook == "Stop":
        for source, target in (
            ("background_tasks", "background_task_count"),
            ("session_crons", "session_cron_count"),
        ):
            if isinstance(data.get(source), list):
                result[target] = len(data[source])
    return result


def output(data):
    event = notification(data)
    if event is None:
        return {}
    body = json.dumps(event, ensure_ascii=True, separators=(",", ":"))
    # Claude writes this to its own terminal, including when hooks have no /dev/tty.
    return {"terminalSequence": "\x1b]777;notify;warp://cli-agent;" + body + "\x07"}


def main():
    if os.environ.get("TORCH_TERMINAL") != "1":
        return
    try:
        data = json.load(sys.stdin)
        if isinstance(data, dict):
            print(json.dumps(output(data)))
    except (ValueError, TypeError, OSError):
        # Status reporting must never block or alter a Claude turn.
        pass


if __name__ == "__main__":
    main()
