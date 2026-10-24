// On Windows, we don't want to display a console window when the application is running in release
// builds. See https://doc.rust-lang.org/reference/runtime.html#the-windows_subsystem-attribute.
#![cfg_attr(feature = "release_bundle", windows_subsystem = "windows")]

use anyhow::Result;
use torch_core::AppId;
use torch_core::channel::{AutoupdateConfig, Channel, ChannelConfig, ChannelState};

// Simple wrapper around torch::run() for Torch OSS builds.
fn main() -> Result<()> {
    if torch::local_hooks::run_if_requested() {
        return Ok(());
    }
    let state = ChannelState::new(
        Channel::Oss,
        ChannelConfig {
            app_id: AppId::new("dev", "tombyte", "Torch"),
            logfile_name: "torch.log".into(),
            // Updates are published as GitHub releases, which only carry macOS builds.
            autoupdate_config: cfg!(target_os = "macos").then(|| AutoupdateConfig {
                releases_base_url: "https://github.com/TomByte/torch/releases".into(),
                show_autoupdate_menu_items: true,
            }),
        },
    );
    ChannelState::set(state);

    torch::run()
}

// If we're not using an external plist, embed the following as the Info.plist.
#[cfg(all(not(feature = "extern_plist"), target_os = "macos"))]
embed_plist::embed_info_plist_bytes!(r#"
    <?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE plist PUBLIC "-//Apple Computer//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
    <plist version="1.0">
    <dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>English</string>
    <key>CFBundleDisplayName</key>
    <string>Torch</string>
    <key>CFBundleExecutable</key>
    <string>torch</string>
    <key>CFBundleIdentifier</key>
    <string>dev.tombyte.Torch</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>Torch</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.developer-tools</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>UIDesignRequiresCompatibility</key>
    <true/>
    <key>CFBundleURLTypes</key>
    <array><dict><key>CFBundleURLName</key><string>Custom App</string><key>CFBundleURLSchemes</key><array><string>torch</string></array></dict></array>
    <key>NSHumanReadableCopyright</key>
    <string>© 2026, Denver Technologies, Inc</string>
    </dict>
    </plist>
"#.as_bytes());
