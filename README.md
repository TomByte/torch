# Torch

Torch is a local terminal based on Warp, maintained independently at [TomByte/torch](https://github.com/TomByte/torch).

- Opens directly into your shell without an account or login screen.
- Keeps tab completion, command history, ordinary autosuggestions, terminal blocks, and local settings.
- Disables built-in AI predictions, natural-language input detection, AI input hints, and account-backed services.
- Removes account, billing, teams, cloud, and agent settings from navigation and search.
- Uses its own `dev.tombyte.Torch` application identity and data directory (`~/.torch` for portable configuration on macOS).
- Ships without Warp telemetry, crash reporting, automatic updates, or upstream repository automation.

## Build and run

The workspace uses Rust 1.92.0 (see `rust-toolchain.toml`), Git LFS, and platform build dependencies. On macOS, install Xcode including its Metal Toolchain, CMake, and `cargo-bundle` before building. `./script/bootstrap` can install platform dependencies; it skips Warp account authentication and common agent skill installation by default.

```sh
git lfs install --local
git lfs pull
cargo check -p warp --bin torch --features gui
./script/run
```

On macOS, `./script/run` creates `target/debug/bundle/osx/Torch.app`. Linux uses `cargo run --bin torch --features gui,local_only`. The internal Cargo package remains named `warp` to preserve workspace compatibility; the shipped executable is `torch`.

`local_only` is enabled by default and explicitly by the run script. The Torch executable requires this feature, including for custom builds. The inherited cloud and AI implementation remains in the source for shared type compatibility, but the local-only policy prevents authentication, AI enablement, cloud GraphQL requests, and account UI. Ordinary CLI tools, including separately installed agent CLIs, can still run in the shell.

## Validation

```sh
cargo test -p warp_features --features local_only,test-util
cargo clippy -p warp_features --features local_only,test-util --all-targets -- -D warnings
cargo check -p warp --bin torch --features gui
```

Upstream tests that explicitly exercise authenticated or AI-enabled behavior describe the original product and are not a Torch acceptance suite. Test local-only policy and ordinary terminal behavior when changing this fork.

The initial Torch changes were validated on macOS with a native app build, shell execution and Tab-completion smoke tests, 14 path tests, and four feature-policy tests. Linux and Windows packaging changes have not been run on those platforms. Rust 1.97 reports inherited style warnings under strict whole-app Clippy; targeted policy/core Clippy checks pass.

## Source and licences

Imported from [warpdotdev/warp](https://github.com/warpdotdev/warp), commit `cd4b970e6331aa7e9c18bdd441ab0a1b841a0b1c`, on 20 October 2026, into a new Git history. There is no upstream remote or GitHub fork relationship. The only project remote is `TomByte/torch`.

The original copyright notices and licences are retained. Most of the source is [AGPL-3.0](LICENSE-AGPL); `warpui` and `warpui_core` are [MIT](LICENSE-MIT). Torch is independent of Warp and Denver Technologies, Inc.

Internal crate names, protocol identifiers, shell integration variables, dependency URLs, and historical technical documentation may retain Warp names for compatibility and attribution. Public dependencies hosted by warpdotdev remain pinned in Cargo manifests/lockfile; repository remotes and release automation are independent.
