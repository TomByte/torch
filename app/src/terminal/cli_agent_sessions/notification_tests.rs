use super::*;

fn subject<'a>(
    session_title: Option<&'a str>,
    group_label: Option<&'a str>,
    tool_name: Option<&'a str>,
) -> CLIAgentNotificationSubject<'a> {
    CLIAgentNotificationSubject {
        agent_name: "Claude",
        session_title,
        group_label,
        tool_name,
    }
}

fn blocked(source: BlockedSource) -> CLIAgentSessionStatus {
    CLIAgentSessionStatus::Blocked {
        message: Some("private summary".to_owned()),
        source,
    }
}

#[test]
fn permission_requests_name_the_tool_and_project() {
    let text = notification_text(
        &subject(Some("Fix login"), Some("torch"), Some("Bash")),
        &blocked(BlockedSource::PermissionRequest),
    )
    .unwrap();
    assert_eq!(text.title, "Fix login");
    assert_eq!(text.body, "torch · Claude wants to use Bash");
}

#[test]
fn questions_and_unnamed_permissions_need_input() {
    for status in [
        blocked(BlockedSource::QuestionAsked),
        blocked(BlockedSource::NeedsInput),
        blocked(BlockedSource::PermissionRequest),
    ] {
        let text = notification_text(&subject(None, Some("torch"), None), &status).unwrap();
        assert_eq!(text.title, "Claude");
        assert_eq!(text.body, "torch · Needs your input");
        assert!(!text.body.contains("private"));
    }
}

#[test]
fn completion_and_failure_without_group() {
    let finished = notification_text(
        &subject(Some("Refactor"), None, None),
        &CLIAgentSessionStatus::Success,
    )
    .unwrap();
    assert_eq!(finished.body, "Finished");
    let failed = notification_text(
        &subject(Some("Refactor"), Some(""), None),
        &CLIAgentSessionStatus::Failed {
            error_type: None,
            message: Some("secret".to_owned()),
        },
    )
    .unwrap();
    assert_eq!(failed.body, "Failed");
}

#[test]
fn working_sessions_do_not_notify() {
    assert!(
        notification_text(
            &subject(None, None, None),
            &CLIAgentSessionStatus::InProgress
        )
        .is_none()
    );
}

#[test]
fn long_titles_are_single_line_and_truncated() {
    let title = "A very long\nsession title that keeps going well past the limit";
    let text = notification_text(
        &subject(Some(title), Some("torch"), None),
        &CLIAgentSessionStatus::Success,
    )
    .unwrap();
    assert!(!text.title.contains('\n'));
    assert!(text.title.chars().count() <= UserNotification::MAX_TITLE_LENGTH);
    assert!(text.title.ends_with('…'));
}
