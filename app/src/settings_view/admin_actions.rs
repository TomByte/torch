use crate::channel::ChannelState;
use crate::server::ids::ServerId;

/// Shared admin panel actions and utilities for settings views
pub struct AdminActions;

impl AdminActions {
    /// Generate the admin panel URL for a given team
    pub fn admin_panel_link_for_team(team_uid: ServerId) -> String {
        format!("{}/admin/{}", ChannelState::server_root_url(), team_uid)
    }

    pub fn admin_panel_link_for_workspace() -> String {
        format!("{}/admin", ChannelState::server_root_url())
    }
}
