---
name: Torch
description: A compact native terminal with quiet grouped sessions and an inset working pane.
colors:
  light-sidebar-top: "#E6ECF4"
  light-sidebar-bottom: "#EEE9F4"
  light-text: "#202124"
  light-secondary: "#616268"
  light-muted: "#85868B"
  light-selection: "#FFFFFFF5"
  light-hover: "#FFFFFF70"
  light-rule: "#D5D6DB"
  light-attention: "#98651F"
  dark-sidebar-top: "#2B2B2B"
  dark-sidebar-bottom: "#282828"
  dark-text: "#E6E6E6"
  dark-secondary: "#B5B5B5"
  dark-muted: "#A8A8A8"
  dark-selection: "#3B3B3B"
  dark-hover: "#FFFFFF12"
  dark-rule: "#424242"
  dark-attention: "#D9B96E"
  light-terminal-background: "#FCFCFD"
  light-terminal-foreground: "#303238"
  dark-terminal-background: "#232323"
  dark-terminal-foreground: "#E6E6E6"
  header-rule: "#88888818"
  light-terminal-accent: "#7798C2"
  dark-terminal-accent: "#8DAEDC"
  dark-header-rule: "#FFFFFF12"
  dark-ansi-normal-black: "#929292"
  dark-ansi-normal-red: "#E88484"
  dark-ansi-normal-green: "#88BE8C"
  dark-ansi-normal-yellow: "#D9B96E"
  dark-ansi-normal-blue: "#8DAEDC"
  dark-ansi-normal-magenta: "#BFA0D6"
  dark-ansi-normal-cyan: "#89C4C4"
  dark-ansi-normal-white: "#D8D8D8"
  dark-ansi-bright-black: "#A0A0A0"
  dark-ansi-bright-red: "#F0A0A0"
  dark-ansi-bright-green: "#A3D4A7"
  dark-ansi-bright-yellow: "#E8CE94"
  dark-ansi-bright-blue: "#ADC8EC"
  dark-ansi-bright-magenta: "#D3BCE4"
  dark-ansi-bright-cyan: "#A6D8D8"
  dark-ansi-bright-white: "#F0F0F0"
  kanagawa-terminal-background: "#1F1F28"
  kanagawa-terminal-foreground: "#DCD7BA"
  kanagawa-terminal-accent: "#7E9CD8"
  kanagawa-terminal-cursor: "#C8C093"
  kanagawa-ansi-normal-black: "#090618"
  kanagawa-ansi-normal-red: "#C34043"
  kanagawa-ansi-normal-green: "#76946A"
  kanagawa-ansi-normal-yellow: "#C0A36E"
  kanagawa-ansi-normal-blue: "#7E9CD8"
  kanagawa-ansi-normal-magenta: "#957FB8"
  kanagawa-ansi-normal-cyan: "#6A9589"
  kanagawa-ansi-normal-white: "#C8C093"
  kanagawa-ansi-bright-black: "#727169"
  kanagawa-ansi-bright-red: "#E82424"
  kanagawa-ansi-bright-green: "#98BB6C"
  kanagawa-ansi-bright-yellow: "#E6C384"
  kanagawa-ansi-bright-blue: "#7FB4CA"
  kanagawa-ansi-bright-magenta: "#938AA9"
  kanagawa-ansi-bright-cyan: "#7AA89F"
  kanagawa-ansi-bright-white: "#DCD7BA"
  ayu-terminal-background: "#242936"
  ayu-terminal-foreground: "#CCCAC2"
  ayu-terminal-accent: "#FFCC66"
  ayu-terminal-cursor: "#FFCC66"
  ayu-ansi-normal-black: "#0A0000"
  ayu-ansi-normal-red: "#F06B5C"
  ayu-ansi-normal-green: "#BFE76D"
  ayu-ansi-normal-yellow: "#E6B752"
  ayu-ansi-normal-blue: "#3BBBF4"
  ayu-ansi-normal-magenta: "#D09FFD"
  ayu-ansi-normal-cyan: "#84CEB5"
  ayu-ansi-normal-white: "#D2D6DC"
  ayu-ansi-bright-black: "#707A8C"
  ayu-ansi-bright-red: "#F39184"
  ayu-ansi-bright-green: "#D5FF80"
  ayu-ansi-bright-yellow: "#FFCD66"
  ayu-ansi-bright-blue: "#73D0FF"
  ayu-ansi-bright-magenta: "#DFBFFF"
  ayu-ansi-bright-cyan: "#95E6CB"
  ayu-ansi-bright-white: "#E3E6EA"
typography:
  terminal:
    fontFamily: "Hack"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.3
  body:
    fontFamily: "Roboto"
    fontSize: "14px"
    fontWeight: 400
  title:
    fontFamily: "Roboto"
    fontSize: "14px"
    fontWeight: 600
  label:
    fontFamily: "Roboto"
    fontSize: "12px"
    fontWeight: 400
  session:
    fontFamily: "Roboto"
    fontSize: "13px"
    fontWeight: 400
  group-label:
    fontFamily: "Roboto"
    fontSize: "12px"
    fontWeight: 600
  attention-label:
    fontFamily: "Roboto"
    fontSize: "12px"
    fontWeight: 500
rounded:
  control: "6px"
  command-palette: "8px"
  command-result: "4px"
  session: "8px"
  terminal: "12px"
spacing:
  row-vertical: "3px"
  session-gap: "2px"
  control-inset: "6px"
  group-heading-bottom: "7px"
  pane-inset: "8px"
  sidebar-list-inset: "9px"
  row-gap: "10px"
  row-horizontal: "12px"
  group-tail: "15px"
  header-right: "24px"
  header-left: "32px"
  window-controls-reserve: "80px"
components:
  command-palette:
    typography: "{typography.body}"
    rounded: "{rounded.command-palette}"
    width: "640px"
  command-result:
    typography: "{typography.body}"
    rounded: "{rounded.command-result}"
    padding: "6px 24px"
  sidebar:
    width: "272px"
  session:
    textColor: "{colors.light-text}"
    typography: "{typography.session}"
    rounded: "{rounded.session}"
    height: "28px"
    padding: "3px 12px"
  session-selected:
    backgroundColor: "{colors.light-selection}"
    textColor: "{colors.light-text}"
    typography: "{typography.session}"
    rounded: "{rounded.session}"
    height: "28px"
    padding: "3px 12px"
  session-hover:
    backgroundColor: "{colors.light-hover}"
  session-selected-dark:
    backgroundColor: "{colors.dark-selection}"
    textColor: "{colors.dark-text}"
    typography: "{typography.session}"
    rounded: "{rounded.session}"
    height: "28px"
    padding: "3px 12px"
  session-hover-dark:
    backgroundColor: "{colors.dark-hover}"
  icon-control:
    textColor: "{colors.light-secondary}"
    rounded: "{rounded.control}"
    padding: "{spacing.control-inset}"
    width: "28px"
    height: "28px"
  group-heading:
    textColor: "{colors.light-muted}"
    typography: "{typography.group-label}"
    padding: "12px 10px 7px 12px"
  terminal-header:
    typography: "{typography.title}"
    height: "40px"
    padding: "0px 24px 0px 32px"
  terminal-header-dark:
    backgroundColor: "{colors.dark-terminal-background}"
    textColor: "{colors.dark-text}"
    typography: "{typography.title}"
    height: "40px"
    padding: "0px 24px 0px 32px"
  terminal-pane:
    backgroundColor: "{colors.light-terminal-background}"
    textColor: "{colors.light-terminal-foreground}"
    rounded: "{rounded.terminal}"
  terminal-pane-dark:
    backgroundColor: "{colors.dark-terminal-background}"
    textColor: "{colors.dark-terminal-foreground}"
    rounded: "{rounded.terminal}"
  terminal-pane-kanagawa:
    backgroundColor: "{colors.kanagawa-terminal-background}"
    textColor: "{colors.kanagawa-terminal-foreground}"
    rounded: "{rounded.terminal}"
  terminal-pane-ayu:
    backgroundColor: "{colors.ayu-terminal-background}"
    textColor: "{colors.ayu-terminal-foreground}"
    rounded: "{rounded.terminal}"
---

# Design System: Torch

## Overview

**Creative North Star: "The Quiet Native Terminal"**

Torch pairs compact grouped sessions with a lightly raised terminal pane. Light mode uses a cool
blue-to-lilac sidebar and a soft off-white working surface; dark mode uses neutral charcoal layers and soft
gray text. The interface keeps session context legible while leaving the working area to shell output.

The binding layout direction is the user-supplied
[terminal reference](docs/design/terminal-reference.png), reinforced by the later light-sidebar
reference. The user's Codex dark UI reference guides the dark palette while preserving that layout.
Its personal sidebar history is not a design asset and is not copied into the repository. The
north-star phrase describes this direction; it is not a separate user-approved brand name.

This is a Rust/WarpUI desktop system. Frontmatter dimensions use portable `px` notation for WarpUI
logical pixels, not screenshot pixels. The tokens describe the current implementation in
`app/src/workspace/view/vertical_tabs/torch.rs`; font and terminal defaults come from
`app/src/settings/font.rs`, `app/src/settings/theme.rs`, and `app/src/terminal/settings.rs`.
Default terminal colors come from `app/src/themes/default_themes.rs`; derived command fills come
from `app/src/themes/mod.rs`. UI font loading comes from `app/src/appearance.rs`. Local palette
behavior comes from `app/src/search/command_palette/local_navigation.rs` and its workspace actions.
The four built-in choices are Light, Dark, Kanagawa, and Ayu. Existing custom theme references and
font preferences remain readable; retired built-in names follow the compatibility mapping described
below. The theme variants share the established layout and component geometry.

The original macOS layout captures in `.impeccable/review/desktop.png`, `desktop-narrow.png`, and
`desktop-long-title.png` establish the compact sidebar and long-title clipping. The initial dark-mode
review covers seven captures in the same directory: `dark-desktop.png` (2560 × 1604 screenshot
pixels), `dark-narrow.png` (1730 × 2046), `light-theme-regression.png` (2560 × 1602),
`dark-terminal-input.png`, `dark-context-menu.png`, `dark-theme-picker.png`, and
`dark-system-settings.png`. Independent finish review found no material defects in this scope,
confirmed dark palette coherence, and found no material light-mode regression. This supports close
visual fidelity, not pixel identity. Synthetic transcripts remain labelled test fixtures rather than
evidence of completed CLI tasks. Recording badges and cursor halos are capture artifacts.

Initial dark-mode live macOS checks passed choosing Dark in the native theme picker, syncing with the currently dark
OS appearance, returning to Light, retaining Dark after app restart, and completing `/usr/bi` to
`/usr/bin/` with Tab. The context menu and narrow layout were also verified. Six focused tests,
Clippy with existing baseline exemptions, and the native bundle passed. Cross-platform runner
`oz-dev` was unavailable; this evidence covers macOS only.

The softer palette and Cmd+P revision is captured in `soft-dark-terminal.png`,
`soft-dark-collapsed.png`, `soft-dark-palette.png`, `soft-light-terminal.png`,
`soft-light-palette.png`, `soft-light-narrow.png`, `soft-directory-query-final.png`, and
`soft-chooser.png` under `.impeccable/review/`. Files ending in `before` are comparison evidence,
not the current palette. Final corrections are captured in `soft-dark-palette-reviewed.png` and
`soft-light-palette-reviewed.png`. Four focused navigation tests, seven theme/group tests,
baseline-aware strict Clippy, and the native bundle passed. The independent finish review
returned ship with no remaining material findings. Verification remains limited to macOS.
Private reference images are not repository assets.

The final macOS live verification passed short reordering within a manual group, a long drag out
and back, Shift-selection across noncontiguous underlying tabs in displayed group order, and
reopening the sidebar after restoring its stored closed state. The final drop resolves the dragged
tab by stable identity before applying its position. These results supplement the source-derived
interaction rules below; screenshots alone do not verify interactions.

**Key Characteristics:**

- Compact session navigation with restrained activity markers.
- Tonal sidebar depth and one inset terminal surface.
- Native UI typography beside configurable monospace output.
- Session content and status derived from application state.

## Colors

The sidebar moves vertically from cool blue to pale lilac in light mode, and through closely spaced
neutral charcoal tones in dark mode. The darker terminal surface, lighter sidebar, and selected-row
fill create distinct layers without a purple cast. The frontmatter holds exact values and alpha.

### Primary

The terminal theme owns the application accent and ANSI output palette. The bundled local-only
Light theme uses `light-terminal-accent`, a muted blue; Dark uses `dark-terminal-accent`, a lighter
blue, with coordinated normal and bright ANSI colors. The `dark-ansi-normal-*` and
`dark-ansi-bright-*` tokens map directly to the eight standard
ANSI slots in each intensity. Programs retain control of which slots or explicit colors they use.
Sidebar sessions use a neutral selection fill and rule. Local command-palette results use a
restrained accent blend for selection.

Kanagawa uses the official Windows Terminal KanagawaWave palette: blue-violet background, warm
foreground, blue accent, and a separate warm cursor. Ayu uses the official npm `ayu@9.1.0` Mirage
palette: blue-gray background, soft foreground, and amber accent/cursor. Ayu's bright-black ANSI
slot deliberately uses Mirage's UI gray for legibility instead of its near-black terminal value.
The `kanagawa-*` and `ayu-*` tokens capture these two variants; the existing Light and Dark tokens
remain unchanged. [Palette sources and licences](docs/design/theme-sources.md) record upstream
provenance and the Ayu adjustment. Terminal text selection keeps Torch's theme-engine behavior.

### Secondary

The `light-attention` and `dark-attention` amber tokens identify blocked or error states. Group
attention text counts blocked sessions only. Amber must not become general decoration.

### Neutral

`sidebar-top` and `sidebar-bottom` define each mode's gradient. `text` carries names and active
status glyphs; `secondary` carries paths, ages, and controls; `muted` carries group labels and quiet
checks. `selection`, `hover`, and `rule` distinguish interaction states without bright fills.
The prefixed `terminal-background` and `terminal-foreground` tokens describe each named built-in
theme; existing custom terminal themes keep their own colors. The faint `header-rule` in Light and
`dark-header-rule` in Dark separate context from output.

**The State Evidence Rule.** Color and status symbols represent observed application state, never
invented activity, sample project progress, or presumed CLI success.

The sidebar first follows `theme.inferred_color_scheme()`: `DarkOnLight` selects the light palette.
For dark schemes named Kanagawa or Ayu, it derives surfaces from that theme's background and
foreground: foreground blends at 4% for sidebar top, 2% for bottom, 10% for selected rows, and 18%
for rules. Secondary text uses an 85% foreground blend; muted text uses 80%. Hover uses foreground
at 5% opacity, the header rule uses 7%, attention uses the theme's bright-yellow ANSI slot, and the
shadow retains the dark shadow token. Other dark schemes use the neutral Dark sidebar palette.
These theme-relative colors preserve the existing hierarchy without importing a new layout.

Measured dark token contrasts are body text on the terminal (12.59:1), muted text on selection
(4.71:1), muted text on the sidebar top endpoint (5.95:1), and normal ANSI black on the terminal
(5.05:1). Light terminal text is 12.50:1 against its background. These checks describe these specific
pairs, not every ANSI program or custom theme.

Local command surfaces blend the active theme background with its foreground at 2% opacity. The
focused terminal composer and command palette share this fill. Restored command history receives
a foreground overlay at 3% opacity. Local palette selection blends the theme background with its
accent at 16%; hover uses the theme's `surface_1()`. These are theme-relative formulas, not fixed
colors that override custom themes.

## Typography

The native UI uses bundled Roboto. Windows attempts Segoe UI before the Roboto fallback; that source
path has not been visually verified here. Terminal defaults use bundled Hack at the frontmatter
size and line-height ratio. Existing user preferences can override these defaults.

Session names use `body`; unread names use the semibold `title` weight. The active pane title also
uses `title`. Paths use body size with secondary color. Group names use `group-label`, counts and
ages use `label`, and attention counts use `attention-label`. There is no display or hero type scale.

**The Output Ownership Rule.** Terminal programs own their text, ANSI colors, wrapping, and content.
Do not reconstruct their output as product cards or embed a built-in assistant transcript.

## Layout

The native window is a horizontal sidebar and flexible terminal area. The sidebar's initial width
is the `sidebar` component token. It can resize from 220 logical pixels to the greater of 220 or 45%
of the available width. It can be hidden using the sidebar control; the same control then appears
in the terminal header. There is no web breakpoint or mobile layout.

The sidebar header is 56 logical pixels high and reserves space for native window controls. The
session list has the `sidebar-list-inset` on both sides, vertical scrolling, and an overlay scrollbar
3 logical pixels wide. Groups use a padded heading, compact rows separated by a 2-logical-pixel gap,
and `group-tail` spacing. Long session names ellipsize rather than increasing row height.

The terminal pane uses `pane-inset` on its top, right, and bottom edges, and on all four edges when
the sidebar is hidden. No left inset is added while the sidebar is open. Its header uses the
`terminal-header` tokens, a title capped at 280 logical pixels, and a flexible working-directory
label. Both clip with ellipsis. With the sidebar hidden, header left padding becomes
`window-controls-reserve`. Existing configured side panels remain part of the workspace layout.

Full-screen terminal programs default to `AltScreenPaddingMode::MatchBlocklist` in local-only
builds. Keep their padding aligned with the terminal block list rather than inventing a second
terminal inset. Narrow-window captures confirm the layout and long-title clipping; they do not
establish an untested minimum window size.

## Elevation & Depth

The sidebar is tonal, and session rows remain flat. The main pane uses a thin rule and one restrained
shadow. The exact native shadows are recorded in the sidecar as `terminal-pane` for light mode and
`terminal-pane-dark` for dark mode. Both use offset (0, 2), blur 12, and spread 0 in logical pixels;
the light shadow is faint blue-black and the dark shadow is translucent black. Hover changes fill.
New terminal rows are allocated to their launch folder before the first sidebar render. On macOS,
the row opens vertically in place over 180 ms with exponential ease-out; neighbouring rows move
only to make room within that group. Restored tabs and Reduce Motion use immediate placement.
Other platforms keep immediate placement until their system motion preference is available.
Running status glyphs remain static; the arrival transition does not imply a spinning indicator.

## Shapes

Controls, session rows, and the terminal pane use their distinct frontmatter radii. The active
session and main pane use a one-logical-pixel rule; multi-selected inactive rows receive selection
fill without the active border. The terminal surface clips its contents to its rounded boundary.
Icons stay small and optically quiet: controls and status glyphs use 16 logical pixels.

## Components

### Sidebar controls

Compact icon controls toggle the sidebar and add a default terminal tab. Their persistent mouse
state handles supply hover and click behavior. Preserve native command routing and window-drag
behavior; do not manufacture a web toolbar or account menu.

### Session rows

A session uses a 28-logical-pixel surface with a 13-logical-pixel title. The gap between rows stays
outside their hover and selection fills, keeping adjacent states distinct. The macOS captures
`compact-sidebar-light.png` and `compact-sidebar-dark.png` in `.impeccable/review/` verify this spacing;
keyboard switching, inline rename, and group collapse were also checked in the native app.

A row places the title first, then age, then status, with the `row-gap` between them. A normal click
activates; Shift-click selects a range in displayed grouped order; Command-click toggles membership
in the multi-selection. Double-click renames, middle-click closes, and right-click opens the tab or
multi-selection context menu. Inline rename uses the existing native editor. Vertical dragging
uses neighboring sessions from the same effective group; it must not silently move a tab into an
unrelated folder group.

Hovering a session swaps its timestamp for a small close button in a fixed 28-logical-pixel slot.
The status stays visible in its own 20-logical-pixel slot and the title does not move.
The button uses the shared icon-button
styling and “Close session” tooltip. Its click takes precedence over row selection and dispatches
the normal tab-close action, including existing running-process confirmation and undo behavior.

### Group headings

Explicit manual groups take precedence over project-root and working-directory groups. A local
session inside a configured project groups under that project's root, including subdirectories and
symlink aliases. If registered roots are nested, the deepest matching root wins. Canonical paths
keep aliases together while distinct roots with the same short name remain separate projects.

Sessions outside configured projects retain working-directory grouping: keys use the full directory
string with trailing separators removed, and labels use its last component. Empty and home-only
fallback paths use Other. Groups appear in first-occurrence order in the workspace tab sequence,
and members retain their order within that sequence. Status never sorts sessions.

All group headings are clickable disclosures with a 12-logical-pixel chevron: down when expanded,
right when collapsed. The existing heading padding belongs to the hit target. Long labels ellipsize
while the session count and amber “needs you” count remain visible; attention counts include blocked
sessions only, even while rows are hidden.

Automatic project, folder, and Other groups keep collapse state in the current window's runtime;
that state is not saved across restarts. Manual groups retain their existing saved collapse state,
double-click rename, and context menu. Collapsing a group hides its rows without closing or stopping
its terminals. Explicitly activating a tab reveals its group. Rows in collapsed automatic groups
are excluded from Shift-selection ranges.

Configured project headings show a small + button on hover, with a fixed slot to prevent text
movement. It creates a fresh terminal at the configured project root even when existing sessions
are in subdirectories, and reveals the new session if the project was collapsed. The button click
does not trigger the heading's collapse action. Missing directories use the same error handling
as Cmd+P. Manual groups and unconfigured folder groups have no project-root creation control.

### Session identity and activity

Title precedence is a user-defined tab title, a locally tracked CLI session title, then the pane's
display title; empty or directory-only fallbacks become Terminal. Age comes from terminal session
activity: now, minutes, hours, or days. It is not a claim about task duration.

Status first comes from the terminal's conversation summary. A running shell command without that
summary supplies InProgress, except for detected CLI agents without structured status: an open
agent is not evidence of active work. Running uses a broken ring, blocked uses an amber ring and
dot, unread completion uses a filled check, and read completion uses a muted check. Errors use an
amber exclamation ring. Idle, cancelled, waiting, and unknown agent states use a small muted dot.
Status assets share a 24-unit drawing grid rendered at 16 logical pixels. When both status and
activity age are absent and no agent is detected, the slot is empty. Unread activity makes the
title bold. Blocked sessions contribute to the project heading's “needs you” count even while
collapsed.

The local Claude integration in `integrations/claude` forwards lifecycle events through terminal
notifications. It neither reads transcripts nor sends prompt/response content. Hooks are gated by
Torch's `TORCH_TERMINAL` environment marker; the installer preserves unrelated Claude settings.
Idle reminder notifications and subagent events cannot override the parent's state.

### Projects settings

Settings → Projects starts with no configured project or parent folders. Do not seed locations such
as `~/GIT` or `~/Sessions`. Add project registers one folder; Add parent folder discovers only its
immediate child directories, without recursive scanning. The catalog refreshes when Cmd+P or the
Projects page opens and when configuration changes. Settings are persisted locally without cloud
sync; unavailable configured folders are labelled in the page.

Use the existing native Settings controls and theme styling. Removing an entry removes that
configured source, without deleting files or closing terminals. A project still supplied by another
configured source remains available. Project configuration does not replace explicit manual groups.

### Command palette

Cmd+P opens local project, session, directory, and action navigation with results immediately
visible. Configured projects lead the empty query, followed by session switching and creation
options. Resume project NAME switches to an existing session whose working directory belongs to
that project; Open project NAME starts a terminal at its root if none exists. Matching includes
subdirectories, canonical symlink aliases, and sessions in other windows. New session in NAME
explicitly creates an additional terminal at the project root. Manage projects opens Settings.

Session switching puts other sessions ahead of the current one. New session starts in the current
working folder, falling back to home if needed; New session in directory… opens the native folder
chooser. Known valid directories from open sessions remain available even without configured
projects. Project entries retain full paths as secondary text to distinguish duplicate short names.

Search matches titles and details case-insensitively and supports fuzzy session names. Exact project
names and local action titles rank above fuzzy session matches. A valid typed absolute path, `~`, or
`~/…` ranks highest and explicitly opens a new terminal in that directory. Relative and nonexistent
paths do not produce that directory action. Paths are passed as filesystem paths, never interpolated
into shell commands. Existing manual groups and shell Tab completion keep their own behavior.

The placeholder reads “Search projects, sessions, and actions”. It starts from the active theme
foreground at 65% opacity and is contrast-adjusted to at least 4.5:1 against the actual command
surface. Its colors update with appearance changes. Local result icons likewise enforce non-text
contrast against the actual selected, hovered, or default fill. Result titles use body typography;
details use label typography, foreground at 75% opacity, and 3 logical pixels of top separation.
The palette keeps its native editor, result list, keyboard handling, and theme outline; its body has
a maximum height of 464 logical pixels. Preview browser focus outlines are illustrative only.

### Theme controls

Use the existing native Settings → Appearance controls. Current theme opens the native picker
with exactly four entries, ordered Light, Dark, Kanagawa, and Ayu. Kanagawa is the Wave variant;
Ayu is Mirage. Light remains the local-only default. Sync with OS uses the configured light and
dark appearance choices, with the same four-theme catalog.

Retired built-in preferences resolve to Light or Dark through the compatibility mapping, including
preferences stored for OS appearance modes. Existing custom theme files and references are retained,
but custom entries and creation controls are hidden from the four-choice picker and Settings.
The previously verified Dark selection persists across restart; final validation for newly added
themes is recorded separately. Theme switching preserves the sidebar layout and terminal behavior.

### Terminal pane

The header shows the active title and current working folder; the body remains the real terminal
and its configured side panels. Separately installed CLI tools can render their own interfaces in
that terminal. Their names, content, and colors are not built-in Torch assistant features.

The sidecar's HTML/CSS snippets are isolated documentation previews of native components. They are
not implementation assets, a replacement renderer, or evidence of native keyboard accessibility.
Preview focus outlines are browser affordances and must not be copied into WarpUI as verified tokens.
Sidecar tonal ramps are synthesized OKLCH preview strips from opaque RGB; the frontmatter colors
remain normative. No additional product palette or motion behavior is implied by the preview.

## Do's and Don'ts

### Do:

- Do preserve the binding terminal reference and the compact native hierarchy.
- Do keep user titles, working directories, activity, and unread state tied to real application data.
- Do preserve explicit groups and use the displayed group order for range selection.
- Do keep terminal themes, font preferences, shell completion, and full-screen programs functional.
- Do preserve the shared sidebar layout and native theme controls in both light and dark modes.
- Do distinguish source-derived styling, visual fixtures, and live behavior verification.

### Don't:

- Don't add built-in AI, account, billing, telemetry, or cloud controls to this local terminal.
- Don't fabricate session activity or treat a quiet check as guaranteed command success.
- Don't reproduce the operating system's recording badge or capture cursor halo as product UI.
- Don't claim pixel identity or verification beyond the recorded macOS captures and live checks.
- Don't replace terminal output with static product-owned conversation content.
