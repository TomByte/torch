# Claude session status in Torch

Torch shows a working ring, an amber marker when Claude needs you, a filled check
for an unread completed turn, and a quiet check once you read it. Errors have an
amber exclamation mark. An uninstrumented session has a neutral dot. Collapsed
project headers still count sessions needing attention.

## Set up from the shipped app

1. Install Torch normally. No source checkout, Python, Node, or separate helper is required.
2. Open **Settings → Scripting → Claude session status**.
3. Check the Claude configuration folder. The default is `CLAUDE_CONFIG_DIR` when
   present in Torch's environment, otherwise `~/.claude`. Use **Choose folder…**
   for another profile. A GUI app may not inherit variables from your shell startup file.
4. Click **Install / repair**. Setup merges Torch's entries into `settings.json`,
   preserves other hooks, plugins, and permissions, and displays a private backup
   path if it changed an existing file. Invalid JSON or unexpected hook structure
   produces an error instead of replacing the file.
5. Open a new Torch terminal and start a new Claude session. Existing shells and
   Claude processes may still hold their old environment or hooks.

Requires a current Claude Code version supporting
[`terminalSequence` hook output](https://code.claude.com/docs/en/hooks#emit-terminal-notifications).
This output works in interactive Claude sessions, not `claude -p` or the Agent SDK.
Setup does not install Claude or enable hooks disabled by your Claude settings or
organization policy.

**Remove** deletes only entries labelled `Torch session status`; restart Claude
for removal to take effect. Backups are retained. Install / repair also migrates
entries created by Torch's early Python installer. Its old `torch/status.py` file
is no longer used and can be deleted after restarting Claude.

## How this ships and updates

The translator is compiled into the Torch executable (`app/src/local_hooks.rs`).
Its `--torch-claude-status-hook` entry point exits before GUI, logging, or network
initialization. All normal Torch bundles contain it automatically; there are no
extra runtime assets to copy or interpreters to discover.

The installed command is identical on every supported machine. Each new local
terminal supplies `TORCH_TERMINAL=1` and `TORCH_HOOK_EXECUTABLE`, pointing to the
executable that launched it. The command quotes that path and resolves it at run
time, so spaces, Unicode, different usernames, app locations, and normal in-place
updates require no settings rewrite. After moving or updating Torch, reopen it
and start new terminals. A missing executable or a non-Torch environment is a
silent no-op. Re-running setup is safe and does not duplicate entries.

Only lifecycle metadata (event, session ID, working directory, and counts of
pending background tasks) is forwarded through Claude's terminal. Prompts,
responses, tool inputs, and transcripts are neither read nor forwarded. No network,
permission decisions, or account service is involved. Child-agent completions
and idle reminders cannot complete the parent turn. Hook errors exit successfully
and must not block Claude. The internal `warp://cli-agent` identifier is retained
for compatibility with the terminal parser.

The setup control currently supports **macOS and Linux local shells**. macOS is
verified; Linux is implemented but still requires a packaged smoke test before a
Linux release. Windows/WSL setup is not exposed yet: it needs a native invocation
and packaging check. SSH and containers need an executable installed in that
environment; this local integration does not forward a host executable into them.
The Claude installer does not configure Codex or other agents.

## Release checks

- Run `cargo test -p warp --lib --features gui,local_only local_hooks::tests` and
  `cargo test -p warp --lib --features gui,local_only settings_view::scripting_page::tests`.
- Build the normal app bundle. On a clean account with no checkout or Python,
  install through Settings into a temporary Claude configuration folder; verify
  unrelated settings survive install, repair, and removal.
- Move the app to a path containing spaces, reopen it, and start a new terminal.
  Check that the hook invokes that copy and returns status JSON without opening
  another window. Check a non-Torch shell remains silent.
- With current Claude Code, submit a prompt, exercise an actual permission or
  question request, then switch away before completion. Verify the attention count,
  unread check, and read transition. Event fixtures alone do not validate Claude's
  end-to-end event delivery.
- Repeat the packaged test on each OS/architecture being released. Do not infer
  Windows, WSL, or Linux success from a macOS build.

## Verification recorded for this change

On macOS, 10 native hook/installer tests and two settings tests passed, followed
by targeted Clippy and a normal packaged build. The relocated app was opened
with a separate data profile and temporary Claude settings. UI install, repeated
repair, removal, and search passed; removal restored the original configuration.
The installed command invoked the relocated executable, and a new native terminal
received that executable's actual path. Invalid input and non-Torch invocations
exited silently. No paid Claude request was made during this packaging check.
Linux/Windows cloud runners were unavailable (`oz-dev` is not installed).
