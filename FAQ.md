# Torch FAQ

**Do I need an account?** No. Torch opens directly into your shell.

**Does Tab still complete commands?** Yes. Ordinary completions and history suggestions remain. Built-in AI predictions and natural-language suggestions are disabled.

**Can I run other tools in the terminal?** Yes, including your own CLI agents.

**Why are there still a few Warp names in the source?** Torch keeps Warp's licence and copyright notices and its attribution. Dependencies fetched from Warp's repositories keep their names, as do internal type and module names and the identifiers agent plugins rely on (the `warp://cli-agent` status protocol and `TERM_PROGRAM=WarpTerminal`). Everything you see in the app and its build output is Torch.

**Where are builds?** macOS builds are attached to each [GitHub release](https://github.com/TomByte/torch/releases), and Torch updates itself from there. You can also build from source using the README. Torch doesn't use Warp's update servers or release pipeline.
