use std::fs::File;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use channel_versions::VersionInfo;
use command::blocking::Command;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use tempfile::TempPath;
use torch_core::channel::{Channel, ChannelState};

use super::{DownloadReady, release_assets_directory_url};
use crate::util::windows::install_dir;

lazy_static! {
    /// The path to the temporary file that stores the installer for the new update.
    static ref INSTALLER_PATH: Arc<Mutex<Option<TempPath>>> = Default::default();
}

/// Download the Inno Setup install wizard, the same one users run on the first Torch install, and
/// place it into the "data dir".
pub(super) async fn download_update_and_cleanup(
    version_info: &VersionInfo,
    _update_id: &str,
    client: &http_client::Client,
) -> Result<DownloadReady> {
    const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

    let installer_file_name = installer_file_name()?;
    let url = format!(
        "{}/{}",
        release_assets_directory_url(ChannelState::channel(), &version_info.version),
        installer_file_name
    );

    // Create a temporary file that we'll write the download into.
    let mut already_exists = false;
    let mut new_installer = tempfile::Builder::new()
        .rand_bytes(0)
        .suffix(&format!("{}-{}", version_info.version, installer_file_name))
        .make(|path| {
            // Treat a 0-byte file as missing.
            let non_empty = path.metadata().map(|m| m.len() > 0).unwrap_or(false);
            already_exists = non_empty;
            if already_exists {
                File::open(path)
            } else {
                File::create(path)
            }
        })?;

    if !already_exists {
        log::info!("Downloading {url} to {}...", new_installer.path().display());

        let response = client
            .get(&url)
            .timeout(DOWNLOAD_TIMEOUT)
            .send()
            .await?
            .error_for_status()?;
        new_installer
            .as_file_mut()
            .write_all(&response.bytes().await?)?;
    }

    *INSTALLER_PATH.lock() = Some(new_installer.into_temp_path());

    Ok(DownloadReady::Yes)
}

const UPDATE_LOG_FILENAME: &str = "torch_update.log";

fn autoupdate_log_file() -> Result<PathBuf> {
    torch_logging::log_directory().map(|dir| dir.join(UPDATE_LOG_FILENAME))
}

pub(super) fn relaunch() -> Result<()> {
    let install_dir = install_dir()?;
    let Some(installer_path) = INSTALLER_PATH
        .lock()
        .as_ref()
        .map(|path| path.to_path_buf())
    else {
        bail!("No installer path");
    };

    let log_arg = match autoupdate_log_file() {
        Ok(dir) => format!("/LOG={}", dir.display()),
        Err(e) => {
            log::warn!("Failed to determine location for autoupdate logs: {e:#}");
            "/LOG".to_string()
        }
    };

    // The Inno Setup install wizard will run without user input. It will re-launch Torch after
    // installing the update files.
    // https://jrsoftware.org/ishelp/index.php?topic=setupcmdline
    Command::new(&installer_path)
        .args([
            // Skip asking the user to confirm.
            "/SP-",
            // Do not prompt the user for anything. Note that we do not use "VERYSILENT" so that a
            // progress bar is still shown. This is useful since the update process may take a few
            // seconds.
            "/SILENT",
            // Do not provide a cancel button on the progress bar page.
            "/NOCANCEL",
            // Indicate that restarting Windows is not necessary.
            "/NORESTART",
            &log_arg,
            "/update=1",
            // Do not forcibly kill Torch via RestartManager. The installer will wait for
            // Torch to exit naturally by polling the single-instance mutex instead.
            "/NOCLOSEAPPLICATIONS",
            &format!("/DIR={}", install_dir.display()),
        ])
        .spawn()?;

    Ok(())
}

fn installer_file_name() -> Result<String> {
    let app_name_prefix = app_name_prefix(ChannelState::channel());

    // For example, on arm64 this is TorchSetup-arm64.exe and on x64 this is
    // TorchSetup.exe.
    if cfg!(target_arch = "aarch64") {
        Ok(format!("{app_name_prefix}Setup-arm64.exe"))
    } else if cfg!(target_arch = "x86_64") {
        Ok(format!("{app_name_prefix}Setup.exe"))
    } else {
        Err(anyhow!(
            "Could not construct setup file name for unsupported architecture"
        ))
    }
}

fn app_name_prefix(channel: Channel) -> &'static str {
    match channel {
        Channel::Stable => "Torch",
        Channel::Preview => "WarpPreview",
        Channel::Local => "warp",
        Channel::Integration => "integration",
        Channel::Dev => "WarpDev",
        Channel::Oss => "Torch",
    }
}

