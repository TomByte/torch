# Torch

<!-- impeccable:product-schema 1 -->

## Platform

Native desktop (Rust and WarpUI), currently verified on macOS.

## Product Purpose

A local terminal with ordinary shell completion and history, without Warp accounts or built-in AI prediction. Separately installed Claude and Codex CLIs run as normal terminal programs.

## Operating Context

The user works across multiple terminal and CLI-agent sessions. Projects start empty: the user adds individual project folders or parent folders in Settings → Projects. Parent folders expose only their immediate child directories. Configuration stays local.

Cmd+P prioritizes configured projects, resumes an existing terminal in a project when available, and starts one otherwise. Sessions group by their matching project root, falling back to their working folder; explicit manual groups take precedence.

## Brand Commitments

Torch. The user-provided October 2026 reference screenshot is the visual authority for this redesign: refined native sidebar, compact grouped sessions, restrained activity indicators, and a white terminal pane. Preserve real terminal output and shell behavior.

## Constraints

Do not restore account, billing, cloud, telemetry, or built-in AI input features. Session names, activity, and transcripts must come from real application state. Original licences remain intact.
