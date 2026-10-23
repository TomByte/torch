//! CLI agent conversations that can be resumed in a restored terminal pane.
use serde::{Deserialize, Serialize};

use crate::terminal::CLIAgent;

const MAX_SESSION_ID_LEN: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ResumableAgent {
    Claude,
    Codex,
}

/// An agent conversation that was running in a pane when its state was saved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumableAgentSession {
    agent: ResumableAgent,
    session_id: String,
}

impl ResumableAgentSession {
    /// Returns `None` for agents without a resume command, or for session IDs that are not plain
    /// tokens, since the ID is placed on a shell command line.
    pub fn new(agent: CLIAgent, session_id: &str) -> Option<Self> {
        let agent = match agent {
            CLIAgent::Claude => ResumableAgent::Claude,
            CLIAgent::Codex => ResumableAgent::Codex,
            CLIAgent::Gemini
            | CLIAgent::Amp
            | CLIAgent::Droid
            | CLIAgent::OpenCode
            | CLIAgent::Copilot
            | CLIAgent::Pi
            | CLIAgent::OhMyPi
            | CLIAgent::Auggie
            | CLIAgent::CursorCli
            | CLIAgent::Goose
            | CLIAgent::Hermes
            | CLIAgent::Vibe
            | CLIAgent::Antigravity
            | CLIAgent::Grok
            | CLIAgent::Kiro
            | CLIAgent::Muse
            | CLIAgent::Unknown => return None,
        };
        let is_token = !session_id.is_empty()
            && session_id.len() <= MAX_SESSION_ID_LEN
            && session_id
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'));
        is_token.then(|| Self {
            agent,
            session_id: session_id.to_owned(),
        })
    }

    pub fn from_json(json: &str) -> Option<Self> {
        let session: Self = serde_json::from_str(json).ok()?;
        let agent = match session.agent {
            ResumableAgent::Claude => CLIAgent::Claude,
            ResumableAgent::Codex => CLIAgent::Codex,
        };
        Self::new(agent, &session.session_id)
    }

    pub fn to_json(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }

    pub fn resume_command(&self) -> String {
        match self.agent {
            ResumableAgent::Claude => format!("claude --resume {}", self.session_id),
            ResumableAgent::Codex => format!("codex resume {}", self.session_id),
        }
    }
}

#[cfg(test)]
#[path = "resume_tests.rs"]
mod tests;
