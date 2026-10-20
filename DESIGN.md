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
  dark-sidebar-top: "#242424"
  dark-sidebar-bottom: "#212121"
  dark-text: "#E6E6E6"
  dark-secondary: "#B5B5B5"
  dark-muted: "#A0A0A0"
  dark-selection: "#373737"
  dark-hover: "#FFFFFF12"
  dark-rule: "#3C3C3C"
  dark-attention: "#D9B96E"
  light-terminal-background: "#FFFFFF"
  light-terminal-foreground: "#111111"
  dark-terminal-background: "#181818"
  dark-terminal-foreground: "#E6E6E6"
  header-rule: "#88888818"
  dark-terminal-accent: "#8DAEDC"
  dark-header-rule: "#FFFFFF12"
  dark-ansi-normal-black: "#858585"
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
  session: "9px"
  terminal: "12px"
spacing:
  row-vertical: "4px"
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
  sidebar:
    width: "272px"
  session:
    textColor: "{colors.light-text}"
    typography: "{typography.body}"
    rounded: "{rounded.session}"
    height: "31px"
    padding: "4px 12px"
  session-selected:
    backgroundColor: "{colors.light-selection}"
    textColor: "{colors.light-text}"
    typography: "{typography.body}"
    rounded: "{rounded.session}"
    height: "31px"
    padding: "4px 12px"
  session-hover:
    backgroundColor: "{colors.light-hover}"
  session-selected-dark:
    backgroundColor: "{colors.dark-selection}"
    textColor: "{colors.dark-text}"
    typography: "{typography.body}"
    rounded: "{rounded.session}"
    height: "31px"
    padding: "4px 12px"
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
---

# Design System: Torch

## Overview

**Creative North Star: "The Quiet Native Terminal"**

Torch pairs compact grouped sessions with a lightly raised terminal pane. Light mode uses a cool
blue-to-lilac sidebar and a white working surface; dark mode uses neutral charcoal layers and soft
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
Default terminal colors come from `app/src/themes/default_themes.rs`, and UI font loading comes from
`app/src/appearance.rs`. User-selected terminal themes and fonts remain authoritative at runtime.

The original macOS layout captures in `.impeccable/review/desktop.png`, `desktop-narrow.png`, and
`desktop-long-title.png` establish the compact sidebar and long-title clipping. The final dark-mode
review covers seven captures in the same directory: `dark-desktop.png` (2560 × 1604 screenshot
pixels), `dark-narrow.png` (1730 × 2046), `light-theme-regression.png` (2560 × 1602),
`dark-terminal-input.png`, `dark-context-menu.png`, `dark-theme-picker.png`, and
`dark-system-settings.png`. Independent finish review found no material defects in this scope,
confirmed dark palette coherence, and found no material light-mode regression. This supports close
visual fidelity, not pixel identity. Synthetic transcripts remain labelled test fixtures rather than
evidence of completed CLI tasks. Recording badges and cursor halos are capture artifacts.

Live macOS checks passed choosing Dark in the native theme picker, syncing with the currently dark
OS appearance, returning to Light, retaining Dark after app restart, and completing `/usr/bi` to
`/usr/bin/` with Tab. The context menu and narrow layout were also verified. Six focused tests,
Clippy with existing baseline exemptions, and the native bundle passed. Cross-platform runner
`oz-dev` was unavailable; this evidence covers macOS only.

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
Dark theme uses `dark-terminal-accent`, a restrained blue, with coordinated normal and bright ANSI
colors. The `dark-ansi-normal-*` and `dark-ansi-bright-*` tokens map directly to the eight standard
ANSI slots in each intensity. Programs retain control of which slots or explicit colors they use.
Active sessions use a neutral selection fill and rule rather than the theme accent.

### Secondary

The `light-attention` and `dark-attention` amber tokens identify blocked or error states. Group
attention text counts blocked sessions only. Amber must not become general decoration.

### Neutral

`sidebar-top` and `sidebar-bottom` define each mode's gradient. `text` carries names and active
status glyphs; `secondary` carries paths, ages, and controls; `muted` carries group labels and quiet
checks. `selection`, `hover`, and `rule` distinguish interaction states without bright fills.
`terminal-background` and `terminal-foreground` describe the bundled Light and Dark themes only;
custom terminal themes keep their own colors. The faint `header-rule` in light mode and
`dark-header-rule` in dark mode separate context from output.

**The State Evidence Rule.** Color and status symbols represent observed application state, never
invented activity, sample project progress, or presumed CLI success.

The sidebar follows `theme.inferred_color_scheme()`: `DarkOnLight` selects the light palette;
otherwise it selects the dark palette. This uses the theme's color-scheme inference rather than a
single color-channel threshold. Explicit custom terminal themes retain their own output colors.

Measured dark token contrasts are body text on the terminal (14.23:1), muted text on selection
(4.55:1), muted text on the sidebar top endpoint (5.94:1), and normal ANSI black on the
terminal (4.81:1). These checks describe these specific pairs, not every ANSI program or custom theme.

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
3 logical pixels wide. Groups use a padded heading, contiguous compact rows, and `group-tail`
spacing. Long session names ellipsize rather than increasing row height.

The terminal pane uses `pane-inset` on its top, right, and bottom edges. Its header uses the
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
the light shadow is faint blue-black and the dark shadow is translucent black. Hover changes fill;
no animation timing or easing is defined in this workspace renderer. Do not infer a pulsing or
spinning animation from a static running glyph.

## Shapes

Controls, session rows, and the terminal pane use their distinct frontmatter radii. The active
session and main pane use a one-logical-pixel rule; multi-selected inactive rows receive selection
fill without the active border. The terminal surface clips its contents to its rounded boundary.
Icons stay small and optically quiet: controls use 16-logical-pixel glyphs and status glyphs use 14.

## Components

### Sidebar controls

Compact icon controls toggle the sidebar and add a default terminal tab. Their persistent mouse
state handles supply hover and click behavior. Preserve native command routing and window-drag
behavior; do not manufacture a web toolbar or account menu.

### Session rows

A row places the title first, then age, then status, with the `row-gap` between them. A normal click
activates; Shift-click selects a range in displayed grouped order; Command-click toggles membership
in the multi-selection. Double-click renames, middle-click closes, and right-click opens the tab or
multi-selection context menu. Inline rename uses the existing native editor. Vertical dragging
uses neighboring sessions from the same effective group; it must not silently move a tab into an
unrelated folder group.

### Group headings

Explicit manual groups take precedence over automatic working-directory groups. Automatic group
keys use the full working-directory string with trailing separators removed; labels use the last
path component. Distinct paths may therefore share a short label without being merged. Empty and
home-only paths use Other. Groups appear in first-occurrence order in the workspace tab sequence,
and members retain their order within that sequence. Status never sorts sessions.

Manual headings support collapse, rename, and a group context menu. Automatically derived headings
are labels, not invented collapsible controls. Counts include the group's sessions, and the amber
“needs you” count includes blocked sessions only.

### Session identity and activity

Title precedence is a user-defined tab title, a locally tracked CLI session title, then the pane's
display title; empty or directory-only fallbacks become Terminal. Age comes from terminal session
activity: now, minutes, hours, or days. It is not a claim about task duration.

Status first comes from the terminal's conversation summary. A running shell command without that
summary supplies InProgress. Running uses a broken ring, blocked uses an amber ring and dot, success
uses a filled check, and errors use an amber alert. Cancelled, waiting, or absent status can use a
muted check; this quiet check must not be interpreted as proof of successful execution. When both
status and activity age are absent, the status slot is empty. Unread activity makes the title bold.

### Theme controls

Use the existing native Settings → Appearance controls: Current theme opens the theme chooser,
and Sync with OS selects the configured theme for the current system appearance. The chooser
renders as a native workspace panel. Dark selection persists across restart; enabling OS sync
follows the configured system themes. Light remains the default for local-only builds, and explicit
custom themes remain available. Theme switching preserves the sidebar layout and terminal behavior.

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
