//! Tips for cloud mode loading screen.

use torchui::AppContext;
use torchui::keymap::Keystroke;

use crate::ai::agent_tips::AITip;

/// A cloud mode tip.
#[derive(Clone, Debug)]
pub struct CloudModeTip {
    text: String,
}

impl CloudModeTip {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

impl AITip for CloudModeTip {
    fn keystroke(&self, _app: &AppContext) -> Option<Keystroke> {
        None
    }

    fn link(&self) -> Option<String> {
        None
    }

    fn description(&self) -> &str {
        &self.text
    }

    // Uses the default implementation which adds "Tip: " prefix and parses backticks as inline code
}

/// Returns a collection of tips for the cloud mode loading screen.
pub fn get_cloud_mode_tips() -> Vec<CloudModeTip> {
    vec![
        CloudModeTip::new(
            "Install the Oz Slack integration to trigger agents from any channel or DM.",
        ),
        CloudModeTip::new("Build programmatic agents using Oz's TypeScript and Python SDKs."),
        CloudModeTip::new("Set team or personal secrets for agents using the `oz secret` command."),
        CloudModeTip::new("View all your agent runs and their status in the Oz web app."),
        CloudModeTip::new("Join any Oz cloud agent run in real-time using Agent Session Sharing."),
        CloudModeTip::new(
            "Set up recurring agents that run on cron schedules for automated maintenance.",
        ),
        CloudModeTip::new(
            "Create agents that automatically fix bugs when issues are filed in Linear.",
        ),
        CloudModeTip::new("Build agents that respond to CI failures and attempt automatic fixes."),
        CloudModeTip::new("Run agents from GitHub Actions using the `oz-agent-action`."),
        CloudModeTip::new(
            "Call the Oz REST API to trigger agents from any backend service or internal tool.",
        ),
        CloudModeTip::new(
            "Create reusable environments with Docker images for consistent agent execution.",
        ),
        CloudModeTip::new("Share agent session links with your team for collaborative debugging."),
        CloudModeTip::new(
            "Use the `--share` flag with the Oz CLI to enable session sharing from anywhere.",
        ),
        CloudModeTip::new(
            "Fork a completed Oz cloud agent session into Torch to continue the work locally.",
        ),
        CloudModeTip::new(
            "Build internal tools that use agents to answer questions from your databases.",
        ),
        CloudModeTip::new("Create a scheduled agent to clean up stale feature flags every week."),
        CloudModeTip::new(
            "Tag @Oz in Linear issues to automatically investigate and propose fixes.",
        ),
        CloudModeTip::new("Run agents on remote dev boxes or CI runners using the Oz CLI."),
        CloudModeTip::new(
            "Configure MCP servers to give Oz cloud agents access to GitHub, Linear, and Sentry.",
        ),
        CloudModeTip::new(
            "Use `oz agent run` to kick off tasks without opening the Torch terminal.",
        ),
        CloudModeTip::new(
            "View your teammates' agent runs in the Oz web app for shared visibility.",
        ),
        CloudModeTip::new(
            "Build agents that automatically triage and label incoming GitHub issues.",
        ),
        CloudModeTip::new("Set up an agent to generate daily summaries of newly opened issues."),
        CloudModeTip::new(
            "Create an agent that automatically reviews PRs and suggests improvements.",
        ),
        CloudModeTip::new("Use `oz environment create` to define reproducible execution contexts."),
        CloudModeTip::new("Trigger agents from webhooks to respond to production incidents."),
        CloudModeTip::new(
            "Build an agent that restarts services or scales deployments when alerts fire.",
        ),
        CloudModeTip::new(
            "Use personal secrets for credentials that should only be used by your agents.",
        ),
        CloudModeTip::new(
            "Use team secrets for shared infrastructure credentials across all agents.",
        ),
        CloudModeTip::new("Create an agent that runs nightly to check for dependency updates."),
        CloudModeTip::new(
            "Build an agent that automatically formats and lints code on a schedule.",
        ),
        CloudModeTip::new("Use `oz schedule create` to set up cron-triggered agents."),
        CloudModeTip::new(
            "Pause and resume scheduled agents without deleting them using `oz schedule pause`.",
        ),
        CloudModeTip::new(
            "Use `oz mcp list` to see which MCP servers are available to your agents.",
        ),
        CloudModeTip::new("Build an internal Slack bot that delegates coding tasks to Oz agents."),
        CloudModeTip::new(
            "Create an agent that responds to @mentions in Slack threads with full context.",
        ),
        CloudModeTip::new("Use the Oz TypeScript SDK to build custom automation pipelines."),
        CloudModeTip::new("Use the Oz Python SDK to integrate agents into your data pipelines."),
        CloudModeTip::new("Monitor agent success rates and runtimes using the Oz API."),
        CloudModeTip::new("Build a dashboard that tracks all agent activity across your team."),
    ]
}
