use std::env;
use std::fs::read_to_string;
use std::time::Duration;

use anyhow::{Context as _, Result};
use channel_versions::ChannelVersions;

use crate::channel::ChannelState;

const FETCH_CHANNEL_VERSIONS_TIMEOUT: Duration = Duration::from_secs(60);

pub async fn fetch_channel_versions(client: &http_client::Client) -> Result<ChannelVersions> {
    if let Ok(path) = env::var("TORCH_CHANNEL_VERSIONS_PATH") {
        // Load channel versions from local filesystem. Used for testing both
        // autoupdate and changelog behavior.
        let path = shellexpand::tilde(&path);
        let channel_versions_string = read_to_string::<&str>(&path)?;
        return serde_json::from_str(channel_versions_string.as_str())
            .context("Failed to parse channel versions JSON");
    }

    fetch_channel_versions_from_github_release(client).await
}

/// Fetches the [`ChannelVersions`] published with the latest GitHub release. GitHub's `latest`
/// skips drafts and prereleases, so only full releases are offered as updates.
async fn fetch_channel_versions_from_github_release(
    client: &http_client::Client,
) -> Result<ChannelVersions> {
    let url = format!(
        "{}/latest/download/channel_versions.json",
        ChannelState::releases_base_url()
    );
    log::info!("Fetching channel versions from {url}");
    let versions: ChannelVersions = client
        .get(url.as_str())
        .timeout(FETCH_CHANNEL_VERSIONS_TIMEOUT)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    log::info!("Received channel versions from the latest release: {versions}");
    Ok(versions)
}
