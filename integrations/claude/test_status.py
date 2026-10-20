import json
import unittest

from install import update_settings
from status import notification, output


class StatusTests(unittest.TestCase):
    def test_permission_and_question_events_need_attention(self):
        self.assertEqual(notification({"hook_event_name": "PermissionRequest"})["event"],
                         "permission_request")
        self.assertEqual(notification({"hook_event_name": "PreToolUse",
                                       "tool_name": "AskUserQuestion"})["event"],
                         "question_asked")

    def test_idle_reminder_does_not_override_completed_turn(self):
        self.assertIsNone(notification({"hook_event_name": "Notification",
                                       "notification_type": "idle_prompt"}))

    def test_subagent_completion_does_not_complete_parent(self):
        self.assertIsNone(notification({"hook_event_name": "Stop", "agent_id": "child"}))
        self.assertIsNone(notification({"hook_event_name": "SubagentStop"}))

    def test_terminal_sequence_contains_no_prompt_or_control_byte_injection(self):
        result = output({"hook_event_name": "UserPromptSubmit", "prompt": "private",
                         "cwd": "/tmp/hello\x07\x1b]52;clipboard", "session_id": "abc"})
        sequence = result["terminalSequence"]
        self.assertEqual(sequence.count("\x07"), 1)
        self.assertEqual(sequence.count("\x1b"), 1)
        event = json.loads(sequence.removeprefix("\x1b]777;notify;warp://cli-agent;")[:-1])
        self.assertEqual(event["event"], "prompt_submit")
        self.assertNotIn("private", sequence)
        self.assertNotIn("prompt", event)
        self.assertEqual(list(result), ["terminalSequence"])

    def test_completion_and_error_are_distinct(self):
        self.assertEqual(notification({"hook_event_name": "Stop"})["event"], "stop")
        self.assertEqual(notification({"hook_event_name": "StopFailure"})["event"], "stop_failure")

    def test_background_work_is_counted_without_forwarding_its_content(self):
        event = notification({"hook_event_name": "Stop",
                              "background_tasks": [{"command": "private command"}],
                              "session_crons": []})
        self.assertEqual(event["background_task_count"], 1)
        self.assertEqual(event["session_cron_count"], 0)
        self.assertNotIn("private", json.dumps(event))

    def test_install_is_idempotent_and_uninstall_preserves_existing_hooks(self):
        settings = {"permissions": {"defaultMode": "default"},
                    "hooks": {"Stop": [{"matcher": "", "hooks": [
                        {"type": "command", "command": "existing-hook"}]}]}}
        original = json.loads(json.dumps(settings))
        update_settings(settings, "python3 status.py")
        once = json.loads(json.dumps(settings))
        update_settings(settings, "python3 status.py")
        self.assertEqual(settings, once)
        update_settings(settings, "", uninstall=True)
        self.assertEqual(settings, original)


if __name__ == "__main__":
    unittest.main()
