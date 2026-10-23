# zsh startup for Torch demo sessions only (script/demo/run sets ZDOTDIR here): the scripted
# agents come first on PATH, and none of your own shell configuration is loaded.
export PATH="${ZDOTDIR:h}/bin:$PATH"
