# Torch-specific guidance

This is the independent Torch fork. The guidance below documents inherited architecture. Torch builds use `cargo check -p torch --bin torch --features gui` and `./script/run`; the Cargo package and its binary are both `torch`. Keep `local_only` enabled. Do not add Warp login, AI prediction, cloud integration, telemetry, or upstream release/sync automation. Preserve ordinary completions and shell behavior, plus original licence/copyright notices. The only Git remote is `TomByte/torch`.

Treat Torch as a distributable app: local integrations must ship with the normal app bundle, resolve user/install paths at runtime, and provide setup/removal without this checkout or developer-machine dependencies. Preserve unrelated user configuration, test clean/custom installation paths, and state which platforms were actually verified.

# AGENTS.md

This file provides guidance when working with code in this repository.

## Development Commands

### Build and Run
- `cargo run` / `./script/run` - Build and run the GUI desktop app locally
- `cargo bundle --bin torch` - Bundle the main (GUI) app
- `./script/release` - Build a signed, notarized, universal Torch release into `dist/` as a DMG and an optional zip; `--help` lists its options

### Testing
- `cargo nextest run --no-fail-fast --workspace` - Run tests with nextest
- `cargo test --doc` - Run doc tests
- `cargo nextest run -p <package>` - Run tests for an individual package

### Linting and Formatting
- `./script/presubmit` - Run the full local presubmit only when explicitly requested
- `./script/format` - Format code
- `cargo clippy -p <package> --all-targets --tests -- -D warnings` - Run targeted Clippy
- `./script/run-clang-format.py -r --extensions 'c,h,cpp,m' ./crates/torchui/src/ ./app/src/` - Check C/C++/Obj-C formatting
- `./script/run-clang-format.py -i -r --extensions 'c,h,cpp,m' ./crates/torchui/src/ ./app/src/` - Format C/C++/Obj-C code in place
- `find . -name "*.wgsl" -exec wgslfmt --check {} +` - Check WGSL shader formatting
- `find . -name "*.wgsl" -exec wgslfmt {} +` - Format WGSL shaders in place

### Implementation Validation Order
Optimize for fast delivery and let CI catch uncommon failures outside targeted local coverage.

1. While editing, run only the smallest targeted `cargo check` or `cargo nextest` command that gives useful feedback; defer checks and tests until after implementation where possible.
2. Once the code and self-review are complete, run the relevant tests and fix the code until they pass.
3. Run the relevant Clippy and other lint, typecheck, or build checks and fix their findings. Return to affected tests only when a fix materially changes behavior.
4. Run every applicable mutating formatter once, after all other code changes are complete.
5. Open or update the PR without rerunning tests or lint after formatting and without adding a full `./script/presubmit` run.

Run the full presubmit only when the user, task, or approved spec explicitly requires it. For agent-driven implementation, this section replaces the broader pre-push presubmit guidance in `CONTRIBUTING.md`; that document still describes the human contributor workflow. A later source, test, manifest, generated-code, or configuration change creates a new candidate: rerun the affected portion of the sequence and finish with the applicable formatter. Local commits are checkpoints rather than validation boundaries and do not each need to pass independently. PR text, comments, labels, and other metadata do not invalidate code validation.

### Platform Setup
- `./script/bootstrap` - Platform-specific setup: Xcode, Homebrew packages, Rust targets and Git LFS.
- `./script/install_cargo_build_deps` - Install Cargo build dependencies
- `./script/install_cargo_test_deps` - Install Cargo test dependencies

## Architecture Overview

This is a Rust-based terminal emulator with a custom UI framework called **TorchUI**, inherited from Warp. Its single front-end is the GUI desktop app in `app/`.

### Key Components

**UI core** (`crates/torchui`, `crates/torchui_core`):
- Entity-Component-Handle pattern: a global `App` object owns all views/models (entities); views hold `ViewHandle<T>` references to other views; `AppContext` provides temporary access to handles during render/events.
- Actions system for event handling.

**GUI rendering** (TorchUI GUI elements):
- `Element`s describe visual layout (Flutter-inspired), rendered on the GPU (WGSL).
- Mouse input uses `MouseStateHandle`: create it once during construction and reference/clone it wherever mouse input is tracked. An inline `MouseStateHandle::default()` while rendering means no mouse interactions work.

**Main app** (`app/`):
- Terminal emulation and shell management (`terminal/`)
- Settings and preferences (`settings/`)
- Workspace and session management (`workspace/`)

**Core Libraries**:
- `crates/torch_core/` - Core utilities and platform abstractions
- `crates/editor/` - Text editing functionality
- `crates/torchui/` and `crates/torchui_core/` - Custom UI framework (core plus the GUI element library)
- `crates/ipc/` - Inter-process communication

### Key Architectural Patterns

1. **Entity-Handle System**: Views reference other views via handles, not direct ownership
2. **Modular Structure**: Workspace contains multiple workspace configurations, each with terminals, notebooks, etc.
3. **Cross-Platform**: Native implementations for macOS, Windows, Linux, plus WASM target

### Development Guidelines

**Workspace Structure**:
- This is a Cargo workspace with 60+ member crates
- Main binary is in `app/`, UI framework in `crates/torchui/`
- Platform-specific code is conditionally compiled
- Integration tests are in `crates/integration/`

**Coding Style Preferences**:
- Avoid unnecessary type annotations, especially in closure params.
- Avoid using too many Rust path qualifiers and use imports for concision. Place import statements at the top of the file as per convention.
  An exception to this is inside cfg-guarded code branches. In those cases, you can either embed the import into the relevant scope or just use an absolute path for one-offs.
- If a function takes a context parameter (`AppContext`, `ViewContext`, or `ModelContext`), it should be named `ctx` and go last. The one exception is for
  functions that take a closure parameter, in which case the closure should be last.
- Always remove unused parameters completely rather than prefixing them with `_`. Update the function signature and all call sites accordingly.
- Prefer inline format arguments in macros like `println!`, `eprintln!`, and `format!` (for example, `eprintln!("{message}")` instead of `eprintln!("{}", message)`) to satisfy Clippy's `uninlined_format_args` lint.
- Do not pass `Itertools::format` results directly to logging macros (`log::*`, `safe_*`, etc.). `Itertools::format` produces a single-use formatter, while logging implementations may format a message more than once. Use a reusable `String` such as `iter.join(", ")` for logging arguments instead. Direct use in `format!` or `write!` is fine.
- When adding a toggleable setting, also add the matching Command Palette enable/disable entry and any required context flags so the setting is discoverable outside Settings.

**Comments**:
Comments have a cost. They carry a maintenance burden, because they must be kept in sync
with the code they describe. It is tempting to assume that more comments is always better,
but be judicious about when a comment is actually necessary because the code cannot speak
for itself.
- **Minimalist Comments**: Assume the reader is a Senior Software Engineer. Never comment
  to explain WHAT or HOW code works if self-documenting names accomplish that.
- **Strictly "Why" Only**: Reserve inline comments strictly for non-obvious business
  rationale, workarounds for third-party bugs, complex algorithms, unidiomatic code, or
  unexpected edge cases.
- **No Line-by-Line Narrations**: Never add comments restating the syntax (e.g., omit
  `# Initialize array`, `# Loop over users`).
- **Clean Docstrings**: Keep doc comments concise. Document public APIs, arguments, types,
  and returns. Do not narrate the method's internal implementation steps.
- **Single-source of documentation**: For items/members that have a doc comment explaining
  their purpose, you do not need to repeat that explanation anywhere else. A good example
  is a float const specifying an amount of spacing. You may use a doc comment on the
  declaration if necessary, but do not repeat that where the const is *referenced*. Another
  example is function call sites. Function doc comments explain what they do. Do not repeat
  the explanation at the call site.
- **Container docs describe the whole, member docs describe the parts**: A field's, variant's,
  or parameter's own doc comment is where that member gets explained. A container's item-level
  doc comment (struct, enum, trait) describes the item as a whole and must not enumerate or
  re-explain its members. Do not name members in the container's doc comment just to describe
  them, and do not restate in a member's doc comment what the container's doc comment already
  said. Behavior genuinely shared by several members belongs in exactly one place, not both.
- **Don't enumerate function call sites in doc comments**: Function doc comments should
  document their behavior and NOT their callers, e.g. it should never say things like,
  "this is used by [certain callers]" or "this is used when...".
- **No "transformation comments"**: Do not add comments that explain *your edits*. Comments
  only need explain the *current state* of the code. Explanations of edits belong in pull
  request comments instead. You shouldn't add comments with phrases like, "this used to do
  so-and-so".
- Do not remove existing comments when making unrelated changes. Only remove or modify a
  comment if the logic it describes has changed.
- The formatter (`./script/format`) is configured with a `max_width` (max line length) of
  100. Flow (reflow) comment line-wrapping to fill that full width rather than wrapping
  early at a narrower column, so comments span as few lines as possible.

**Terminal Model Locking**:
- Be extremely careful when calling `model.lock()` on the terminal model (`TerminalModel`). Acquiring multiple locks on the same model from different call sites can cause a deadlock, resulting in a UI freeze (beach ball on macOS).
- Before adding a new `model.lock()` call, verify that no caller in the current call stack already holds the lock.
- Prefer passing already-locked model references down the call stack rather than acquiring new locks.
- If you must lock the model, keep the lock scope as short as possible and avoid calling other functions that might also attempt to lock.

**Testing**:
- Use `cargo nextest` for parallel test execution
- Integration tests use the custom framework in `crates/integration/`.
- Follow the Implementation Validation Order above; do not add a full presubmit run after targeted tests unless it was explicitly required.
- Unit tests should be placed in separate files using the naming convention `${filename}_tests.rs` or `mod_test.rs`
- Test files should be included at the end of their corresponding module with:
  ```rust
  #[cfg(test)]
  #[path = "filename_tests.rs"]  // or "mod_test.rs"
  mod tests;
  ```

**Pull Request Workflow**:
- Follow the Implementation Validation Order before opening a PR or pushing a code update. Do not repeat validation when the candidate has not changed.
- CI is the broad cross-platform and workspace gate. Push once the targeted tests and lint checks pass and the formatter has run; address a later CI failure as a new revision.
- Do not create public pull requests or public issues that disclose a non-public security vulnerability. Refer users to `SECURITY.md` for the proper disclosure methods instead.
 - When opening PRs, use the PR template at `.github/pull_request_template.md`
 - Add changelog entries when appropriate using the format at the bottom of the PR template. Use the following prefixes (without the `{{}}` brackets):
   - `CHANGELOG-NEW-FEATURE:` for new, relatively sizable features (use sparingly - these may get marketing/docs)
   - `CHANGELOG-IMPROVEMENT:` for new functionality of existing features
   - `CHANGELOG-BUG-FIX:` for fixes related to known bugs or regressions
   - `CHANGELOG-IMAGE:` for GCP-hosted image URLs
   - Leave changelog lines blank or remove them if no changelog entry is needed

**Database**:
- Uses Diesel ORM with SQLite
- Migrations in `crates/persistence/migrations/`
- Schema defined in `crates/persistence/src/schema.rs`

### Feature Flags

Torch uses compile-time feature flags with a small runtime plumbing layer.

How to add a feature flag:
- Add a new variant to `torch_core/src/features.rs` in the `FeatureFlag` enum
- (Optional) Enable it by default for dogfood builds by listing it in `DOGFOOD_FLAGS`
- Gate code paths with `FeatureFlag::YourFlag.is_enabled()`
- For preview or release rollout, add to `PREVIEW_FLAGS` or `RELEASE_FLAGS` respectively (as appropriate)

Best practices:
- **Prefer runtime checks over cfg directives**: Prefer `FeatureFlag::YourFlag.is_enabled()` over `#[cfg(...)]` compile-time directives so flags can be toggled without recompilation and are easier to clean up later. Use `#[cfg(...)]` only when the code cannot compile without them (for example, platform-specific code or dependencies that do not exist when the feature is disabled).
- Keep flags high-level and product-focused rather than per-call-site
- Remove the flag and dead branches after launch has stabilized
- For UI sections that expose a new feature, hide the UI behind the same flag

Example:
```rust
#[derive(Sequence)]
pub enum FeatureFlag {
    YourNewFeature,
}

// Default-on for dogfood builds
pub const DOGFOOD_FLAGS: &[FeatureFlag] = &[
    FeatureFlag::YourNewFeature,
];

// Use in code
if FeatureFlag::YourNewFeature.is_enabled() {
    // gated behavior
}
```

### Exhaustive Matching

When adding/editing match statements, avoid using the wildcard _ when at all possible. Exhaustive matching is helpful for ensuring that all variants are handled, especially when adding new variants to enums in the future.
