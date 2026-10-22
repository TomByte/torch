set -g WARP_SESSION_ID @@WARP_SESSION_ID@@
set _user $USER; test -n "$_user"; or set _user (command whoami 2>/dev/null)
set -g WARP_IN_MSYS2 (test "$OS" = Windows_NT; and echo true; or echo false)
# When Torch launches the shell with the bootstrap script's path, the shell sources it right after this hook instead of waiting for Torch to write a `source` command to the PTY.
set _sources_bootstrap false; set -q WARP_BOOTSTRAP_FILE; and test -r "$WARP_BOOTSTRAP_FILE"; and set _sources_bootstrap true
if test "$WARP_IN_MSYS2" = true; set _msg "\e]9278;k;A;InitShell\a\e]9278;k;B;session_id;$WARP_SESSION_ID\a\e]9278;k;B;shell;fish\a\e]9278;k;B;user;$_user\a\e]9278;k;B;hostname;$hostname\a\e]9278;k;C\a"; else; set _msg (string escape --style=url -- "{\"hook\": \"InitShell\", \"value\": {\"session_id\": $WARP_SESSION_ID, \"shell\": \"fish\", \"user\": \"$_user\", \"hostname\": \"$hostname\", \"sources_bootstrap\": $_sources_bootstrap}}"); end
set WARP_USING_WINDOWS_CON_PTY @@USING_CON_PTY_BOOLEAN@@
# We send the InitShell hook via OSCs when on Windows and via DCSs otherwise.
if test "$WARP_USING_WINDOWS_CON_PTY" = true; if test "$WARP_IN_MSYS2" = true; printf "$_msg"; else; printf '\e]9278;j;%s\x07' "$_msg"; end; else; printf '\x1b\x50\x24\x6a%s\x1b\x5c' "$_msg"; end
set -e _user _msg
# fish runs its interactive setup, which prints the greeting, on the first prompt. Run it first so that, as when the bootstrap is written to the PTY after that prompt, its output stays out of the visible bootstrap block.
if test $_sources_bootstrap = true; functions -q __fish_on_interactive; and __fish_on_interactive; source $WARP_BOOTSTRAP_FILE; end
set -e _sources_bootstrap WARP_BOOTSTRAP_FILE
