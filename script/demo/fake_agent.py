#!/usr/bin/env python3
"""Scripted stand-in for a coding-agent CLI, used to stage Torch screenshots.

It prints a short agent-style transcript and reports its state to Torch with the same OSC 777
`warp://cli-agent` notifications that real agent plugins send, so the sidebar and Show Agents
render genuine states without running a real agent. The agent name comes from the command it was
invoked as (claude, gemini, opencode, ...), the task from the first argument, and the final state
from the task (see TASK_STATES) or TORCH_DEMO_STATE: working, needs-you, finished, failed or idle.
"""

import json
import os
import sys
import time
import uuid

AGENT = os.path.basename(sys.argv[0])
TASK = " ".join(sys.argv[1:]).strip() or "Tidy up the README"
# The state each example task ends in, unless TORCH_DEMO_STATE overrides it.
TASK_STATES = {"flaky": "needs-you", "settings": "finished", "sdk": "failed"}
STATE = os.environ.get("TORCH_DEMO_STATE") or next(
    (state for key, state in TASK_STATES.items() if key in TASK.lower()), "working"
)
# With TORCH_DEMO_STAGGER set, agents reach their final state this many seconds after starting,
# so a screenshot taken a few minutes in shows varied ages instead of "now" everywhere.
TASK_DELAYS = {"flaky": 300, "sdk": 120}
DELAY = os.environ.get("TORCH_DEMO_STAGGER") and next(
    (delay for key, delay in TASK_DELAYS.items() if key in TASK.lower()), 0
)
SESSION = str(uuid.uuid4())

DIM, BOLD, RESET = "\033[2m", "\033[1m", "\033[0m"
ACCENT = {"claude": "\033[38;5;173m", "gemini": "\033[38;5;111m", "opencode": "\033[38;5;150m"}.get(
    AGENT, "\033[38;5;180m"
)

# Steps shown before the final state, per task keyword. Each is (tool, detail, result).
STEPS = {
    "rate limiting": [
        ("Read", "src/middleware/mod.rs", "Read 142 lines"),
        ("Grep", "\"RateLimit\" in src/", "Found 6 matches in 3 files"),
        ("Edit", "src/middleware/rate_limit.rs", "Added token-bucket limiter keyed by API key"),
        ("Bash", "cargo test -p orbit-api rate_limit", "12 passed"),
    ],
    "flaky": [
        ("Read", "e2e/checkout.spec.ts", "Read 218 lines"),
        ("Bash", "npm run test:e2e -- checkout --repeat-each=20", "3 of 20 runs failed: timeout waiting for #pay"),
        ("Grep", "\"waitForTimeout\" in e2e/", "Found 4 matches in 2 files"),
        ("Read", "src/checkout/PaymentFrame.tsx", "Read 96 lines"),
        ("Read", "playwright.config.ts", "Read 41 lines"),
        ("Edit", "e2e/checkout.spec.ts", "Wait for the payment frame instead of a fixed delay"),
        ("Edit", "e2e/helpers/payment.ts", "Added waitForPaymentFrame() with a 10s budget"),
        ("Bash", "npm run test:e2e -- checkout --repeat-each=20", "20 passed (1.4m)"),
    ],
    "settings": [
        ("Read", "src/pages/settings/Profile.tsx", "Read 301 lines"),
        ("Edit", "src/pages/settings/Profile.tsx", "Replaced 9 legacy inputs with <Field>"),
        ("Edit", "src/pages/settings/Notifications.tsx", "Replaced 5 legacy inputs with <Field>"),
        ("Bash", "npm run typecheck && npm test -- settings", "All checks passed"),
    ],
    "sdk": [
        ("Read", "package.json", "payments-sdk 4.2.0 -> 5.0.0"),
        ("Bash", "npm install payments-sdk@5", "added 3 packages, changed 1"),
        ("Grep", "\"createCharge\" in src/", "Found 11 call sites"),
    ],
    "drift": [
        ("Read", "modules/preview-env/main.tf", "Read 88 lines"),
        ("Bash", "terraform plan -detailed-exitcode", "Plan: 0 to add, 1 to change, 0 to destroy"),
        ("Read", "envs/preview/terraform.tfstate", "ttl_hours differs: 24 in state, 48 in code"),
    ],
    "terraform": [
        ("Read", "modules/web/main.tf", "Read 96 lines"),
        ("Write", "modules/preview-env/main.tf", "Created module with variables for branch and TTL"),
        ("Bash", "terraform validate", "Success! The configuration is valid."),
    ],
}
FINAL = {
    "needs-you": ("Bash", "npm run test:e2e -- checkout --repeat-each=50"),
    "finished": "Migrated 14 inputs across 2 pages to <Field>. Typecheck and settings tests pass.",
    "failed": "rate_limit",
}


def notify(event, **fields):
    body = {"v": 1, "agent": AGENT, "event": event, "session_id": SESSION, "cwd": os.getcwd(),
            "project": os.path.basename(os.getcwd()), **fields}
    sys.stdout.write(f"\033]777;notify;warp://cli-agent;{json.dumps(body)}\007")
    sys.stdout.flush()


def say(line="", pause=0.0):
    print(line, flush=True)
    time.sleep(pause)


def spin(until):
    """Shows a working spinner until the given time."""
    spinner = "✻✳✶✢·✢✶✳"
    tick = 0
    while time.time() < until:
        sys.stdout.write(f"\r{ACCENT}{spinner[tick % len(spinner)]}{RESET} Working… {DIM}(esc to interrupt){RESET}")
        sys.stdout.flush()
        tick += 1
        time.sleep(0.15)


def steps_for(task):
    lowered = task.lower()
    for key, steps in STEPS.items():
        if key in lowered:
            return steps
    return STEPS["rate limiting"]


def main():
    sys.stdout.write(f"\033]0;{TASK}\007")
    notify("session_start", plugin_version="demo")
    say(f"{ACCENT}{BOLD}{AGENT}{RESET} {DIM}· {os.path.basename(os.getcwd())}{RESET}")
    say()
    say(f"{BOLD}>{RESET} {TASK}", 0.3)
    notify("prompt_submit", query=TASK)
    say()
    for tool, detail, result in steps_for(TASK):
        say(f"{ACCENT}●{RESET} {BOLD}{tool}{RESET}({detail})", 0.25)
        say(f"  {DIM}⎿  {result}{RESET}", 0.25)
        notify("tool_complete", tool_name=tool)
    say()
    if DELAY:
        spin(time.time() + DELAY)
        sys.stdout.write("\r\033[K")

    if STATE == "needs-you":
        tool, command = FINAL["needs-you"]
        say(f"{ACCENT}●{RESET} {BOLD}{tool}{RESET}({command})")
        say(f"  {DIM}Run the checkout suite 50 times to confirm the fix?{RESET}")
        say(f"  {BOLD}❯ 1. Yes{RESET}   2. Yes, and don't ask again   3. No")
        notify("permission_request", tool_name=tool, tool_input={"command": command})
    elif STATE == "finished":
        say(f"{ACCENT}●{RESET} {FINAL['finished']}")
        notify("stop", summary=FINAL["finished"])
        say()
        say(f"{BOLD}>{RESET} ", 0)
    elif STATE == "failed":
        say(f"{ACCENT}●{RESET} \033[31mAPI rate limit reached. Try again in a few minutes.{RESET}")
        notify("stop_failure", error_type=FINAL["failed"])
    elif STATE == "idle":
        notify("stop")
        say(f"{BOLD}>{RESET} ", 0)
    else:
        spin(float("inf"))

    # Stay open like a real agent waiting for its next prompt.
    try:
        sys.stdin.readline()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print()
