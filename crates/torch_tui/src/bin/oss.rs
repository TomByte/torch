//! Torch's `torch-tui` binary and `default-run` target.
//!
//! This is what bare `cargo run -p torch_tui` builds. Like `app/src/bin/oss.rs`, it hand-builds a
//! config with Warp's cloud services disabled. It is a console application (no GUI window,
//! no app bundle), so unlike the GUI binaries it sets no `windows_subsystem`
//! attribute and embeds no `Info.plist`.

use anyhow::Result;
use torch_core::AppId;
use torch_core::channel::{Channel, ChannelConfig, ChannelState, OzConfig, WarpServerConfig};

fn main() -> Result<()> {
    let mut state = ChannelState::new(
        Channel::Oss,
        ChannelConfig {
            app_id: AppId::new("dev", "tombyte", "TorchTui"),
            logfile_name: "torch-tui.log".into(),
            server_config: WarpServerConfig::disabled(),
            oz_config: OzConfig::disabled(),
            autoupdate_config: None,
            mcp_static_config: None,
        },
    );
    if cfg!(debug_assertions) {
        state = state.with_additional_features(torch_core::features::DEBUG_FLAGS);
    }
    ChannelState::set(state);

    torch_tui::run()
}
