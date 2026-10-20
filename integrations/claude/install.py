#!/usr/bin/env python3
"""Install or remove Torch's local status hooks without replacing other settings."""

import argparse
import datetime
import json
import os
from pathlib import Path
import shlex
import shutil
import sys

EVENTS = (
    "UserPromptSubmit", "PreToolUse", "PermissionRequest", "PostToolUse",
    "PostToolUseFailure", "Stop", "StopFailure", "Notification", "Elicitation",
    "ElicitationResult",
)
MARKER = "Torch session status"


def update_settings(settings, command, uninstall=False):
    hooks = settings.setdefault("hooks", {})
    for event in EVENTS:
        groups = []
        for group in hooks.get(event, []):
            remaining = [hook for hook in group.get("hooks", [])
                         if hook.get("statusMessage") != MARKER]
            if remaining:
                groups.append({**group, "hooks": remaining})
        if not uninstall:
            groups.append({"hooks": [{
                "type": "command", "command": command, "timeout": 3,
                "statusMessage": MARKER,
            }]})
        if groups:
            hooks[event] = groups
        else:
            hooks.pop(event, None)
    return settings


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--uninstall", action="store_true")
    parser.add_argument("--config-dir", type=Path,
                        default=Path(os.environ.get("CLAUDE_CONFIG_DIR", Path.home() / ".claude")))
    args = parser.parse_args()
    config_dir = args.config_dir.expanduser().resolve()
    settings_path = config_dir / "settings.json"
    raw = settings_path.read_text() if settings_path.exists() else "{}"
    settings = json.loads(raw)
    destination = config_dir / "torch" / "status.py"
    command = f"{shlex.quote(sys.executable)} {shlex.quote(str(destination))}"
    updated = update_settings(settings, command, args.uninstall)
    config_dir.mkdir(parents=True, exist_ok=True)
    if settings_path.exists():
        stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S-%f")
        backup = settings_path.with_name(f"settings.json.torch-backup-{stamp}")
        shutil.copy2(settings_path, backup)
        print(f"Settings backup: {backup}")
    if not args.uninstall:
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(Path(__file__).with_name("status.py"), destination)
    temporary = settings_path.with_name("settings.json.torch-tmp")
    with temporary.open("x") as handle:
        os.chmod(temporary, 0o600)
        json.dump(updated, handle, indent=2)
        handle.write("\n")
    temporary.replace(settings_path)
    print("Torch status hooks removed." if args.uninstall else
          "Torch status hooks installed. Restart Torch, then start a new Claude session.")


if __name__ == "__main__":
    main()
