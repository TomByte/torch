use crate::server::ids::ServerId;

/// The team an outbound request is scoped to, as sent in `X-Torch-Team-Uid`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RequestTeamScope(Option<ServerId>);

impl RequestTeamScope {
    /// The wire uid. `None` sends no team header, leaving the server to its own default.
    pub(crate) fn team_uid(self) -> Option<ServerId> {
        self.0
    }
}
