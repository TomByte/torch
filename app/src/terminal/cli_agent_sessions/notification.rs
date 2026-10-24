//! Desktop notification text for CLI agent sessions in local-only builds.
use torchui::notification::UserNotification;

use super::{BlockedSource, CLIAgentSessionStatus};

/// What a notification says about a CLI agent session.
pub(crate) struct CLIAgentNotificationText {
    pub title: String,
    pub body: String,
}

/// Session details a notification describes.
pub(crate) struct CLIAgentNotificationSubject<'a> {
    pub agent_name: &'a str,
    pub session_title: Option<&'a str>,
    pub group_label: Option<&'a str>,
    pub tool_name: Option<&'a str>,
}

/// Returns `None` for statuses that need no notification.
pub(crate) fn notification_text(
    subject: &CLIAgentNotificationSubject,
    status: &CLIAgentSessionStatus,
) -> Option<CLIAgentNotificationText> {
    let what = match status {
        CLIAgentSessionStatus::Blocked {
            source: BlockedSource::PermissionRequest,
            ..
        } => match subject.tool_name.filter(|tool| !tool.trim().is_empty()) {
            Some(tool) => format!("{} wants to use {}", subject.agent_name, tool.trim()),
            None => "Needs your input".to_owned(),
        },
        CLIAgentSessionStatus::Blocked {
            source: BlockedSource::QuestionAsked | BlockedSource::NeedsInput,
            ..
        } => "Needs your input".to_owned(),
        CLIAgentSessionStatus::Success => "Finished".to_owned(),
        CLIAgentSessionStatus::Failed { .. } => "Failed".to_owned(),
        CLIAgentSessionStatus::InProgress => return None,
    };
    let title = subject
        .session_title
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or(subject.agent_name);
    let body = match subject.group_label.filter(|label| !label.is_empty()) {
        Some(label) => format!("{label} · {what}"),
        None => what,
    };
    Some(CLIAgentNotificationText {
        title: truncate(&single_line(title), UserNotification::MAX_TITLE_LENGTH),
        body: truncate(&single_line(&body), UserNotification::MAX_BODY_LENGTH),
    })
}

fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

#[cfg(test)]
#[path = "notification_tests.rs"]
mod tests;
