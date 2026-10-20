# Claude session status in Torch

Torch displays a broken ring while Claude works, an amber ring and dot when it
needs permission or an answer, a filled check for an unread completed turn,
and a quiet check once you have read it. Errors have an amber exclamation mark.
An idle or uninstrumented session has a muted dot; an open CLI alone is not
evidence that an agent is working. Project headers count sessions needing you,
including those in collapsed groups.

## Install

Requires Python 3 and a current Claude Code version supporting the
[`terminalSequence` hook output](https://code.claude.com/docs/en/hooks#emit-terminal-notifications).
The development host uses Claude Code 2.1.304. Hook translation and native status
rendering were verified with local event fixtures on macOS; Linux and Windows
have not been run through this check. From this checkout:

```sh
python3 integrations/claude/install.py
```

Restart Torch and launch a new Claude session. Existing sessions may retain their
old hook configuration and environment. The installer copies the hook to your
Claude configuration directory, merges its entries into `settings.json`, and
prints the location of a dated backup. Existing hooks, plugins, and permissions
are preserved. Re-running the installer updates Torch's entries without duplicates.
`CLAUDE_CONFIG_DIR` and `--config-dir PATH` are supported.

The hook runs only when `TORCH_TERMINAL=1`, which Torch supplies to new local
shells. It emits status through Claude's terminal output; there are no network
requests, transcript reads, prompt/response logging, or permission decisions.
It deliberately ignores idle reminder notifications and subagent completions.
The `warp://cli-agent` wire identifier is retained for protocol compatibility.

This reports the parent turn's lifecycle, not a percentage complete. When Claude
reports pending background tasks or scheduled wakeups, Torch retains the working
state rather than marking the session complete. Remote SSH shells do not
automatically inherit this local integration.

## Remove

```sh
python3 integrations/claude/install.py --uninstall
```

Only Torch's hook entries are removed. The copied script and backups are retained.
Restart Claude to stop using the hooks. To restore the whole previous settings
file instead, use the printed backup (this also reverts later settings edits).

## Other CLI agents

The sidebar also consumes existing structured CLI status events from Codex and
other supported agents. This installer configures **Claude only**. Uninstrumented
agents remain a neutral dot rather than showing guessed working/permission states.

## Verify

```sh
python3 -m unittest discover -s integrations/claude -p 'test_*.py'
```

In Torch, submit a Claude prompt, observe the working ring, then observe a real
permission/question prompt and its amber marker. Switch away before the turn
finishes to see the unread check, then reopen it to see the quiet check. Hovering
a row keeps its status visible and shows the close button in the timestamp slot.

The implementation passed 13 sidebar tests, the local unread-notification regression
test, and seven Python hook/installer tests, plus targeted Clippy and a macOS build.
Native event fixtures confirmed attention counts, working/error indicators, hover
behavior, and unread-to-read transitions. The broader CLI suite passed 178 tests;
four inherited plugin-manager checks still fail because they expect Warp branding
or feature flags disabled by Torch. Cross-platform runners were unavailable
(`oz-dev` is not installed), so Windows/WSL and Linux remain unverified.
